//! Optional project-authored Windows installer front ends.
use super::{Settings, copy_tree, project_path, run_command};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Manifest {
    Web {
        entry: String,
        #[serde(default)]
        window: Option<WindowOptions>,
    },
    Csharp {
        entry: String,
        #[serde(default)]
        window: Option<WindowOptions>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent native window capabilities"
)]
struct WindowOptions {
    width: u32,
    height: u32,
    min_width: u32,
    min_height: u32,
    max_width: u32,
    max_height: u32,
    title_bar: bool,
    resizable: bool,
    minimize_button: bool,
    maximize_button: bool,
    start_maximized: bool,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            width: 900,
            height: 650,
            min_width: 320,
            min_height: 240,
            max_width: 0,
            max_height: 0,
            title_bar: true,
            resizable: true,
            minimize_button: true,
            maximize_button: true,
            start_maximized: false,
        }
    }
}

impl WindowOptions {
    fn validate(&self) -> Result<(), String> {
        for (axis, initial, minimum, maximum) in [
            ("width", self.width, self.min_width, self.max_width),
            ("height", self.height, self.min_height, self.max_height),
        ] {
            if minimum == 0
                || minimum > 32_767
                || initial < minimum
                || initial > 32_767
                || maximum > 32_767
                || (maximum != 0 && (maximum < minimum || initial > maximum))
            {
                return Err(format!(
                    "Installer window {axis}: use 1..32767, minimum <= initial <= maximum, or maximum 0 for unlimited."
                ));
            }
        }
        if self.start_maximized && (!self.resizable || !self.maximize_button) {
            return Err(
                "Installer window startMaximized requires resizable and maximizeButton.".into(),
            );
        }
        Ok(())
    }
}

impl Manifest {
    fn window(&self) -> Option<&WindowOptions> {
        match self {
            Self::Web { window, .. } | Self::Csharp { window, .. } => window.as_ref(),
        }
    }
}

fn configuration(root: &Path) -> Result<Option<(Manifest, PathBuf)>, String> {
    let directory = root.join("scripts/install");
    let manifest_path = directory.join("installer.json");
    if !manifest_path.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let manifest_path = project_path(root, "scripts/install/installer.json")?;
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest_path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("scripts/install/installer.json: {e}"))?;
    let object = value
        .as_object()
        .ok_or("scripts/install/installer.json must be a JSON object.")?;
    // The companion HTTP installer uses the same filename with a different schema.
    // Only an explicit kind opts into a compiled front end.
    if !object.contains_key("kind") {
        return Ok(None);
    }
    let manifest: Manifest = serde_json::from_value(value)
        .map_err(|e| format!("scripts/install/installer.json: {e}"))?;
    if let Some(window) = manifest.window() {
        window.validate()?;
    }
    let (entry, extension) = match &manifest {
        Manifest::Web { entry, .. } => (entry, "html"),
        Manifest::Csharp { entry, .. } => (entry, "cs"),
    };
    let path = project_path(&directory, entry)?;
    if !path.is_file()
        || !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(extension))
    {
        return Err(format!(
            "Installer entry must be an existing .{extension} file inside scripts/install."
        ));
    }
    Ok(Some((manifest, path)))
}

pub(super) fn validate(root: &Path) -> Result<(), String> {
    configuration(root).map(|_| ())
}

pub(super) fn enabled(root: &Path) -> Result<bool, String> {
    configuration(root).map(|configuration| configuration.is_some())
}

