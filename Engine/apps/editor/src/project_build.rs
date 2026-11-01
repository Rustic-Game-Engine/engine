//! Project distribution UI and shell-free packaging jobs.
mod portable;
use eframe::egui;
use engine_editor::AuthoringDocument;
use engine_platform::AtomicFileService;
use engine_play::PlayMode;
use engine_project::VirtualDirectory;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Target {
    #[default]
    Windows,
    Website,
    Apk,
    Aab,
}
impl Target {
    fn label(self) -> &'static str {
        match self {
            Self::Windows => "Windows game (.exe)",
            Self::Website => "Website (playable game)",
            Self::Apk => "Android APK (debug)",
            Self::Aab => "Android App Bundle (release)",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    target: Target,
    name: String,
    version: String,
    publisher: String,
    installer: bool,
    welcome: String,
    license: String,
    icon: String,
    banner: String,
    desktop_shortcut: bool,
    modern: bool,
    android_package: String,
    android_version_code: u32,
    compiler: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            target: Target::Windows,
            name: "Game".into(),
            version: "1.0.0".into(),
            publisher: String::new(),
            installer: true,
            welcome: "Welcome to the setup wizard.".into(),
            license: String::new(),
            icon: String::new(),
            banner: String::new(),
            desktop_shortcut: true,
            modern: true,
            android_package: "org.rustic.game".into(),
            android_version_code: 1,
            compiler: "ISCC.exe".into(),
        }
    }
}

#[derive(Default)]
pub struct ProjectBuild {
    pub open: bool,
    settings: Settings,
    loaded: bool,
    status: String,
    job: Option<Receiver<Result<PathBuf, String>>>,
    output: Option<PathBuf>,
}

