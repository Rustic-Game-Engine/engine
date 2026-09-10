use crate::{
    EngineValue, GameplayHost, JavaScriptBehavior, JavaScriptRuntimeError, ScriptId,
    ScriptLanguage, validate_javascript,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::ffi::{OsStr, OsString};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tempfile::TempDir;
use thiserror::Error;

const MAX_DIAGNOSTIC_BYTES: usize = 16 * 1024;
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageAvailability {
    pub language: ScriptLanguage,
    pub available: bool,
    pub executable: Option<PathBuf>,
    pub version: Option<String>,
    pub detail: String,
}

#[derive(Debug, Error)]
pub enum ExternalRuntimeError {
    #[error("{0} source exceeds the {1} byte limit")]
    SourceTooLarge(&'static str, usize),
    #[error("{0} source is not valid UTF-8: {1}")]
    InvalidUtf8(&'static str, String),
    #[error("{0} toolchain is unavailable; install {1} and ensure it is on PATH")]
    ToolchainUnavailable(&'static str, &'static str),
    #[error("{0} validation/build failed: {1}")]
    Build(&'static str, String),
    #[error("{0} runtime failed: {1}")]
    Runtime(&'static str, String),
    #[error("{0} callback exceeded the {1:?} limit")]
    Timeout(&'static str, Duration),
    #[error("{0} returned more than {1} bytes")]
    OversizedResponse(&'static str, u64),
    #[error("HTML/CSS validation failed: {0}")]
    Web(String),
}

/// Locates an optional external toolchain and records its version without executing project code.
pub fn probe_language_toolchain(language: ScriptLanguage) -> LanguageAvailability {
    if matches!(
        language,
        ScriptLanguage::Lua54 | ScriptLanguage::JavaScript | ScriptLanguage::Web
    ) {
        return LanguageAvailability {
            language,
            available: true,
            executable: None,
            version: Some(env!("CARGO_PKG_VERSION").into()),
            detail: "runtime is bundled with Rustic".into(),
        };
    }
    let specification = toolchain_spec(language);
    let executable = specification.and_then(|spec| find_executable(spec.candidates));
    let version = executable.as_ref().and_then(|path| {
        let spec = specification?;
        let output = Command::new(path)
            .args(spec.version_arguments)
            .output()
            .ok()?;
        let text = if output.stdout.is_empty() {
            String::from_utf8_lossy(&output.stderr)
        } else {
            String::from_utf8_lossy(&output.stdout)
        };
        text.lines().next().map(str::trim).map(str::to_owned)
    });
    LanguageAvailability {
        language,
        available: executable.is_some(),
        executable,
        version,
        detail: specification.map_or_else(
            || "no external toolchain is defined".into(),
            |spec| format!("expected {}", spec.install_hint),
        ),
    }
}

/// Performs compile-only validation for an external language or structural Web validation.
///
/// # Errors
/// Returns bounded diagnostics for invalid source, failed compilation, or a missing toolchain.
pub fn validate_external(
    language: ScriptLanguage,
    source: &[u8],
    source_name: &str,
    maximum_bytes: usize,
) -> Result<(), ExternalRuntimeError> {
    validate_source_bytes(language, source, maximum_bytes)?;
    if language == ScriptLanguage::Web {
        return validate_web(source, source_name);
    }
    if language == ScriptLanguage::Luau {
        return validate_with_stdin(language, source, &["--compile=-", "-"]);
    }
    PreparedProgram::build(language, source, source_name).map(|_| ())
}

/// A bounded, out-of-process behavior for toolchain-hosted languages.
pub struct ExternalBehavior {
    script_id: ScriptId,
    language: ScriptLanguage,
    session: ProcessSession,
    _program: PreparedProgram,
    host: Arc<Mutex<Box<dyn GameplayHost>>>,
    enabled: bool,
}

impl std::fmt::Debug for ExternalBehavior {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalBehavior")
            .field("script_id", &self.script_id)
            .field("language", &self.language)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl ExternalBehavior {
    /// Builds or prepares one isolated external behavior.
    ///
    /// # Errors
    /// Returns a source, toolchain, or build diagnostic.
    pub fn load(
        script_id: ScriptId,
        language: ScriptLanguage,
        source: &[u8],
        source_name: &str,
        host: Box<dyn GameplayHost>,
    ) -> Result<Self, ExternalRuntimeError> {
        Self::load_shared(
            script_id,
            language,
            source,
            source_name,
            Arc::new(Mutex::new(host)),
        )
    }

    /// Builds a replacement while retaining engine-owned world/property state.
    ///
    /// # Errors
    /// Returns a source, toolchain, or build diagnostic without altering the old generation.
    pub fn load_shared(
        script_id: ScriptId,
        language: ScriptLanguage,
        source: &[u8],
        source_name: &str,
        host: Arc<Mutex<Box<dyn GameplayHost>>>,
    ) -> Result<Self, ExternalRuntimeError> {
        validate_source_bytes(language, source, 1024 * 1024)?;
        let program = PreparedProgram::build(language, source, source_name)?;
        let session = ProcessSession::start(&program)?;
        Ok(Self {
            script_id,
            language,
            session,
            _program: program,
            host,
            enabled: true,
        })
    }

    pub const fn script_id(&self) -> ScriptId {
        self.script_id
    }

    pub const fn language(&self) -> ScriptLanguage {
        self.language
    }

    pub fn host(&self) -> Arc<Mutex<Box<dyn GameplayHost>>> {
        Arc::clone(&self.host)
    }

    /// # Errors
    /// Returns and contains a callback process/protocol failure.
    pub fn on_create(&mut self) -> Result<(), ExternalRuntimeError> {
        self.call("on_create", None)
    }
    /// # Errors
    /// Returns and contains a callback process/protocol failure.
    pub fn on_start(&mut self) -> Result<(), ExternalRuntimeError> {
        self.call("on_start", None)
    }
    /// # Errors
    /// Returns and contains a callback process/protocol failure.
    pub fn fixed_update(&mut self, delta: f64) -> Result<(), ExternalRuntimeError> {
        self.call("fixed_update", Some(delta))
    }
    /// # Errors
    /// Returns and contains a callback process/protocol failure.
    pub fn update(&mut self, delta: f64) -> Result<(), ExternalRuntimeError> {
        self.call("update", Some(delta))
    }
    /// # Errors
    /// Returns and contains a callback process/protocol failure.
    pub fn on_destroy(&mut self) -> Result<(), ExternalRuntimeError> {
        self.call("on_destroy", None)
    }
    /// # Errors
    /// Returns and contains a callback process/protocol failure.
    pub fn on_stop(&mut self) -> Result<(), ExternalRuntimeError> {
        self.call("on_stop", None)
    }

    fn call(&mut self, callback: &str, delta: Option<f64>) -> Result<(), ExternalRuntimeError> {
        if !self.enabled {
            return Ok(());
        }
        let request = {
            let host = self.host.lock().map_err(|_| {
                ExternalRuntimeError::Runtime(
                    self.language.display_name(),
                    "gameplay host lock poisoned".into(),
                )
            })?;
            let input = host.input_frame();
            let keys = input
                .keys
                .iter()
                .chain(input.keys_pressed.iter())
                .chain(input.keys_released.iter())
                .map(|key| {
                    (
                        key.clone(),
                        serde_json::to_value(input.key(key)).unwrap_or(Value::Null),
                    )
                })
                .collect();
            let any_key_pressed = input.any_key_pressed();
            let attributes = [
                "Name",
                "Position",
                "Size",
                "Color",
                "CanTouch",
                "CanCollide",
                "Anchored",
                "Parent",
            ]
            .into_iter()
            .filter_map(|name| {
                host.attribute(name)
                    .ok()
                    .flatten()
                    .map(|value| (name.to_owned(), engine_to_json(&value)))
            })
            .collect();
            let scene_paths = host
                .scene_paths()
                .into_iter()
                .map(|(path, id)| (path, Value::String(id.to_string())))
                .collect();
            Invocation {
                format_version: 1,
                callback: callback.into(),
                delta,
                entity_id: host.entity_id().to_string(),
                delta_time: host.delta_time(),
                fixed_delta_time: host.fixed_delta_time(),
                translation: host.translation(),
                properties: host
                    .properties()
                    .into_iter()
                    .map(|(key, value)| (key, engine_to_json(&value)))
                    .collect(),
                actions: Map::new(),
                attributes,
                scene_paths,
                keys,
                key_events: input.key_events,
                any_key_pressed,
            }
        };
        let result = self
            .session
            .invoke(self.language, &request)
            .and_then(|response| {
                if response.format_version != 1 {
                    return Err(ExternalRuntimeError::Runtime(
                        self.language.display_name(),
                        format!("unsupported response version {}", response.format_version),
                    ));
                }
                let mut host = self.host.lock().map_err(|_| {
                    ExternalRuntimeError::Runtime(
                        self.language.display_name(),
                        "gameplay host lock poisoned".into(),
                    )
                })?;
                for command in response.commands {
                    apply_command(self.language, host.as_mut(), command)?;
                }
                Ok(())
            });
        if result.is_err() {
            self.enabled = false;
        }
        result
    }
}

/// HTML/CSS entry content with sandboxed inline JavaScript lifecycle callbacks.
pub struct WebBehavior {
    inner: JavaScriptBehavior,
}

impl WebBehavior {
    /// Validates the document and creates its sandboxed inline-script behavior.
    ///
    /// # Errors
    /// Returns structural HTML/CSS or JavaScript diagnostics.
    pub fn load(
        script_id: ScriptId,
        source: &[u8],
        source_name: &str,
        host: Box<dyn GameplayHost>,
        instruction_budget: u64,
    ) -> Result<Self, ExternalRuntimeError> {
        Self::load_shared(
            script_id,
            source,
            source_name,
            Arc::new(Mutex::new(host)),
            instruction_budget,
        )
    }

    /// Creates a replacement document while retaining engine-owned state.
    ///
    /// # Errors
    /// Returns structural HTML/CSS or JavaScript diagnostics.
    pub fn load_shared(
        script_id: ScriptId,
        source: &[u8],
        source_name: &str,
        host: Arc<Mutex<Box<dyn GameplayHost>>>,
        instruction_budget: u64,
    ) -> Result<Self, ExternalRuntimeError> {
        validate_web(source, source_name)?;
        let text = std::str::from_utf8(source)
            .map_err(|error| ExternalRuntimeError::InvalidUtf8("HTML/CSS", error.to_string()))?;
        let script = extract_inline_scripts(text)?;
        let script = if script.trim().is_empty() {
            "globalThis.behavior = {};".to_owned()
        } else {
            script
        };
        let inner = JavaScriptBehavior::load_shared(
            script_id,
            script.as_bytes(),
            source_name,
            host,
            instruction_budget,
        )
        .map_err(web_javascript_error)?;
        Ok(Self { inner })
    }

    pub const fn script_id(&self) -> ScriptId {
        self.inner.script_id()
    }
    pub fn host(&self) -> Arc<Mutex<Box<dyn GameplayHost>>> {
        self.inner.host()
    }
    /// # Errors
    /// Returns a contained inline-script callback failure.
    pub fn on_create(&mut self) -> Result<(), ExternalRuntimeError> {
        self.inner.on_create().map_err(web_javascript_error)
    }
    /// # Errors
    /// Returns a contained inline-script callback failure.
    pub fn on_start(&mut self) -> Result<(), ExternalRuntimeError> {
        self.inner.on_start().map_err(web_javascript_error)
    }
    /// # Errors
    /// Returns a contained inline-script callback failure.
    pub fn fixed_update(&mut self, delta: f64) -> Result<(), ExternalRuntimeError> {
        self.inner.fixed_update(delta).map_err(web_javascript_error)
    }
    /// # Errors
    /// Returns a contained inline-script callback failure.
    pub fn update(&mut self, delta: f64) -> Result<(), ExternalRuntimeError> {
        self.inner.update(delta).map_err(web_javascript_error)
    }
    /// # Errors
    /// Returns a contained inline-script callback failure.
    pub fn on_destroy(&mut self) -> Result<(), ExternalRuntimeError> {
        self.inner.on_destroy().map_err(web_javascript_error)
    }
    /// # Errors
    /// Returns a contained inline-script callback failure.
    pub fn on_stop(&mut self) -> Result<(), ExternalRuntimeError> {
        self.inner.on_stop().map_err(web_javascript_error)
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "map_err transfers the owned JavaScript error into a bounded Web diagnostic"
)]
fn web_javascript_error(error: JavaScriptRuntimeError) -> ExternalRuntimeError {
    ExternalRuntimeError::Web(error.to_string())
}

#[derive(Clone, Copy)]
struct ToolchainSpec {
    candidates: &'static [&'static str],
    version_arguments: &'static [&'static str],
    install_hint: &'static str,
}

fn toolchain_spec(language: ScriptLanguage) -> Option<ToolchainSpec> {
    match language {
        ScriptLanguage::Luau => Some(ToolchainSpec {
            candidates: &["luau"],
            version_arguments: &["--version"],
            install_hint: "the Luau CLI",
        }),
        ScriptLanguage::Python => Some(ToolchainSpec {
            candidates: &["python", "python3"],
            version_arguments: &["--version"],
            install_hint: "Python 3",
        }),
        ScriptLanguage::C => Some(ToolchainSpec {
            candidates: &["clang", "gcc", "cl"],
            version_arguments: &["--version"],
            install_hint: "Clang, GCC, or MSVC",
        }),
        ScriptLanguage::Cpp => Some(ToolchainSpec {
            candidates: &["clang++", "g++", "cl"],
            version_arguments: &["--version"],
            install_hint: "Clang, GCC, or MSVC",
        }),
        ScriptLanguage::CSharp => Some(ToolchainSpec {
            candidates: &["dotnet"],
            version_arguments: &["--version"],
            install_hint: ".NET SDK 10 or newer",
        }),
        ScriptLanguage::Java => Some(ToolchainSpec {
            candidates: &["java"],
            version_arguments: &["--version"],
            install_hint: "OpenJDK 11 or newer (java and javac)",
        }),
        ScriptLanguage::Php => Some(ToolchainSpec {
            candidates: &["php"],
            version_arguments: &["--version"],
            install_hint: "PHP CLI",
        }),
        _ => None,
    }
}

struct PreparedProgram {
    directory: TempDir,
    executable: PathBuf,
    arguments: Vec<OsString>,
    language: ScriptLanguage,
}

impl PreparedProgram {
    #[allow(
        clippy::too_many_lines,
        reason = "all build recipes remain together so the language/toolchain matrix is auditable"
    )]
    fn build(
        language: ScriptLanguage,
        source: &[u8],
        source_name: &str,
    ) -> Result<Self, ExternalRuntimeError> {
        let directory = tempfile::tempdir().map_err(|error| runtime_io(language, error))?;
        let extension = match language {
            ScriptLanguage::Luau => "luau",
            ScriptLanguage::Python => "py",
            ScriptLanguage::C => "c",
            ScriptLanguage::Cpp => "cpp",
            ScriptLanguage::CSharp => "cs",
            ScriptLanguage::Java => "java",
            ScriptLanguage::Php => "php",
            _ => Path::new(source_name)
                .extension()
                .and_then(OsStr::to_str)
                .unwrap_or("txt"),
        };
        let filename = if language == ScriptLanguage::Java {
            "RusticBehavior.java".into()
        } else {
            format!("behavior.{extension}")
        };
        let source_path = directory.path().join(filename);
        std::fs::write(&source_path, source).map_err(|error| runtime_io(language, error))?;
        if language == ScriptLanguage::Cpp {
            std::fs::write(directory.path().join("rustic.hpp"), crate::cpp_sdk::HEADER)
                .map_err(|error| runtime_io(language, error))?;
        }
        let availability = probe_language_toolchain(language);
        let executable = availability.executable.ok_or_else(|| {
            let spec = toolchain_spec(language).expect("external language has toolchain spec");
            ExternalRuntimeError::ToolchainUnavailable(language.display_name(), spec.install_hint)
        })?;
        let mut program = Self {
            directory,
            executable,
            arguments: Vec::new(),
            language,
        };
        match language {
            ScriptLanguage::Python => {
                run_checked(
                    language,
                    Command::new(&program.executable)
                        .args(["-I", "-m", "py_compile"])
                        .arg(&source_path),
                )?;
                program.arguments = vec![OsString::from("-I"), source_path.into_os_string()];
            }
            ScriptLanguage::Php => {
                run_checked(
                    language,
                    Command::new(&program.executable)
                        .arg("-l")
                        .arg(&source_path),
                )?;
                program.arguments = vec![source_path.into_os_string()];
            }
            ScriptLanguage::C | ScriptLanguage::Cpp => {
                let output = program.directory.path().join(if cfg!(windows) {
                    "behavior.exe"
                } else {
                    "behavior"
                });
                let compiler_name = program
                    .executable
                    .file_stem()
                    .and_then(OsStr::to_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let mut command = Command::new(&program.executable);
                if compiler_name == "cl" {
                    command
                        .args(["/nologo", "/W4"])
                        .arg(&source_path)
                        .arg(format!("/Fe:{}", output.display()));
                } else {
                    command.args(["-Wall", "-Wextra", "-Werror"]);
                    command.arg(if language == ScriptLanguage::Cpp {
                        "-std=c++20"
                    } else {
                        "-std=c17"
                    });
                    command.arg(&source_path).arg("-o").arg(&output);
                }
                run_checked(language, &mut command)?;
                program.executable = output;
            }
            ScriptLanguage::CSharp => {
                let version = availability.version.unwrap_or_else(|| "10.0".into());
                let major = version.split('.').next().unwrap_or("10");
                let project = program.directory.path().join("RusticBehavior.csproj");
                std::fs::write(
                    &project,
                    format!("<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net{major}.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup></Project>"),
                ).map_err(|error| runtime_io(language, error))?;
                run_checked(
                    language,
                    Command::new(&program.executable)
                        .args(["build", "-c", "Release", "--nologo"])
                        .arg(&project),
                )?;
                let dll = program
                    .directory
                    .path()
                    .join(format!("bin/Release/net{major}.0/RusticBehavior.dll"));
                program.arguments = vec![dll.into_os_string()];
            }
            ScriptLanguage::Java => {
                let javac = find_executable(&["javac"]).ok_or(
                    ExternalRuntimeError::ToolchainUnavailable("Java", "OpenJDK javac"),
                )?;
                let classes = program.directory.path().join("classes");
                std::fs::create_dir(&classes).map_err(|error| runtime_io(language, error))?;
                run_checked(
                    language,
                    Command::new(javac)
                        .arg("-d")
                        .arg(&classes)
                        .arg(&source_path),
                )?;
                program.arguments = vec![
                    OsString::from("-cp"),
                    classes.into_os_string(),
                    OsString::from("RusticBehavior"),
                ];
            }
            ScriptLanguage::Luau => {
                validate_with_stdin(language, source, &["--compile=-", "-"])?;
                program.arguments = vec![source_path.into_os_string()];
            }
            _ => {
                return Err(ExternalRuntimeError::Runtime(
                    language.display_name(),
                    "language is not an external process adapter".into(),
                ));
            }
        }
        Ok(program)
    }
}

struct ProcessSession {
    child: Child,
    stdin: ChildStdin,
    responses: Receiver<Result<Vec<u8>, String>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stdout_worker: Option<JoinHandle<()>>,
    stderr_worker: Option<JoinHandle<()>>,
}

impl ProcessSession {
    fn start(program: &PreparedProgram) -> Result<Self, ExternalRuntimeError> {
        let mut command = Command::new(&program.executable);
        command
            .args(&program.arguments)
            .current_dir(program.directory.path())
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        copy_safe_environment(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| runtime_io(program.language, error))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            ExternalRuntimeError::Runtime(
                program.language.display_name(),
                "child stdin unavailable".into(),
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ExternalRuntimeError::Runtime(
                program.language.display_name(),
                "child stdout unavailable".into(),
            )
        })?;
        let child_stderr = child.stderr.take().ok_or_else(|| {
            ExternalRuntimeError::Runtime(
                program.language.display_name(),
                "child stderr unavailable".into(),
            )
        })?;
        let (sender, responses) = mpsc::sync_channel(8);
        let stdout_worker = std::thread::Builder::new()
            .name(format!("rustic-{}-stdout", program.language.display_name()))
            .spawn(move || read_response_lines(stdout, sender))
            .map_err(|error| runtime_io(program.language, error))?;
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stderr_output = Arc::clone(&stderr);
        let stderr_worker = std::thread::Builder::new()
            .name(format!("rustic-{}-stderr", program.language.display_name()))
            .spawn(move || {
                let mut reader = child_stderr.take(MAX_DIAGNOSTIC_BYTES as u64);
                let mut bytes = Vec::new();
                let _ = reader.read_to_end(&mut bytes);
                if let Ok(mut output) = stderr_output.lock() {
                    *output = bytes;
                }
            })
            .map_err(|error| runtime_io(program.language, error))?;
        Ok(Self {
            child,
            stdin,
            responses,
            stderr,
            stdout_worker: Some(stdout_worker),
            stderr_worker: Some(stderr_worker),
        })
    }

    fn invoke(
        &mut self,
        language: ScriptLanguage,
        request: &Invocation,
    ) -> Result<InvocationResponse, ExternalRuntimeError> {
        let bytes = serde_json::to_vec(request).map_err(|error| {
            ExternalRuntimeError::Runtime(language.display_name(), error.to_string())
        })?;
        self.stdin
            .write_all(&bytes)
            .and_then(|()| self.stdin.write_all(b"\n"))
            .and_then(|()| self.stdin.flush())
            .map_err(|error| runtime_io(language, error))?;
        let stdout_bytes = match self.responses.recv_timeout(CALLBACK_TIMEOUT) {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(message)) => {
                return Err(ExternalRuntimeError::Runtime(
                    language.display_name(),
                    message,
                ));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return Err(ExternalRuntimeError::Timeout(
                    language.display_name(),
                    CALLBACK_TIMEOUT,
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let detail = self
                    .stderr
                    .lock()
                    .map_or_else(|_| "host exited".into(), |bytes| diagnostic_text(&bytes));
                return Err(ExternalRuntimeError::Runtime(
                    language.display_name(),
                    detail,
                ));
            }
        };
        serde_json::from_slice(&stdout_bytes).map_err(|error| {
            ExternalRuntimeError::Runtime(
                language.display_name(),
                format!("invalid response JSON: {error}"),
            )
        })
    }
}

impl Drop for ProcessSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(worker) = self.stdout_worker.take() {
            let _ = worker.join();
        }
        if let Some(worker) = self.stderr_worker.take() {
            let _ = worker.join();
        }
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "the worker owns the sender so channel disconnection signals process exit"
)]
fn read_response_lines(
    stdout: std::process::ChildStdout,
    sender: SyncSender<Result<Vec<u8>, String>>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut bytes = Vec::new();
        let result = reader
            .by_ref()
            .take(MAX_RESPONSE_BYTES + 1)
            .read_until(b'\n', &mut bytes);
        match result {
            Ok(0) => break,
            Ok(_) if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_RESPONSE_BYTES => {
                let _ = sender.send(Err(format!("response exceeds {MAX_RESPONSE_BYTES} bytes")));
                break;
            }
            Ok(_) => {
                while bytes
                    .last()
                    .is_some_and(|byte| matches!(byte, b'\n' | b'\r'))
                {
                    bytes.pop();
                }
                if sender.send(Ok(bytes)).is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(error.to_string()));
                break;
            }
        }
    }
}

#[derive(Serialize)]
struct Invocation {
    format_version: u32,
    callback: String,
    delta: Option<f64>,
    entity_id: String,
    delta_time: f64,
    fixed_delta_time: f64,
    translation: [f64; 3],
    properties: Map<String, Value>,
    actions: Map<String, Value>,
    attributes: Map<String, Value>,
    scene_paths: Map<String, Value>,
    keys: Map<String, Value>,
    key_events: Vec<crate::KeyEvent>,
    any_key_pressed: bool,
}

#[derive(Deserialize)]
struct InvocationResponse {
    format_version: u32,
    #[serde(default)]
    commands: Vec<ExternalCommand>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ExternalCommand {
    SetTranslation {
        value: [f64; 3],
    },
    SetProperty {
        name: String,
        value: Value,
    },
    EditAttribute {
        name: String,
        value: Value,
    },
    Log {
        level: String,
        message: String,
    },
    SetEnabled {
        enabled: bool,
    },
    AddInstance {
        source: String,
        parent: Option<String>,
    },
    CloneInstance {
        source: String,
        parent: Option<String>,
    },
}

fn apply_command(
    language: ScriptLanguage,
    host: &mut dyn GameplayHost,
    command: ExternalCommand,
) -> Result<(), ExternalRuntimeError> {
    let result = match command {
        ExternalCommand::SetTranslation { value } => host.set_translation(value),
        ExternalCommand::SetProperty { name, value } => host.property(&name).map_or_else(
            || Err(format!("property `{name}` is not declared")),
            |current| {
                json_to_engine(value, &current)
                    .and_then(|converted| host.set_property(&name, converted))
            },
        ),
        ExternalCommand::EditAttribute { name, value } => {
            host.attribute(&name).and_then(|current| {
                current.map_or_else(
                    || Err(format!("attribute `{name}` is unavailable")),
                    |current| {
                        json_to_engine(value, &current)
                            .and_then(|converted| host.edit_attribute(&name, converted))
                    },
                )
            })
        }
        ExternalCommand::Log { level, message } => host.log(&level, &message),
        ExternalCommand::SetEnabled { enabled } => {
            host.set_enabled(enabled);
            Ok(())
        }
        ExternalCommand::AddInstance { source, parent } => parse_parent(parent)
            .and_then(|parent| host.add_instance(&source, parent))
            .map(|_| ()),
        ExternalCommand::CloneInstance { source, parent } => parse_parent(parent)
            .and_then(|parent| host.clone_instance(&source, parent))
            .map(|_| ()),
    };
    result.map_err(|message| ExternalRuntimeError::Runtime(language.display_name(), message))
}

fn parse_parent(parent: Option<String>) -> Result<Option<crate::EntityId>, String> {
    parent
        .map(|value| {
            value
                .parse::<crate::EntityId>()
                .map_err(|_| format!("invalid parent entity id `{value}`"))
        })
        .transpose()
}

fn validate_source_bytes(
    language: ScriptLanguage,
    source: &[u8],
    maximum_bytes: usize,
) -> Result<(), ExternalRuntimeError> {
    if source.len() > maximum_bytes {
        return Err(ExternalRuntimeError::SourceTooLarge(
            language.display_name(),
            maximum_bytes,
        ));
    }
    std::str::from_utf8(source).map(|_| ()).map_err(|error| {
        ExternalRuntimeError::InvalidUtf8(language.display_name(), error.to_string())
    })
}

fn validate_web(source: &[u8], source_name: &str) -> Result<(), ExternalRuntimeError> {
    let text = std::str::from_utf8(source)
        .map_err(|error| ExternalRuntimeError::InvalidUtf8("HTML/CSS", error.to_string()))?;
    let lower = text.to_ascii_lowercase();
    let css_only = std::path::Path::new(source_name)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("css"));
    if !css_only && !lower.trim_start().starts_with("<!doctype html>") {
        return Err(ExternalRuntimeError::Web(
            "visible UI entries must start with <!doctype html>".into(),
        ));
    }
    if !css_only
        && (!lower.contains("<html")
            || !lower.contains("</html>")
            || !lower.contains("<body")
            || !lower.contains("</body>"))
    {
        return Err(ExternalRuntimeError::Web(
            "entry document must contain matching <html> and <body> tags".into(),
        ));
    }
    let scripts = extract_inline_scripts(text)?;
    if !scripts.trim().is_empty() {
        validate_javascript(scripts.as_bytes(), "inline-script.js", 1024 * 1024)
            .map_err(|error| ExternalRuntimeError::Web(error.to_string()))?;
    }
    let opens = text.chars().filter(|character| *character == '{').count();
    let closes = text.chars().filter(|character| *character == '}').count();
    if opens != closes {
        return Err(ExternalRuntimeError::Web(
            "CSS declaration braces are unbalanced".into(),
        ));
    }
    Ok(())
}

pub(crate) fn extract_inline_scripts(text: &str) -> Result<String, ExternalRuntimeError> {
    let mut remaining = text;
    let mut output = String::new();
    loop {
        let lower = remaining.to_ascii_lowercase();
        let Some(open) = lower.find("<script") else {
            break;
        };
        let after_open = lower[open..]
            .find('>')
            .ok_or_else(|| ExternalRuntimeError::Web("unterminated <script> tag".into()))?
            + open
            + 1;
        let tail_lower = remaining[after_open..].to_ascii_lowercase();
        let close = tail_lower
            .find("</script>")
            .ok_or_else(|| ExternalRuntimeError::Web("missing </script> tag".into()))?;
        output.push_str(&remaining[after_open..after_open + close]);
        output.push('\n');
        remaining = &remaining[after_open + close + "</script>".len()..];
    }
    Ok(output)
}

fn validate_with_stdin(
    language: ScriptLanguage,
    source: &[u8],
    arguments: &[&str],
) -> Result<(), ExternalRuntimeError> {
    let availability = probe_language_toolchain(language);
    let executable = availability.executable.ok_or_else(|| {
        let spec = toolchain_spec(language).expect("external language has toolchain spec");
        ExternalRuntimeError::ToolchainUnavailable(language.display_name(), spec.install_hint)
    })?;
    let mut child = Command::new(executable)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| runtime_io(language, error))?;
    child
        .stdin
        .take()
        .ok_or_else(|| {
            ExternalRuntimeError::Build(
                language.display_name(),
                "compiler stdin unavailable".into(),
            )
        })?
        .write_all(source)
        .map_err(|error| runtime_io(language, error))?;
    let output = child
        .wait_with_output()
        .map_err(|error| runtime_io(language, error))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ExternalRuntimeError::Build(
            language.display_name(),
            diagnostic_text(&output.stderr),
        ))
    }
}

