use engine_platform::{NativeProcessLauncher, ProcessError, ProcessLauncher, ProcessRequest};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeEditor {
    VisualStudioCode,
    VisualStudioCodeInsiders,
    Custom,
    SystemDefault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectOpenBehavior {
    FileOnly,
    ProjectFolder,
    WorkspaceFile,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditorConfiguration {
    pub format_version: u32,
    pub preferred: CodeEditor,
    pub executable: Option<PathBuf>,
    /// One native argument per entry. Placeholders are `{file}`, `{line}`, `{column}`,
    /// `{project}`, and `{workspace}`. No shell parsing or interpolation occurs.
    pub argument_template: Vec<String>,
    pub project_open_behavior: ProjectOpenBehavior,
    pub refresh_generated_workspace: bool,
}

impl Default for EditorConfiguration {
    fn default() -> Self {
        Self {
            format_version: 1,
            preferred: CodeEditor::VisualStudioCode,
            executable: None,
            argument_template: vec!["--goto".into(), "{file}:{line}:{column}".into()],
            project_open_behavior: ProjectOpenBehavior::WorkspaceFile,
            refresh_generated_workspace: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditorLaunch {
    pub program: PathBuf,
    pub arguments: Vec<OsString>,
    pub working_directory: PathBuf,
}

impl EditorLaunch {
    pub fn process_request(&self) -> ProcessRequest {
        self.arguments.iter().fold(
            ProcessRequest::new(&self.program).working_directory(&self.working_directory),
            engine_platform::ProcessRequest::argument,
        )
    }
}

#[derive(Debug, Error)]
pub enum EditorError {
    #[error("external editor executable was not found or is not a file: {0}")]
    MissingExecutable(PathBuf),
    #[error("custom editor requires an executable path")]
    CustomExecutableRequired,
    #[error("invalid editor argument template: {0}")]
    InvalidTemplate(String),
    #[error("external editor launch failed: {0}")]
    Launch(#[from] ProcessError),
}

pub fn discover_code_editor(preferred: CodeEditor) -> Option<PathBuf> {
    let names: &[&str] = match preferred {
        CodeEditor::VisualStudioCode => {
            if cfg!(windows) {
                &["code.cmd", "code.exe"]
            } else {
                &["code"]
            }
        }
        CodeEditor::VisualStudioCodeInsiders => {
            if cfg!(windows) {
                &["code-insiders.cmd", "code-insiders.exe"]
            } else {
                &["code-insiders"]
            }
        }
        CodeEditor::SystemDefault => {
            if cfg!(windows) {
                &["explorer.exe"]
            } else if cfg!(target_os = "macos") {
                &["open"]
            } else {
                &["xdg-open"]
            }
        }
        CodeEditor::Custom => &[],
    };
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
            .find(|candidate| candidate.is_file())
    })
}

/// Builds a shell-free launch description.
///
/// # Errors
/// Returns an error for a missing executable, unsupported settings, or invalid placeholders.
pub fn build_editor_launch(
    configuration: &EditorConfiguration,
    project: &Path,
    workspace: Option<&Path>,
    file: Option<&Path>,
    line: Option<u32>,
    column: Option<u32>,
) -> Result<EditorLaunch, EditorError> {
    if configuration.format_version != 1 {
        return Err(EditorError::InvalidTemplate(format!(
            "unsupported settings version {}",
            configuration.format_version
        )));
    }
    let program = configuration
        .executable
        .clone()
        .or_else(|| discover_code_editor(configuration.preferred))
        .ok_or_else(|| {
            if configuration.preferred == CodeEditor::Custom {
                EditorError::CustomExecutableRequired
            } else {
                EditorError::MissingExecutable(PathBuf::from(format!(
                    "{:?}",
                    configuration.preferred
                )))
            }
        })?;
    if !program.is_file() {
        return Err(EditorError::MissingExecutable(program));
    }
    let args = if configuration.preferred == CodeEditor::SystemDefault {
        vec![file.unwrap_or(project).as_os_str().to_owned()]
    } else if file.is_some() {
        let Some(file) = file else {
            return Err(EditorError::InvalidTemplate("file was not supplied".into()));
        };
        if configuration.argument_template.is_empty() {
            return Err(EditorError::InvalidTemplate(
                "file argument template is empty".into(),
            ));
        }
        configuration
            .argument_template
            .iter()
            .map(|part| expand(part, project, workspace, Some(file), line, column))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![match configuration.project_open_behavior {
            ProjectOpenBehavior::FileOnly | ProjectOpenBehavior::ProjectFolder => {
                project.as_os_str().to_owned()
            }
            ProjectOpenBehavior::WorkspaceFile => {
                workspace.unwrap_or(project).as_os_str().to_owned()
            }
        }]
    };
    Ok(EditorLaunch {
        program,
        arguments: args,
        working_directory: project.to_path_buf(),
    })
}

fn expand(
    template: &str,
    project: &Path,
    workspace: Option<&Path>,
    file: Option<&Path>,
    line: Option<u32>,
    column: Option<u32>,
) -> Result<OsString, EditorError> {
    for token in ["{file}", "{line}", "{column}", "{project}", "{workspace}"] {
        if template.contains(token)
            && match token {
                "{file}" => file.is_none(),
                "{line}" => line.is_none(),
                "{column}" => column.is_none(),
                "{workspace}" => workspace.is_none(),
                _ => false,
            }
        {
            return Err(EditorError::InvalidTemplate(format!(
                "placeholder {token} has no value"
            )));
        }
    }
    let mut value = template.replace("{project}", &project.to_string_lossy());
    if let Some(path) = workspace {
        value = value.replace("{workspace}", &path.to_string_lossy());
    }
    if let Some(path) = file {
        value = value.replace("{file}", &path.to_string_lossy());
    }
    if let Some(value_line) = line {
        value = value.replace("{line}", &value_line.to_string());
    }
    if let Some(value_column) = column {
        value = value.replace("{column}", &value_column.to_string());
    }
    if value.contains('{') || value.contains('}') {
        return Err(EditorError::InvalidTemplate(format!(
            "unknown placeholder in `{template}`"
        )));
    }
    Ok(value.into())
}

/// Launches the validated native request.
///
/// # Errors
/// Returns an error when the operating system cannot start the editor.
pub fn open_in_external_editor(launch: &EditorLaunch) -> Result<u32, EditorError> {
    NativeProcessLauncher
        .spawn(&launch.process_request())
        .map(engine_platform::SpawnedProcess::id)
        .map_err(EditorError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vscode_goto_preserves_spaces_and_unicode_as_one_argument() {
        let temp = tempfile::tempdir().unwrap();
        let exe = temp.path().join("code.exe");
        std::fs::write(&exe, b"").unwrap();
        let config = EditorConfiguration {
            executable: Some(exe),
            ..EditorConfiguration::default()
        };
        let project = temp.path().join("My Project 游戏");
        let file = project.join("scripts/hero 雪.lua");
        let launch =
            build_editor_launch(&config, &project, None, Some(&file), Some(17), Some(9)).unwrap();
        assert_eq!(launch.arguments.len(), 2);
        assert_eq!(launch.arguments[0], "--goto");
        assert_eq!(
            launch.arguments[1],
            OsString::from(format!("{}:17:9", file.display()))
        );
    }

    #[test]
    fn invalid_custom_configuration_and_unknown_placeholder_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let config = EditorConfiguration {
            preferred: CodeEditor::Custom,
            ..EditorConfiguration::default()
        };
        assert!(matches!(
            build_editor_launch(&config, temp.path(), None, None, None, None),
            Err(EditorError::CustomExecutableRequired)
        ));
        let exe = temp.path().join("editor.exe");
        std::fs::write(&exe, b"").unwrap();
        let config = EditorConfiguration {
            executable: Some(exe),
            argument_template: vec!["{bogus}".into()],
            ..EditorConfiguration::default()
        };
        assert!(matches!(
            build_editor_launch(
                &config,
                temp.path(),
                None,
                Some(&temp.path().join("a.lua")),
                Some(1),
                Some(1)
            ),
            Err(EditorError::InvalidTemplate(_))
        ));
    }
}
