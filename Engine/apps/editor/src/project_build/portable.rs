//! Generated exports share one compiled game runtime; projects supply no web/app templates.
use super::{Settings, copy_tree};
use engine_play::PlaySnapshot;
use engine_scripting::{ScriptLanguage, load_manifest};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn runtime_directory(executable: &Path) -> PathBuf {
    let directory = executable.parent().unwrap_or_else(|| Path::new("."));
    let directory = if directory.file_name().is_some_and(|name| name == "deps") {
        directory.parent().unwrap_or(directory)
    } else {
        directory
    };
    directory.join("browser-runtime")
}

pub(super) fn runtime_available(directory: &Path) -> bool {
    directory.join("rustic-browser.js").is_file() && directory.join("rustic-browser.wasm").is_file()
}

pub(super) fn validate_scripts(root: &Path) -> Result<(), String> {
    let path = root.join("config/scripts.ron");
    if !path.is_file() {
        return Ok(());
    }
    let loaded =
        load_manifest(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    for entry in loaded.manifest.scripts {
        if !matches!(
            entry.language,
            ScriptLanguage::Lua54 | ScriptLanguage::JavaScript | ScriptLanguage::Web
        ) {
            return Err(format!(
                "{} uses {}. Direct website/Android exports currently support Lua 5.4, JavaScript and HTML/CSS behaviors; external desktop toolchains cannot run on these targets.",
                entry.relative_path.display(),
                entry.language.display_name()
            ));
        }
        if !root.join(&entry.relative_path).is_file() {
            return Err(format!(
                "Registered script is missing: {}",
                entry.relative_path.display()
            ));
        }
    }
    Ok(())
}

pub(super) fn export_website(
    data: &Path,
    destination: &Path,
    runtime: &Path,
    name: &str,
) -> Result<(), String> {
    let snapshot = PlaySnapshot::open(data).map_err(|e| e.to_string())?;
    validate_scripts(data)?;
    let mut files = vec!["play-snapshot.ron".to_owned()];
    for file in &snapshot.manifest().files {
        let path = file
            .relative_path
            .to_str()
            .ok_or("Export paths must be valid UTF-8")?
            .replace('\\', "/");
        if path.starts_with("ui/") && path.to_ascii_lowercase().ends_with(".php") {
            return Err(format!(
                "{path} requires a PHP server and cannot run in an offline browser/Android game."
            ));
        }
        files.push(path);
    }
    if !runtime_available(runtime) {
        return Err("The engine's compiled browser runtime is missing. Build tools/build-browser-runtime.py or install an engine distribution containing browser-runtime/.".into());
    }
    let wasm = fs::read(runtime.join("rustic-browser.wasm")).map_err(|e| e.to_string())?;
    if !wasm.starts_with(b"\0asm\x01\0\0\0") {
        return Err("The bundled browser runtime is not a valid WebAssembly module.".into());
    }
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    fs::write(destination.join("rustic-browser.wasm"), wasm).map_err(|e| e.to_string())?;
    fs::copy(
        runtime.join("rustic-browser.js"),
        destination.join("rustic-browser.js"),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        destination.join("Ubuntu-Font-Licence.txt"),
        include_bytes!("../../../../LICENSES/Ubuntu-Font-Licence.txt"),
    )
    .map_err(|e| e.to_string())?;
    fs::write(destination.join("Font-Copyright.txt"), "Ubuntu font: Copyright 2011 Canonical Ltd. Licensed under the Ubuntu Font Licence 1.0; see Ubuntu-Font-Licence.txt.\n").map_err(|e| e.to_string())?;
    fs::write(destination.join("index.html"), include_str!("player.html"))
        .map_err(|e| e.to_string())?;
    fs::write(destination.join("player.js"), include_str!("player.js"))
        .map_err(|e| e.to_string())?;
    fs::write(
        destination.join("game-files.json"),
        serde_json::to_vec(&files).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        destination.join("game-info.json"),
        serde_json::to_vec(&serde_json::json!({"name":name})).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    copy_tree(data, &destination.join("data"), &[])?;
    Ok(())
}

pub(super) fn valid_package(value: &str) -> bool {
    let pieces: Vec<_> = value.split('.').collect();
    pieces.len() >= 2
        && pieces.iter().all(|piece| {
            piece.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                && piece
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        })
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(super) fn generate_android(
    data: &Path,
    project: &Path,
    runtime: &Path,
    settings: &Settings,
) -> Result<(), String> {
    if !valid_package(&settings.android_package)
        || !(1..=2_100_000_000).contains(&settings.android_version_code)
    {
        return Err(
            "Enter a package ID such as com.studio.mygame and a positive Android version code."
                .into(),
        );
    }
    let java = project.join("app/src/main/java/org/rustic/player");
    let resources = project.join("app/src/main/res/values");
    let wrapper = project.join("gradle/wrapper");
    for directory in [&java, &resources, &wrapper] {
        fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    }
    let write = |path: PathBuf, bytes: &[u8]| fs::write(path, bytes).map_err(|e| e.to_string());
    write(
        project.join("settings.gradle"),
        include_bytes!("android-settings.gradle"),
    )?;
    write(
        project.join("gradle.properties"),
        b"android.useAndroidX=true\norg.gradle.jvmargs=-Xmx2048m -Dfile.encoding=UTF-8\n",
    )?;
    let gradle = include_str!("android-app.gradle")
        .replace("@APPLICATION_ID@", &settings.android_package)
        .replace("@VERSION_CODE@", &settings.android_version_code.to_string())
        .replace("@VERSION_NAME@", &settings.version);
    write(project.join("app/build.gradle"), gradle.as_bytes())?;
    write(
        wrapper.join("gradle-wrapper.jar"),
        include_bytes!("../../../../assets/android/gradle-wrapper.jar"),
    )?;
    write(
        project.join("Gradle-LICENSE.txt"),
        include_bytes!("../../../../LICENSES/Gradle-Apache-2.0.txt"),
    )?;
    write(wrapper.join("gradle-wrapper.properties"), b"distributionBase=GRADLE_USER_HOME\ndistributionPath=wrapper/dists\ndistributionUrl=https\\://services.gradle.org/distributions/gradle-8.13-bin.zip\ndistributionSha256Sum=20f1b1176237254a6fc204d8434196fa11a4cfb387567519c61556e8710aed78\nnetworkTimeout=60000\nvalidateDistributionUrl=true\nzipStoreBase=GRADLE_USER_HOME\nzipStorePath=wrapper/dists\n")?;
    write(
        java.join("GameActivity.java"),
        include_bytes!("GameActivity.java"),
    )?;
    write(
        resources.join("strings.xml"),
        format!(
            "<resources><string name=\"app_name\">\"{}\"</string></resources>",
            xml(&settings.name)
                .replace('\\', "\\\\")
                .replace('\'', "\\'")
        )
        .as_bytes(),
    )?;
    write(project.join("app/src/main/AndroidManifest.xml"), br#"<manifest xmlns:android="http://schemas.android.com/apk/res/android"><uses-feature android:glEsVersion="0x00030000" android:required="true"/><application android:label="@string/app_name" android:theme="@android:style/Theme.Material.Light.NoActionBar" android:hardwareAccelerated="true" android:allowBackup="false"><activity android:name="org.rustic.player.GameActivity" android:exported="true" android:configChanges="orientation|screenSize|keyboardHidden"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>"#)?;
    export_website(
        data,
        &project.join("app/src/main/assets"),
        runtime,
        &settings.name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_editor::AuthoringDocument;
    use engine_project::{Project, ProjectTemplate};

    fn fixture(root: &Path) -> (PathBuf, PathBuf) {
        let project =
            Project::create(root.join("game"), "Export Test", ProjectTemplate::Empty3d).unwrap();
        engine_scripting::GameSettings::default()
            .save(project.root())
            .unwrap();
        let mut document = AuthoringDocument::open(project, false).unwrap();
        document.add_folder(None).unwrap();
        let snapshot = crate::play_session::stage_snapshot(
            &document,
            engine_play::PlayMode::Standalone,
            &root.join("stage"),
        )
        .unwrap();
        let data = root.join("data");
        copy_tree(snapshot.root(), &data, &[]).unwrap();
        snapshot.remove().unwrap();
        let runtime = root.join("browser-runtime");
        fs::create_dir(&runtime).unwrap();
        // Packaging fixture; actual runtime execution is covered by browser smoke tests.
        fs::write(runtime.join("rustic-browser.wasm"), b"\0asm\x01\0\0\0").unwrap();
        fs::write(runtime.join("rustic-browser.js"), b"// packaging fixture").unwrap();
        (data, runtime)
    }
    #[test]
    fn game_exports_without_any_web_source_folder() {
        let root = tempfile::tempdir().unwrap();
        let (data, runtime) = fixture(root.path());
        let output = root.path().join("website");
        export_website(&data, &output, &runtime, "Test Game").unwrap();
        assert!(!root.path().join("game/web").exists());
        assert!(output.join("index.html").is_file());
        assert!(output.join("rustic-browser.wasm").is_file());
        assert_eq!(
            PlaySnapshot::open(output.join("data"))
                .unwrap()
                .scene_bytes()
                .unwrap(),
            PlaySnapshot::open(&data).unwrap().scene_bytes().unwrap()
        );
        let files: Vec<String> =
            serde_json::from_slice(&fs::read(output.join("game-files.json")).unwrap()).unwrap();
        assert!(files.iter().any(|path| path == "play-snapshot.ron"));
        assert!(files.iter().all(|path| !path.contains('\\')));
    }
    #[test]
    fn android_project_is_generated_with_the_actual_game_and_no_template() {
        let root = tempfile::tempdir().unwrap();
        let (data, runtime) = fixture(root.path());
        let output = root.path().join("generated-android");
        let settings = Settings {
            android_package: "com.example.mygame".into(),
            name: "My & Game's".into(),
            ..Settings::default()
        };
        generate_android(&data, &output, &runtime, &settings).unwrap();
        assert!(!root.path().join("game/android").exists());
        assert!(
            output
                .join("app/src/main/assets/data/play-snapshot.ron")
                .is_file()
        );
        assert!(
            fs::read_to_string(output.join("app/build.gradle"))
                .unwrap()
                .contains("applicationId 'com.example.mygame'")
        );
        let activity = fs::read_to_string(
            output.join("app/src/main/java/org/rustic/player/GameActivity.java"),
        )
        .unwrap();
        assert!(activity.contains("WebViewAssetLoader"));
        assert!(activity.contains("appassets.androidplatform.net/assets/index.html"));
        let manifest = fs::read_to_string(output.join("app/src/main/AndroidManifest.xml")).unwrap();
        assert!(!manifest.contains("android.permission.INTERNET"));
        assert!(
            fs::read_to_string(output.join("app/src/main/res/values/strings.xml"))
                .unwrap()
                .contains("My &amp; Game&apos;s")
        );
        assert!(
            fs::read(output.join("gradle/wrapper/gradle-wrapper.jar"))
                .unwrap()
                .starts_with(b"PK")
        );
    }
    #[test]
    fn incompatible_script_toolchains_are_reported_before_export() {
        let root = tempfile::tempdir().unwrap();
        let (data, _) = fixture(root.path());
        fs::create_dir_all(data.join("scripts")).unwrap();
        fs::write(data.join("scripts/test.py"), b"pass").unwrap();
        engine_scripting::register_script_asset(&data, PathBuf::from("scripts/test.py")).unwrap();
        let error = validate_scripts(&data).unwrap_err();
        assert!(error.contains("Python"));
        assert!(error.contains("external desktop toolchains"));
    }
    #[test]
    #[ignore = "requires compiled browser runtime; invoked by tools/test-browser-export.mjs"]
    fn write_browser_smoke_fixture() {
        use engine_scripting::{
            ScriptApiVersion, ScriptId, ScriptManifest, ScriptManifestEntry, ScriptReference,
        };
        use engine_world::{
            Camera, EntitySnapshot, Light, LocalTransform, PartAttributes, Primitive,
            SceneDocument, ScriptComponent,
        };
        let root = PathBuf::from(
            std::env::var_os("RUSTIC_BROWSER_SMOKE_DIR").expect("Set RUSTIC_BROWSER_SMOKE_DIR"),
        );
        fs::create_dir_all(&root).unwrap();
        let global = ScriptId::new();
        let controller = ScriptId::new();
        let mut scene = SceneDocument::new("Browser Smoke");
        scene.startup_scripts = vec![ScriptReference::new(global)];
        scene.entities = vec![
            EntitySnapshot {
                name: Some("GameCamera".into()),
                camera: Some(Camera {
                    active: false,
                    order: 10,
                    ..Camera::default()
                }),
                local_transform: LocalTransform {
                    translation: glam::Vec3::new(0.0, 0.0, -5.0),
                    ..LocalTransform::IDENTITY
                },
                ..EntitySnapshot::default()
            },
            EntitySnapshot {
                light: Some(Light::default()),
                ..EntitySnapshot::default()
            },
            EntitySnapshot {
                name: Some("Player".into()),
                primitive: Some(Primitive::Cube { size: 2.0 }),
                scripts: vec![ScriptComponent::new(controller)],
                part_attributes: PartAttributes {
                    color: [1.0, 0.02, 0.02, 1.0],
                    anchored: true,
                    can_touch: true,
                    ..PartAttributes::default()
                },
                ..EntitySnapshot::default()
            },
        ];
        let manifest = ScriptManifest {
            format_version: 2,
            scripts: vec![
                ScriptManifestEntry {
                    id: global,
                    language: ScriptLanguage::Lua54,
                    relative_path: "scripts/start.lua".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                },
                ScriptManifestEntry {
                    id: controller,
                    language: ScriptLanguage::JavaScript,
                    relative_path: "scripts/controller.js".into(),
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                },
            ],
        };
        let snapshot = engine_play::SnapshotBuilder::new(root.join("stage")).stage(engine_play::PlayMode::Standalone, engine_play::SnapshotInput::new("scene/main.scene", scene.to_bytes().unwrap()), &[
            engine_play::SnapshotInput::new("config/scripts.ron", ron::ser::to_string(&manifest).unwrap().into_bytes()),
            engine_play::SnapshotInput::new("scripts/start.lua", b"Game.setCurrentCamera('GameCamera'); rustic.log('info','lua-started')".to_vec()),
            engine_play::SnapshotInput::new("scripts/controller.js", br"globalThis.behavior={Start(){rustic.log('info','javascript-started');Input.onClick(rustic.entity_id(),()=>rustic.log('info','object-clicked'));Input.onTouchStart(()=>rustic.log('info','touch-started'));},FixedUpdate(dt){if(rustic.key('KeyD').held){const[x,y,z]=rustic.get_translation();rustic.set_translation(x+dt*5,y,z);rustic.log('info','key-moved');}}};".to_vec()),
        ]).unwrap();
        let runtime = runtime_directory(&std::env::current_exe().unwrap());
        export_website(
            snapshot.root(),
            &root.join("website"),
            &runtime,
            "Browser Smoke",
        )
        .unwrap();
        snapshot.remove().unwrap();
    }

    #[test]
    fn android_metadata_rejects_gradle_and_path_injection() {
        for id in [
            "no_dot",
            "com.example';exec",
            "com../game",
            "com.example.game\n",
            "1.game",
        ] {
            assert!(!valid_package(id));
        }
        assert!(valid_package("com.example.game_2"));
    }
}