fn run_checked(
    language: ScriptLanguage,
    command: &mut Command,
) -> Result<(), ExternalRuntimeError> {
    let output = command
        .output()
        .map_err(|error| runtime_io(language, error))?;
    if output.status.success() {
        Ok(())
    } else {
        let bytes = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        Err(ExternalRuntimeError::Build(
            language.display_name(),
            diagnostic_text(bytes),
        ))
    }
}

fn find_executable(candidates: &[&str]) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    let extensions: Vec<OsString> = if cfg!(windows) {
        std::env::var_os("PATHEXT")
            .unwrap_or_else(|| ".EXE;.CMD;.BAT;.COM".into())
            .to_string_lossy()
            .split(';')
            .map(OsString::from)
            .collect()
    } else {
        vec![OsString::new()]
    };
    for directory in std::env::split_paths(&paths) {
        for candidate in candidates {
            let direct = directory.join(candidate);
            if direct.is_file() {
                return Some(direct);
            }
            for extension in &extensions {
                let path = directory.join(format!("{candidate}{}", extension.to_string_lossy()));
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn copy_safe_environment(command: &mut Command) {
    for name in [
        "PATH",
        "PATHEXT",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "DOTNET_ROOT",
        "JAVA_HOME",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

fn diagnostic_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_DIAGNOSTIC_BYTES)])
        .trim()
        .to_owned()
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "map_err transfers owned I/O errors into language-scoped diagnostics"
)]
fn runtime_io(language: ScriptLanguage, error: std::io::Error) -> ExternalRuntimeError {
    ExternalRuntimeError::Runtime(language.display_name(), error.to_string())
}

