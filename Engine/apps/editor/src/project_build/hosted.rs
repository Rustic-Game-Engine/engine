//! Native website packages retain the ordinary gameplay adapters for every language.
use super::copy_tree;
use std::{fs, path::Path};

pub(super) fn export(
    data: &Path,
    destination: &Path,
    runtime: &Path,
    name: &str,
) -> Result<(), String> {
    engine_play::PlaySnapshot::open(data).map_err(|e| e.to_string())?;
    if !runtime.is_file() {
        return Err("The native game runtime is missing beside the editor.".into());
    }
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    fs::copy(runtime, destination.join(super::native_runtime_name())).map_err(|e| e.to_string())?;
    if let Some(parent) = runtime.parent() {
        for entry in fs::read_dir(parent).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_file()
                && (entry
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("dll") || e == "so" || e == "dylib")
                    || entry.file_name() == "rustic-luau-host.exe"
                    || entry.file_name() == "rustic-luau-host")
            {
                fs::copy(entry.path(), destination.join(entry.file_name()))
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    copy_tree(data, &destination.join("data"), &[])?;
    for (name, contents) in [
        (
            "index.html",
            include_str!("../../../runtime/src/website/player.html"),
        ),
        (
            "player.js",
            include_str!("../../../runtime/src/website/player.js"),
        ),
        ("readme.md", include_str!("hosted-readme.md")),
        (
            "start-website.cmd",
            "@echo off\r\n\"%~dp0rustic-runtime.exe\" --website %*\r\n",
        ),
        (
            "start-website.sh",
            "#!/bin/sh\nexec \"$(dirname -- \"$0\")/rustic-runtime\" --website \"$@\"\n",
        ),
    ] {
        fs::write(destination.join(name), contents).map_err(|e| e.to_string())?;
    }
    fs::write(
        destination.join("game-info.json"),
        serde_json::to_vec(&serde_json::json!({"name":name})).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_preserves_all_language_sources_without_browser_resources() {
        use engine_scripting::register_script_asset;
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("scripts")).unwrap();
        let mut inputs = vec![];
        for extension in [
            "lua", "luau", "js", "py", "c", "cpp", "cs", "java", "php", "html",
        ] {
            let directory = if matches!(extension, "php" | "html") {
                "ui"
            } else {
                "scripts"
            };
            fs::create_dir_all(source.join(directory)).unwrap();
            let relative = std::path::PathBuf::from(format!("{directory}/test.{extension}"));
            fs::write(source.join(&relative), b"source").unwrap();
            register_script_asset(&source, relative.clone()).unwrap();
            inputs.push(engine_play::SnapshotInput::new(
                relative,
                b"source".to_vec(),
            ));
        }
        inputs.push(engine_play::SnapshotInput::new(
            "config/scripts.ron",
            fs::read(source.join("config/scripts.ron")).unwrap(),
        ));
        let snapshot = engine_play::SnapshotBuilder::new(temp.path().join("stage"))
            .stage(
                engine_play::PlayMode::Standalone,
                engine_play::SnapshotInput::new(
                    "scene/main.scene",
                    engine_world::SceneDocument::new("Test").to_bytes().unwrap(),
                ),
                &inputs,
            )
            .unwrap();
        let runtime = temp.path().join(super::super::native_runtime_name());
        fs::write(&runtime, b"runtime").unwrap();
        let output = temp.path().join("website");
        export(snapshot.root(), &output, &runtime, "<Test>").unwrap();
        engine_play::PlaySnapshot::open(output.join("data")).unwrap();
        for input in inputs {
            assert!(output.join("data").join(input.relative_path).is_file());
        }
        assert!(output.join("start-website.cmd").is_file());
        assert!(output.join("start-website.sh").is_file());
        assert!(!output.join("rustic-browser.wasm").exists());
    }
}