impl ProjectBuild {
    #[allow(
        clippy::too_many_lines,
        reason = "keep native build dialog controls and their state changes together"
    )]
    pub fn show(&mut self, context: &egui::Context, document: &AuthoringDocument) {
        if let Some(job) = &self.job {
            match job.try_recv() {
                Ok(result) => {
                    self.job = None;
                    match result {
                        Ok(path) => {
                            self.status = format!("Build complete: {}", path.display());
                            self.output = Some(path);
                        }
                        Err(error) => self.status = format!("Build failed: {error}"),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.job = None;
                    self.status = "Build worker stopped unexpectedly.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    context.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        if !self.open {
            return;
        }
        let root = document.project().root();
        if !self.loaded {
            self.loaded = true;
            self.settings
                .name
                .clone_from(&document.project().metadata().name);
            let path = root.join("config/build.json");
            if path.exists() {
                match fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
                {
                    Ok(settings) => self.settings = settings,
                    Err(error) => self.status = format!("Could not load build settings: {error}"),
                }
            }
        }
        let mut open = self.open;
        egui::Window::new("Build Project")
            .open(&mut open)
            .default_width(620.0)
            .vscroll(true)
            .show(context, |ui| {
                ui.add_enabled_ui(self.job.is_none(), |ui| {
                    egui::ComboBox::from_label("Target")
                        .selected_text(self.settings.target.label())
                        .show_ui(ui, |ui| {
                            for target in [Target::Windows, Target::Website, Target::Apk, Target::Aab] {
                                ui.selectable_value(&mut self.settings.target, target, target.label());
                            }
                        });
                    field(ui, "Application name", &mut self.settings.name);
                    match self.settings.target {
                        Target::Windows => {
                            ui.label("Packages the bundled Windows runtime and a snapshot of the current scene, scripts and assets. Distribute the entire output folder.");
                            ui.checkbox(&mut self.settings.installer, "Create a custom installer");
                            if self.settings.installer {
                                ui.collapsing("Design installer", |ui| {
                                    field(ui, "Version", &mut self.settings.version);
                                    field(ui, "Publisher", &mut self.settings.publisher);
                                    field(ui, "Welcome message", &mut self.settings.welcome);
                                    field(ui, "License file (project relative, optional)", &mut self.settings.license);
                                    field(ui, "Setup icon (.ico, project relative, optional)", &mut self.settings.icon);
                                    field(ui, "Wizard image (.bmp, project relative, optional)", &mut self.settings.banner);
                                    ui.checkbox(&mut self.settings.modern, "Modern wizard style");
                                    ui.checkbox(&mut self.settings.desktop_shortcut, "Offer desktop shortcut");
                                    field(ui, "Inno Setup compiler (ISCC.exe path)", &mut self.settings.compiler);
                                    ui.small("Leave ISCC.exe for automatic setup. Building finds or installs Inno Setup and adds it to your user PATH. First installation requires internet access.");
                                    ui.group(|ui| {
                                        ui.strong(format!("{} Setup", self.settings.name));
                                        ui.label(&self.settings.welcome);
                                        ui.label(format!("Version {} · {}", self.settings.version, self.settings.publisher));
                                        ui.label("Installation folder: your local Programs folder");
                                        ui.horizontal(|ui| {
                                            ui.add_enabled(false, egui::Button::new("Back"));
                                            ui.add_enabled(false, egui::Button::new("Next"));
                                            ui.add_enabled(false, egui::Button::new("Cancel"));
                                        });
                                    });
                                    ui.small("Text preview. Inno Setup renders your icon and wizard image in the compiled installer. Installation is per user and includes an uninstaller.");
                                });
                            }
                        }
                        Target::Website => {
                            ui.label("Turns the current game into a playable website: scenes, scripts, assets and the WebAssembly runtime are packaged automatically. Upload the generated website folder to any static host. No web source folder or game server is needed.");
                        }
                        Target::Apk | Target::Aab => {
                            field(ui, "Android package ID", &mut self.settings.android_package);
                            ui.horizontal(|ui| { ui.label("Version code"); ui.add(egui::DragValue::new(&mut self.settings.android_version_code).range(1..=2_100_000_000_u32)); });
                            field(ui, "Version name", &mut self.settings.version);
                            ui.label("Generates an Android app containing the compiled game. It runs locally through Android System WebView with touch/keyboard input, without an Android template or game server.");
                            ui.label("Requires JDK 17+, Android SDK platform 36/build-tools 36.0.0 and the bundled browser runtime. Gradle downloads build dependencies when first used. APK is debug-signed. Release AAB signing uses RUSTIC_ANDROID_KEYSTORE, RUSTIC_ANDROID_STORE_PASSWORD, RUSTIC_ANDROID_KEY_ALIAS and RUSTIC_ANDROID_KEY_PASSWORD.");
                        }
                    }
                    ui.separator();
                    let requirements = readiness(root, &self.settings);
                    for message in &requirements {
                        ui.label(format!("Required: {message}"));
                    }
                    if requirements.is_empty() {
                        ui.label("Required resources found. Build tools will validate content and configuration.");
                    }
                    ui.label("Outputs go into a new folder under the project's builds directory. The active scene's current edits are included in game builds.");
                    if ui.add_enabled(requirements.is_empty() && !document.is_read_only(), egui::Button::new("Build Project")).clicked()
                        && let Err(error) = self.start(document, context.clone()) {
                        self.status = format!("Build failed: {error}");
                    }
                    if document.is_read_only() {
                        ui.label("Builds are unavailable while this project is open read-only.");
                    }
                });
                if self.job.is_some() {
                    ui.horizontal(|ui| { ui.spinner(); ui.label("Building project…"); });
                }
                ui.label(&self.status);
                if let Some(output) = &self.output {
                    ui.label("Output folder");
                    let mut path = output.display().to_string();
                    ui.add(egui::TextEdit::singleline(&mut path).interactive(false));
                }
            });
        self.open = open;
    }

    fn start(
        &mut self,
        document: &AuthoringDocument,
        context: egui::Context,
    ) -> Result<(), String> {
        validate_settings(&self.settings)?;
        let root = document.project().root().to_path_buf();
        let bytes = serde_json::to_vec_pretty(&self.settings).map_err(|e| e.to_string())?;
        AtomicFileService::new()
            .write(&root.join("config/build.json"), &bytes)
            .map_err(|e| e.to_string())?;
        let builds = document
            .project()
            .directory(VirtualDirectory::Builds)
            .map_err(|e| e.to_string())?;
        if self.settings.target != Target::Windows {
            portable::validate_scripts(&root)?;
        }
        let staging = tempfile::Builder::new()
            .prefix("project-")
            .tempdir_in(builds)
            .map_err(|e| e.to_string())?;
        {
            let snapshot = crate::play_session::stage_snapshot(
                document,
                PlayMode::Standalone,
                &staging.path().join("snapshot"),
            )?;
            let copied = copy_tree(snapshot.root(), &staging.path().join("data"), &[]);
            let cleanup = snapshot.remove().map_err(|e| e.to_string());
            copied?;
            cleanup?;
            fs::remove_dir(staging.path().join("snapshot")).map_err(|e| e.to_string())?;
        }
        let settings = self.settings.clone();
        let runtime = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("rustic-runtime.exe");
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("project-build".into())
            .spawn(move || {
                let result =
                    build(&root, staging.path(), &settings, &runtime).map(|()| staging.keep());
                let _ = sender.send(result);
                context.request_repaint();
            })
            .map_err(|e| e.to_string())?;
        self.job = Some(receiver);
        self.output = None;
        self.status = "Building project…".into();
        Ok(())
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
}

fn project_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Choose a path inside the project without parent components.".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let joined = root
        .join(path)
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !joined.starts_with(root) {
        return Err("Resource path escapes the project.".into());
    }
    Ok(joined)
}

fn find_program(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    if path.components().count() != 1 {
        return None;
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(program))
            .find(|path| path.is_file())
    })
}