fn engine_to_json(value: &EngineValue) -> Value {
    match value {
        EngineValue::Boolean(value) => Value::Bool(*value),
        EngineValue::Integer(value) => Value::Number((*value).into()),
        EngineValue::Number(value) => json!(value),
        EngineValue::String(value) => Value::String(value.clone()),
        EngineValue::Vec2(value) => json!(value),
        EngineValue::Vec3(value) => json!(value),
        EngineValue::Entity(value) => value.map_or(Value::Null, |id| Value::String(id.to_string())),
    }
}

fn json_to_engine(value: Value, current: &EngineValue) -> Result<EngineValue, String> {
    match current {
        EngineValue::Boolean(_) => value.as_bool().map(EngineValue::Boolean),
        EngineValue::Integer(_) => value.as_i64().map(EngineValue::Integer),
        EngineValue::Number(_) => value.as_f64().map(EngineValue::Number),
        EngineValue::String(_) => value.as_str().map(|text| EngineValue::String(text.into())),
        EngineValue::Vec2(_) => serde_json::from_value(value).ok().map(EngineValue::Vec2),
        EngineValue::Vec3(_) => serde_json::from_value(value).ok().map(EngineValue::Vec3),
        EngineValue::Entity(_) if value.is_null() => Some(EngineValue::Entity(None)),
        EngineValue::Entity(_) => value
            .as_str()
            .and_then(|text| text.parse().ok())
            .map(|id| EngineValue::Entity(Some(id))),
    }
    .ok_or_else(|| "public property type mismatch".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ActionState;
    use engine_core::EntityId;
    use std::collections::BTreeMap;

    struct Host {
        entity: EntityId,
        translation: [f64; 3],
    }

    impl GameplayHost for Host {
        fn entity_id(&self) -> EntityId {
            self.entity
        }
        fn delta_time(&self) -> f64 {
            1.0 / 60.0
        }
        fn fixed_delta_time(&self) -> f64 {
            1.0 / 60.0
        }
        fn translation(&self) -> [f64; 3] {
            self.translation
        }
        fn set_translation(&mut self, value: [f64; 3]) -> Result<(), String> {
            self.translation = value;
            Ok(())
        }
        fn find_entity(&self, _query: &str) -> Result<Option<EntityId>, String> {
            Ok(None)
        }
        fn add_instance(
            &mut self,
            _source: &str,
            _parent: Option<EntityId>,
        ) -> Result<EntityId, String> {
            Ok(EntityId::new())
        }
        fn clone_instance(
            &mut self,
            _source: &str,
            _parent: Option<EntityId>,
        ) -> Result<EntityId, String> {
            Ok(EntityId::new())
        }
        fn input_action(&self, _name: &str) -> ActionState {
            ActionState::default()
        }
        fn log(&mut self, _level: &str, _message: &str) -> Result<(), String> {
            Ok(())
        }
        fn property(&self, _name: &str) -> Option<EngineValue> {
            None
        }
        fn properties(&self) -> BTreeMap<String, EngineValue> {
            BTreeMap::new()
        }
        fn set_property(&mut self, _name: &str, _value: EngineValue) -> Result<(), String> {
            Err("not declared".into())
        }
        fn set_enabled(&mut self, _enabled: bool) {}
    }

    #[test]
    fn external_protocol_accepts_instance_commands() {
        let response: InvocationResponse = serde_json::from_str(
            r#"{"format_version":1,"commands":[{"op":"add_instance","source":"assets/models/chair.obj","parent":null},{"op":"clone_instance","source":"Game.scene.Chair","parent":null}]}"#,
        ).unwrap();
        let mut host = Host {
            entity: EntityId::new(),
            translation: [0.0; 3],
        };
        for command in response.commands {
            apply_command(ScriptLanguage::Python, &mut host, command).unwrap();
        }
    }

    #[test]
    fn web_validation_compiles_inline_javascript_and_rejects_structure_errors() {
        validate_web(b"<!doctype html><html><body><style>body { color: red; }</style><script>globalThis.behavior={update(){}};</script></body></html>", "hud.html").unwrap();
        assert!(validate_web(b"<html><body></body></html>", "hud.html").is_err());
        assert!(
            validate_web(
                b"<!doctype html><html><body><style>body {</style></body></html>",
                "hud.html"
            )
            .is_err()
        );
        assert!(
            validate_web(
                b"<!doctype html><html><body><script>function (</script></body></html>",
                "hud.html"
            )
            .is_err()
        );
        validate_web(b"body { color: red; }", "theme.css").unwrap();
    }

    #[test]
    fn every_external_language_has_a_probe_contract() {
        for language in [
            ScriptLanguage::Luau,
            ScriptLanguage::Python,
            ScriptLanguage::C,
            ScriptLanguage::Cpp,
            ScriptLanguage::CSharp,
            ScriptLanguage::Java,
            ScriptLanguage::Php,
        ] {
            let probe = probe_language_toolchain(language);
            assert_eq!(probe.language, language);
            assert!(!probe.detail.is_empty());
        }
    }

    #[test]
    fn python_adapter_executes_the_versioned_lifecycle_protocol_when_available() {
        if !probe_language_toolchain(ScriptLanguage::Python).available {
            return;
        }
        let source = br#"import json, sys
for line in sys.stdin:
    request = json.loads(line)
    commands = []
    if request["callback"] == "fixed_update":
        value = request["translation"]
        value[0] += 3
        commands.append({"op":"set_translation", "value":value})
    print(json.dumps({"format_version":1, "commands":commands}), flush=True)
"#;
        let mut behavior = ExternalBehavior::load(
            ScriptId::new(),
            ScriptLanguage::Python,
            source,
            "behavior.py",
            Box::new(Host {
                entity: EntityId::new(),
                translation: [0.0; 3],
            }),
        )
        .unwrap();
        behavior.on_create().unwrap();
        behavior.on_start().unwrap();
        behavior.fixed_update(1.0 / 60.0).unwrap();
        let translation = behavior.host().lock().unwrap().translation();
        assert!((translation[0] - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn every_available_process_adapter_builds_and_executes() {
        for language in [
            ScriptLanguage::Python,
            ScriptLanguage::CSharp,
            ScriptLanguage::C,
            ScriptLanguage::Cpp,
            ScriptLanguage::Java,
            ScriptLanguage::Php,
        ] {
            if !probe_language_toolchain(language).available {
                continue;
            }
            let (name, source) = protocol_fixture(language);
            let mut behavior = ExternalBehavior::load(
                ScriptId::new(),
                language,
                source,
                name,
                Box::new(Host {
                    entity: EntityId::new(),
                    translation: [0.0; 3],
                }),
            )
            .unwrap_or_else(|error| panic!("{} fixture failed: {error}", language.display_name()));
            behavior.on_create().unwrap_or_else(|error| {
                panic!(
                    "{} first invocation failed: {error}",
                    language.display_name()
                )
            });
            behavior.on_start().unwrap_or_else(|error| {
                panic!("{} invocation failed: {error}", language.display_name())
            });
        }
    }

    #[test]
    fn web_behavior_executes_inline_lifecycle() {
        let source = br"<!doctype html><html><body><script>globalThis.behavior={fixed_update(){const [x,y,z]=rustic.get_translation();rustic.set_translation(x+5,y,z);}};</script></body></html>";
        let mut behavior = WebBehavior::load(
            ScriptId::new(),
            source,
            "behavior.html",
            Box::new(Host {
                entity: EntityId::new(),
                translation: [0.0; 3],
            }),
            10_000,
        )
        .unwrap();
        behavior.fixed_update(1.0 / 60.0).unwrap();
        let translation = behavior.host().lock().unwrap().translation();
        assert!((translation[0] - 5.0).abs() < f64::EPSILON);
    }

    fn protocol_fixture(language: ScriptLanguage) -> (&'static str, &'static [u8]) {
        match language {
            ScriptLanguage::Python => ("behavior.py", b"import json,sys\nfor line in sys.stdin:\n print(json.dumps({'format_version':1,'commands':[]}),flush=True)\n"),
            ScriptLanguage::CSharp => ("behavior.cs", b"using System; string? line; while ((line = Console.ReadLine()) is not null) Console.WriteLine(\"{\\\"format_version\\\":1,\\\"commands\\\":[]}\");"),
            ScriptLanguage::C => ("behavior.c", b"#include <stdio.h>\nint main(void){char line[1048577];while(fgets(line,sizeof line,stdin)){puts(\"{\\\"format_version\\\":1,\\\"commands\\\":[]}\");fflush(stdout);}return 0;}\n"),
            ScriptLanguage::Cpp => ("behavior.cpp", br#"#include "rustic.hpp"
void start(){rustic.log("info", "C++ SDK started");}
void fixed(double dt){auto p=rustic.get_translation();rustic.set_translation(p.x+dt,p.y,p.z);auto key=rustic.key("KeyW");(void)key;auto found=Game.scene.Find("Room.Table");(void)found;rustic.EditAttribute("Position",RusticValue::Array{1.0,2.0,3.0});}
int main(){return rustic_run(RusticBehavior{.on_start=start,.fixed_update=fixed});}
"#),
            ScriptLanguage::Java => ("RusticBehavior.java", b"import java.io.*;class RusticBehavior{public static void main(String[]a)throws Exception{var r=new BufferedReader(new InputStreamReader(System.in));while(r.readLine()!=null){System.out.println(\"{\\\"format_version\\\":1,\\\"commands\\\":[]}\");System.out.flush();}}}\n"),
            ScriptLanguage::Php => ("behavior.php", b"<?php while (($line=fgets(STDIN))!==false){json_decode($line,true,flags:JSON_THROW_ON_ERROR);echo json_encode(['format_version'=>1,'commands'=>[]],JSON_THROW_ON_ERROR),PHP_EOL;flush();}\n"),
            _ => unreachable!("fixture requested only for process adapters"),
        }
    }
}