pub(super) fn export(root: &Path, output: &Path, settings: &Settings) -> Result<(), String> {
    let Some((manifest, entry)) = configuration(root)? else {
        return Ok(());
    };
    let (dotnet, _) = engine_scripting::find_dotnet_sdk(8)
        .ok_or("Install .NET SDK 8 or newer to build a custom installer. For a custom SDK location, set DOTNET_ROOT to the folder containing dotnet.exe.")?;
    let sources = tempfile::tempdir().map_err(|e| e.to_string())?;
    let destination = output.join("install");
    let bundle = sources.path().join("bundle");
    copy_tree(
        &root.join("scripts/install"),
        &bundle.join("ui"),
        &["bin", "obj"],
    )?;
    fs::create_dir(bundle.join("payload")).map_err(|e| e.to_string())?;
    let backend = output.join("setup.exe");
    fs::copy(&backend, bundle.join("payload/setup.exe")).map_err(|e| e.to_string())?;
    fs::write(
        bundle.join("game-name.json"),
        serde_json::to_vec(settings.name.trim()).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        bundle.join("window-options.json"),
        serde_json::to_vec(&manifest.window()).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    for (name, source) in [
        (
            "Installer.csproj",
            include_str!("installer_templates/Installer.csproj"),
        ),
        ("Program.cs", include_str!("installer_templates/Program.cs")),
        (
            "WindowOptions.cs",
            include_str!("installer_templates/WindowOptions.cs"),
        ),
    ] {
        fs::write(sources.path().join(name), source).map_err(|e| e.to_string())?;
    }
    match manifest {
        Manifest::Web { entry, .. } => {
            fs::write(
                sources.path().join("InstallerUi.cs"),
                include_str!("installer_templates/WebUi.cs"),
            )
            .map_err(|e| e.to_string())?;
            fs::write(
                bundle.join("ui-entry.json"),
                serde_json::to_vec(&entry).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
        Manifest::Csharp { .. } => {
            fs::copy(entry, sources.path().join("InstallerUi.cs")).map_err(|e| e.to_string())?;
        }
    }
    // The publish bundles the .NET runtime, authored files and Inno Setup payload.
    run_command(
        Command::new(dotnet)
            .current_dir(sources.path())
            .arg("publish")
            .arg(sources.path().join("Installer.csproj"))
            .args([
                "-c",
                "Release",
                "-r",
                "win-x64",
                "--self-contained",
                "true",
                "-o",
            ])
            .arg(&destination),
    )
    .map_err(|e| format!("Could not publish the custom installer UI. Check NuGet access and the compiler output: {e}"))?;
    if !destination.join("setup.exe").is_file() {
        return Err("Custom installer publish did not produce install/setup.exe.".into());
    }
    if fs::read_dir(&destination)
        .map_err(|e| e.to_string())?
        .count()
        != 1
    {
        return Err("Custom installer publish produced files beside install/setup.exe.".into());
    }
    fs::remove_file(backend).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_defaults_limits_and_unknown_fields_are_validated() {
        let defaults = WindowOptions::default();
        assert!(defaults.title_bar && defaults.resizable && defaults.maximize_button);
        assert_eq!((defaults.width, defaults.height), (900, 650));
        defaults.validate().unwrap();
        let custom: WindowOptions = serde_json::from_str(r#"{"titleBar":false,"width":800,"height":500,"minWidth":600,"maxWidth":1200,"maxHeight":800}"#).unwrap();
        custom.validate().unwrap();
        assert!(!custom.title_bar);
        assert_eq!(serde_json::to_value(custom).unwrap()["maxWidth"], 1200);
        for value in [
            r#"{"width":0}"#,
            r#"{"height":32768}"#,
            r#"{"minWidth":901}"#,
            r#"{"maxHeight":400}"#,
            r#"{"minHeight":0}"#,
            r#"{"maxWidth":32768}"#,
            r#"{"resizable":false,"startMaximized":true}"#,
            r#"{"maximizeButton":false,"startMaximized":true}"#,
        ] {
            assert!(
                serde_json::from_str::<WindowOptions>(value)
                    .unwrap()
                    .validate()
                    .is_err(),
                "{value}"
            );
        }
        for value in [
            r#"{"maxWidth":-1}"#,
            r#"{"titlebar":false}"#,
            r#"{"resizable":"no"}"#,
        ] {
            assert!(serde_json::from_str::<WindowOptions>(value).is_err());
        }
    }

    #[test]
    fn both_front_ends_validate_window_settings_and_preserve_omitted_options() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("scripts/install");
        fs::create_dir_all(&install).unwrap();
        for (kind, entry) in [("web", "index.html"), ("csharp", "InstallerUi.cs")] {
            fs::write(install.join(entry), "source").unwrap();
            let manifest = serde_json::json!({"kind": kind, "entry": entry});
            fs::write(install.join("installer.json"), manifest.to_string()).unwrap();
            assert!(
                configuration(root.path())
                    .unwrap()
                    .unwrap()
                    .0
                    .window()
                    .is_none()
            );
            for window in [
                serde_json::json!({"maxWidth": 100}),
                serde_json::json!({"titlebar": false}),
            ] {
                let manifest = serde_json::json!({"kind": kind, "entry": entry, "window": window});
                fs::write(install.join("installer.json"), manifest.to_string()).unwrap();
                assert!(validate(root.path()).is_err());
            }
        }
    }

    #[test]
    #[ignore = "requires dotnet SDK, NuGet access and Windows runtime-pack downloads"]
    fn publishes_both_front_ends_as_standalone_executables() {
        for (kind, entry, source) in [
            ("web", "index.html", "<!doctype html><p>Setup</p>"),
            (
                "csharp",
                "InstallerUi.cs",
                "public sealed class InstallerUi : System.Windows.Forms.Form { public InstallerUi(InstallerContext context) { Text = context.GameName; } }",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let output = tempfile::tempdir().unwrap();
            let install = root.path().join("scripts/install");
            fs::create_dir_all(&install).unwrap();
            fs::write(
                install.join("installer.json"),
                format!(r#"{{"kind":"{kind}","entry":"{entry}","window":{{"titleBar":false,"maxWidth":1200,"maxHeight":800}}}}"#),
            )
            .unwrap();
            fs::write(install.join(entry), source).unwrap();
            fs::write(install.join("asset.txt"), "installer asset fixture").unwrap();
            fs::write(output.path().join("setup.exe"), "backend fixture").unwrap();
            export(root.path(), output.path(), &Settings::default()).unwrap();
            let custom = output.path().join("install");
            assert_eq!(fs::read_dir(&custom).unwrap().count(), 1);
            let executable = fs::read(custom.join("setup.exe")).unwrap();
            assert!(executable.starts_with(b"MZ"));
            for content in [
                b"installer asset fixture".as_slice(),
                b"backend fixture",
                b"\"maxWidth\":1200",
                b"\"titleBar\":false",
            ] {
                assert!(
                    executable
                        .windows(content.len())
                        .any(|bytes| bytes == content)
                );
            }
            assert!(!output.path().join("setup.exe").exists());
        }
    }

    #[test]
    fn optional_manifest_and_both_languages() {
        let root = tempfile::tempdir().unwrap();
        assert!(configuration(root.path()).unwrap().is_none());
        let install = root.path().join("scripts/install");
        fs::create_dir_all(&install).unwrap();
        for (kind, entry) in [("web", "index.html"), ("csharp", "InstallerUi.cs")] {
            fs::write(install.join(entry), "source").unwrap();
            fs::write(
                install.join("installer.json"),
                format!(r#"{{"kind":"{kind}","entry":"{entry}"}}"#),
            )
            .unwrap();
            assert!(configuration(root.path()).unwrap().is_some());
            let output = tempfile::tempdir().unwrap();
            super::super::export_custom_installer(root.path(), output.path(), &Settings::default())
                .unwrap();
            assert!(!output.path().join("scripts/install").exists());
        }
    }
    #[test]
    fn companion_api_manifest_does_not_opt_into_compiled_ui() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("scripts/install");
        fs::create_dir_all(&install).unwrap();
        for manifest in [
            "{}",
            r#"{"title":"My Game","payload":"../../game","screens":[]}"#,
        ] {
            fs::write(install.join("installer.json"), manifest).unwrap();
            assert!(!enabled(root.path()).unwrap());
            validate(root.path()).unwrap();
            let output = tempfile::tempdir().unwrap();
            export(root.path(), output.path(), &Settings::default()).unwrap();
            assert!(!output.path().join("install").exists());
        }
    }
    #[test]
    fn malformed_missing_and_escaping_entries_fail() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("scripts/install");
        fs::create_dir_all(&install).unwrap();
        fs::write(root.path().join("outside.html"), "outside").unwrap();
        for manifest in [
            r#"{"kind":"unknown","entry":"index.html"}"#,
            r#"{"kind":"web","entry":"missing.html"}"#,
            r#"{"kind":"web","entry":"../../outside.html"}"#,
            "null",
        ] {
            fs::write(install.join("installer.json"), manifest).unwrap();
            assert!(configuration(root.path()).is_err());
        }
    }
}