fn automatic_compiler(compiler: &str) -> bool {
    compiler.trim().is_empty() || compiler.eq_ignore_ascii_case("ISCC.exe")
}

#[cfg(windows)]
fn ensure_installer_compiler(compiler: &str) -> Result<PathBuf, String> {
    use std::os::windows::process::CommandExt as _;

    let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
    let script = directory.path().join("ensure-inno-setup.ps1");
    fs::write(&script, include_str!("project_build/ensure-inno-setup.ps1"))
        .map_err(|e| e.to_string())?;
    // Pass user input as data, never interpolate it into PowerShell source.
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .env("RUSTIC_INNO_COMPILER", compiler)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map_err(|e| format!("Could not start automatic Inno Setup configuration: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Inno Setup configuration failed: {}. Check internet access and user PATH permissions, then retry. You can also install Inno Setup 6 manually and set its full ISCC.exe path.",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    if !path.is_absolute() || !path.is_file() {
        return Err("Inno Setup configuration did not return an existing compiler.".into());
    }
    // Use the resolved absolute path immediately; the running editor retains its
    // inherited environment even after the script updates the persistent PATH.
    Ok(path)
}

#[cfg(not(windows))]
fn ensure_installer_compiler(_compiler: &str) -> Result<PathBuf, String> {
    Err("Inno Setup requires a Windows host.".into())
}

fn java_executable() -> Option<PathBuf> {
    let executable = if cfg!(windows) { "java.exe" } else { "java" };
    std::env::var_os("JAVA_HOME")
        .map(|home| PathBuf::from(home).join("bin").join(executable))
        .filter(|path| path.is_file())
        .or_else(|| find_program(executable))
}

fn android_sdk() -> Option<PathBuf> {
    let mut paths: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        paths.push(PathBuf::from(local).join("Android/Sdk"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(&home).join("Android/Sdk"));
        paths.push(PathBuf::from(home).join("Library/Android/sdk"));
    }
    paths.into_iter().find(|path| path.is_dir())
}

fn available(program: &str, argument: &str) -> bool {
    Command::new(program)
        .arg(argument)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn readiness(root: &Path, settings: &Settings) -> Vec<String> {
    let mut missing = Vec::new();
    match settings.target {
        Target::Windows => {
            if !cfg!(windows) {
                missing.push("Windows host for the Windows runtime and Inno Setup.".into());
            }
            if !std::env::current_exe()
                .is_ok_and(|p| p.with_file_name("rustic-runtime.exe").is_file())
            {
                missing.push("rustic-runtime.exe beside the editor.".into());
            }
            if settings.installer
                && !automatic_compiler(&settings.compiler)
                && find_program(&settings.compiler).is_none()
            {
                missing.push("The custom Inno Setup compiler path does not exist. Use ISCC.exe for automatic setup.".into());
            }
        }
        Target::Website => {}
        Target::Apk | Target::Aab => {
            if java_executable().is_none() {
                missing.push("JAVA_HOME pointing to a JDK with bin/java.".into());
            }
            match android_sdk() {
                None => missing
                    .push("Android SDK: install it or set ANDROID_HOME/ANDROID_SDK_ROOT.".into()),
                Some(sdk) => {
                    if !sdk.join("platforms/android-36/android.jar").is_file() {
                        missing.push("Android SDK platform 36.".into());
                    }
                    if !sdk.join("build-tools/36.0.0").is_dir() {
                        missing.push("Android SDK build-tools 36.0.0.".into());
                    }
                }
            }
        }
    }
    if settings.target != Target::Windows {
        if !std::env::current_exe()
            .is_ok_and(|exe| portable::runtime_available(&portable::runtime_directory(&exe)))
        {
            missing.push("Compiled browser runtime: install an engine distribution with browser-runtime/ or run tools/build-browser-runtime.py from engine source.".into());
        }
        if let Err(error) = portable::validate_scripts(root) {
            missing.push(error);
        }
    }
    missing
}

fn validate_settings(settings: &Settings) -> Result<(), String> {
    // These values appear in Inno directives/messages and the Windows executable name.
    for (label, value) in [
        ("name", &settings.name),
        ("version", &settings.version),
        ("publisher", &settings.publisher),
        ("welcome message", &settings.welcome),
    ] {
        if value
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '{' | '}'))
        {
            return Err(format!(
                "The {label} cannot contain control characters, quotes or braces."
            ));
        }
    }
    if matches!(settings.target, Target::Apk | Target::Aab)
        && (!portable::valid_package(&settings.android_package)
            || !(1..=2_100_000_000).contains(&settings.android_version_code))
    {
        return Err(
            "Use an Android package ID such as com.studio.mygame and a positive version code."
                .into(),
        );
    }
    let name = settings.name.trim();
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if name.eq_ignore_ascii_case("rustic-luau-host")
        || name.is_empty()
        || name.ends_with(['.', ' '])
        || name.chars().any(|c| r#"<>:"/\|?*"#.contains(c))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|s| matches!(s, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
    {
        return Err("Choose an application name that is a valid Windows filename.".into());
    }
    if settings.version.is_empty()
        || !settings
            .version
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
    {
        return Err("Use a numeric version such as 1.0.0.".into());
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "each target's packaging and artifact validation are kept in one match arm"
)]
fn build(root: &Path, output: &Path, settings: &Settings, runtime: &Path) -> Result<(), String> {
    let missing = readiness(root, settings);
    if !missing.is_empty() {
        return Err(missing.join("\n"));
    }
    match settings.target {
        Target::Windows => {
            let payload = output.join("game");
            fs::create_dir(&payload).map_err(|e| e.to_string())?;
            fs::copy(
                runtime,
                payload.join(format!("{}.exe", settings.name.trim())),
            )
            .map_err(|e| e.to_string())?;
            fs::rename(output.join("data"), payload.join("data")).map_err(|e| e.to_string())?;
            if let Some(parent) = runtime.parent() {
                for entry in fs::read_dir(parent).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    if entry.file_type().map_err(|e| e.to_string())?.is_file()
                        && (entry
                            .path()
                            .extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
                            || entry.file_name() == "rustic-luau-host.exe")
                    {
                        fs::copy(entry.path(), payload.join(entry.file_name()))
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            if settings.installer {
                let compiler = ensure_installer_compiler(&settings.compiler)?;
                if !available(&compiler.to_string_lossy(), "/?") {
                    return Err("Inno Setup compiler could not run. Check the custom compiler path or repair the Inno Setup installation.".into());
                }
                for (source, target, extension) in [
                    (&settings.license, "license.txt", ""),
                    (&settings.icon, "setup.ico", "ico"),
                    (&settings.banner, "wizard.bmp", "bmp"),
                ] {
                    if !source.is_empty() {
                        let path = project_path(root, source)?;
                        if !extension.is_empty()
                            && !path
                                .extension()
                                .is_some_and(|e| e.eq_ignore_ascii_case(extension))
                        {
                            return Err(format!("{source} must be a .{extension} file."));
                        }
                        fs::copy(path, output.join(target)).map_err(|e| e.to_string())?;
                    }
                }
                fs::write(output.join("installer.iss"), installer_script(settings))
                    .map_err(|e| e.to_string())?;
                run_command(
                    Command::new(compiler)
                        .current_dir(output)
                        .arg("installer.iss"),
                )?;
                if !output.join("setup.exe").is_file() {
                    return Err("Installer compiler did not produce setup.exe.".into());
                }
            }
        }
        Target::Website => {
            portable::export_website(
                &output.join("data"),
                &output.join("website"),
                &portable::runtime_directory(runtime),
                &settings.name,
            )?;
            fs::remove_dir_all(output.join("data")).map_err(|e| e.to_string())?;
        }
        Target::Apk | Target::Aab => {
            let java = java_executable().ok_or("Set JAVA_HOME to a JDK.")?;
            if !available(&java.to_string_lossy(), "-version") {
                return Err(
                    "Java could not run from JAVA_HOME; configure a compatible JDK.".into(),
                );
            }
            let template = output.join("android-project");
            portable::generate_android(
                &output.join("data"),
                &template,
                &portable::runtime_directory(runtime),
                settings,
            )?;
            fs::remove_dir_all(output.join("data")).map_err(|e| e.to_string())?;
            let task = if settings.target == Target::Apk {
                "assembleDebug"
            } else {
                "bundleRelease"
            };
            let sdk = android_sdk().ok_or("Android SDK is unavailable")?;
            run_command(
                Command::new(java)
                    .current_dir(&template)
                    .env("ANDROID_HOME", &sdk)
                    .env("ANDROID_SDK_ROOT", &sdk)
                    .args([
                        "-classpath",
                        "gradle/wrapper/gradle-wrapper.jar",
                        "org.gradle.wrapper.GradleWrapperMain",
                        "--no-daemon",
                        "--console=plain",
                        task,
                    ]),
            )?;
            let (directory, extension) = if settings.target == Target::Apk {
                ("apk", "apk")
            } else {
                ("bundle", "aab")
            };
            let mut artifacts = Vec::new();
            collect_artifacts(
                &template.join("app/build/outputs").join(directory),
                extension,
                &mut artifacts,
            )?;
            if artifacts.is_empty() {
                return Err(format!("Gradle produced no .{extension} artifacts."));
            }
            let destination = output.join("artifacts");
            fs::create_dir(&destination).map_err(|e| e.to_string())?;
            artifacts.sort();
            for (index, artifact) in artifacts.iter().enumerate() {
                fs::copy(
                    artifact,
                    destination.join(format!("app-{index}.{extension}")),
                )
                .map_err(|e| e.to_string())?;
            }
            fs::remove_dir_all(template).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn run_command(command: &mut Command) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("Could not start build tool: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "Build tool exited with {}.\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn copy_tree(source: &Path, destination: &Path, excluded: &[&str]) -> Result<(), String> {
    let canonical_source = source.canonicalize().map_err(|e| e.to_string())?;
    // Resolve the closest existing ancestor so a relative path or linked builds
    // directory cannot accidentally make an export copy itself recursively.
    let ancestor = destination
        .ancestors()
        .find(|path| path.exists())
        .ok_or("No output parent directory")?;
    if ancestor
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(&canonical_source)
    {
        return Err("Build output cannot be inside the source folder.".into());
    }
    if fs::symlink_metadata(source)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err(format!(
            "Build input contains a symbolic link: {}",
            source.display()
        ));
    }
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| excluded.contains(&name))
        {
            continue;
        }
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err(format!(
                "Build input contains a symbolic link: {}",
                entry.path().display()
            ));
        }
        if kind.is_dir() {
            copy_tree(
                &entry.path(),
                &destination.join(entry.file_name()),
                excluded,
            )?;
        } else if kind.is_file() {
            let mut source_file = fs::File::open(entry.path()).map_err(|e| e.to_string())?;
            let mut destination_file =
                fs::File::create(destination.join(entry.file_name())).map_err(|e| e.to_string())?;
            std::io::copy(&mut source_file, &mut destination_file).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn collect_artifacts(
    path: &Path,
    extension: &str,
    output: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            collect_artifacts(&entry.path(), extension, output)?;
        } else if kind.is_file() && entry.path().extension().is_some_and(|e| e == extension) {
            output.push(entry.path());
        }
    }
    Ok(())
}

fn installer_script(settings: &Settings) -> String {
    let name = settings.name.trim();
    let style = if settings.modern { "modern" } else { "classic" };
    let mut script = format!(
        r"[Setup]
AppName={name}
AppVersion={version}
AppPublisher={publisher}
DefaultDirName={{localappdata}}\Programs\{name}
DefaultGroupName={name}
PrivilegesRequired=lowest
WizardStyle={style}
DisableWelcomePage=no
OutputDir=.
OutputBaseFilename=setup
Compression=lzma2
SolidCompression=yes
UninstallDisplayIcon={{app}}\{name}.exe
UninstallFilesDir={{app}}\uninstall
",
        version = settings.version,
        publisher = settings.publisher
    );
    for (value, directive, file) in [
        (&settings.license, "LicenseFile", "license.txt"),
        (&settings.icon, "SetupIconFile", "setup.ico"),
        (&settings.banner, "WizardImageFile", "wizard.bmp"),
    ] {
        if !value.is_empty() {
            let _ = writeln!(script, "{directive}={file}");
        }
    }
    let _ = write!(
        script,
        r#"
[Messages]
WelcomeLabel2={welcome}
[Files]
Source: "game\*"; DestDir: "{{app}}"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{{group}}\{name}"; Filename: "{{app}}\{name}.exe"
"#,
        welcome = settings.welcome
    );
    if settings.desktop_shortcut {
        let _ = write!(
            script,
            r#"Name: "{{autodesktop}}\{name}"; Filename: "{{app}}\{name}.exe"; Tasks: desktopicon
[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
"#
        );
    }
    script
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_installer_directive_and_path_injection() {
        for name in [
            "../escape",
            "CON",
            "COM1.exe",
            "rustic-luau-host",
            "bad\n[Run]",
            "{cmd}",
            "quote\"",
        ] {
            assert!(
                validate_settings(&Settings {
                    name: name.into(),
                    ..Settings::default()
                })
                .is_err()
            );
        }
    }
    #[test]
    fn installer_includes_designed_pages_and_per_user_installation() {
        let settings = Settings {
            name: "My Game".into(),
            license: "license.txt".into(),
            icon: "icon.ico".into(),
            banner: "image.bmp".into(),
            ..Settings::default()
        };
        let script = installer_script(&settings);
        assert!(script.contains("LicenseFile=license.txt"));
        assert!(script.contains("SetupIconFile=setup.ico"));
        assert!(script.contains("WizardImageFile=wizard.bmp"));
        assert!(script.contains("PrivilegesRequired=lowest"));
        assert!(script.contains("{app}\\My Game.exe"));
        assert!(script.contains("WelcomeLabel2=Welcome"));
    }
    #[test]
    fn export_rejects_output_nested_in_source() {
        let temp = tempfile::tempdir().unwrap();
        assert!(copy_tree(temp.path(), &temp.path().join("output"), &[]).is_err());
        assert!(!temp.path().join("output").exists());
    }

    #[test]
    fn packaged_snapshot_preserves_active_edits_and_nested_assets() {
        use engine_project::{Project, ProjectTemplate};
        let temp = tempfile::tempdir().unwrap();
        let project =
            Project::create(temp.path().join("game"), "Test", ProjectTemplate::Empty3d).unwrap();
        engine_scripting::GameSettings::default()
            .save(project.root())
            .unwrap();
        fs::create_dir_all(project.root().join("assets/build")).unwrap();
        fs::write(project.root().join("assets/build/level.txt"), b"asset").unwrap();
        let mut document = AuthoringDocument::open(project, false).unwrap();
        document.add_folder(None).unwrap();
        assert!(document.is_modified());
        let snapshot = crate::play_session::stage_snapshot(
            &document,
            PlayMode::Standalone,
            &temp.path().join("stage"),
        )
        .unwrap();
        let packaged = temp.path().join("data");
        copy_tree(snapshot.root(), &packaged, &[]).unwrap();
        snapshot.remove().unwrap();
        let verified = engine_play::PlaySnapshot::open(&packaged).unwrap();
        assert_eq!(
            verified.scene_bytes().unwrap(),
            document.snapshot_bytes().unwrap()
        );
        assert_eq!(verified.manifest().mode, PlayMode::Standalone);
        assert_eq!(
            fs::read(packaged.join("assets/build/level.txt")).unwrap(),
            b"asset"
        );
        assert!(
            !fs::metadata(packaged.join("assets/build/level.txt"))
                .unwrap()
                .permissions()
                .readonly()
        );
    }

    #[cfg(unix)]
    #[test]
    fn export_rejects_symlinked_content() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("web")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", temp.path().join("web/secret")).unwrap();
        assert!(copy_tree(&temp.path().join("web"), &temp.path().join("out"), &[]).is_err());
        assert!(project_path(temp.path(), "../outside").is_err());
    }
}
