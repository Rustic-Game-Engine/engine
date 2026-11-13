//! Optional project-authored Windows installer front ends.
use super::{Settings, copy_tree, project_path, run_command};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Manifest {
    Web { entry: String },
    Csharp { entry: String },
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
    let (entry, extension) = match &manifest {
        Manifest::Web { entry } => (entry, "html"),
        Manifest::Csharp { entry } => (entry, "cs"),
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
    let sources = tempfile::tempdir().map_err(|e| e.to_string())?;
    let destination = output.join("custom-installer");
    fs::create_dir(&destination).map_err(|e| e.to_string())?;
    fs::write(
        sources.path().join("Installer.csproj"),
        include_str!("installer_templates/Installer.csproj"),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        sources.path().join("Program.cs"),
        include_str!("installer_templates/Program.cs"),
    )
    .map_err(|e| e.to_string())?;
    match manifest {
        Manifest::Web { entry } => {
            copy_tree(
                &root.join("scripts/install"),
                &destination.join("ui"),
                &["bin", "obj"],
            )?;
            fs::write(
                sources.path().join("InstallerUi.cs"),
                include_str!("installer_templates/WebUi.cs"),
            )
            .map_err(|e| e.to_string())?;
            fs::write(
                destination.join("ui-entry.json"),
                serde_json::to_vec(&entry).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
        Manifest::Csharp { .. } => {
            fs::copy(entry, sources.path().join("InstallerUi.cs")).map_err(|e| e.to_string())?;
            copy_tree(
                &root.join("scripts/install"),
                &destination.join("ui"),
                &["bin", "obj"],
            )?;
        }
    }
    // A self-contained publish avoids requiring the .NET runtime on player machines.
    run_command(
        Command::new("dotnet")
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
    .map_err(|e| format!("Custom installer UI requires the .NET 8 SDK and NuGet access: {e}"))?;
    if !destination.join("setup.exe").is_file() {
        return Err("Custom installer publish did not produce setup.exe.".into());
    }
    fs::write(
        destination.join("game-name.json"),
        serde_json::to_vec(settings.name.trim()).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::create_dir(destination.join("payload")).map_err(|e| e.to_string())?;
    fs::rename(
        output.join("setup.exe"),
        destination.join("payload/setup.exe"),
    )
    .map_err(|e| e.to_string())?;
    fs::write(destination.join("README.txt"), "Distribute this entire folder. Run setup.exe to open the custom installer UI. Web UIs require Microsoft Edge WebView2 Runtime. payload/setup.exe is the standard Inno Setup fallback installer.\n")
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires dotnet SDK, NuGet access and Windows runtime-pack downloads"]
    fn publishes_both_front_ends_with_assets_and_backend() {
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
                format!(r#"{{"kind":"{kind}","entry":"{entry}"}}"#),
            )
            .unwrap();
            fs::write(install.join(entry), source).unwrap();
            fs::write(install.join("asset.txt"), "asset").unwrap();
            fs::write(output.path().join("setup.exe"), "backend fixture").unwrap();
            export(root.path(), output.path(), &Settings::default()).unwrap();
            let custom = output.path().join("custom-installer");
            assert!(custom.join("setup.exe").is_file());
            assert_eq!(
                fs::read_to_string(custom.join("payload/setup.exe")).unwrap(),
                "backend fixture"
            );
            assert_eq!(
                fs::read_to_string(custom.join("ui/asset.txt")).unwrap(),
                "asset"
            );
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
            assert!(!output.path().join("custom-installer").exists());
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
