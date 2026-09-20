use engine_project::{
    CreateProjectOptions, PROJECT_DESCRIPTOR_FILE, Project, ProjectError, ProjectTemplate,
    VirtualDirectory,
};
use std::fs;
use tempfile::tempdir;

#[test]
fn create_open_and_save_preserve_identity() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("my-game");
    let mut options = CreateProjectOptions::new("My Game", ProjectTemplate::Empty3d);
    options.author = Some("Engine Tester".to_owned());
    options.engine_version = Some("0.1.0".to_owned());

    let mut created = Project::create_with_options(&root, options).unwrap();
    let original_id = created.id();
    for directory in VirtualDirectory::ALL {
        assert!(created.directory(directory).unwrap().is_dir());
    }

    let descriptor_text = fs::read_to_string(root.join(PROJECT_DESCRIPTOR_FILE)).unwrap();
    assert!(descriptor_text.contains("format_version: 1"));
    assert!(descriptor_text.contains("My Game"));
    assert!(descriptor_text.contains(&original_id.to_string()));

    created.metadata_mut().description = Some("Changed safely".to_owned());
    created.save().unwrap();
    let reopened = Project::open(&root).unwrap();
    assert_eq!(reopened.id(), original_id);
    assert_eq!(
        reopened.metadata().description.as_deref(),
        Some("Changed safely")
    );
}

#[test]
fn custom_virtual_paths_are_created_and_resolved() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("custom-layout");
    let mut options = CreateProjectOptions::new("Custom", ProjectTemplate::Blank);
    options.directories.assets = "game/content".into();
    options.directories.scenes = "game/worlds".into();

    let project = Project::create_with_options(&root, options).unwrap();
    assert_eq!(
        project.directory(VirtualDirectory::Assets).unwrap(),
        root.join("game/content")
    );
    assert!(root.join("game/worlds").is_dir());
}

#[test]
fn traversal_is_rejected_before_writing() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("unsafe-project");
    let escaped = temporary.path().join("escaped");
    let mut options = CreateProjectOptions::new("Unsafe", ProjectTemplate::Blank);
    options.directories.assets = "../escaped".into();

    let result = Project::create_with_options(&root, options);
    assert!(matches!(
        result,
        Err(ProjectError::UnsafeVirtualPath {
            directory: VirtualDirectory::Assets,
            ..
        })
    ));
    assert!(!root.exists());
    assert!(!escaped.exists());
}

#[test]
fn absolute_virtual_path_is_rejected() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("unsafe-absolute");
    let mut options = CreateProjectOptions::new("Unsafe", ProjectTemplate::Blank);
    options.directories.cache = temporary.path().join("external-cache");

    assert!(matches!(
        Project::create_with_options(&root, options),
        Err(ProjectError::UnsafeVirtualPath {
            directory: VirtualDirectory::Cache,
            ..
        })
    ));
    assert!(!root.exists());
}

#[test]
fn create_does_not_touch_a_non_empty_destination() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("occupied");
    fs::create_dir_all(&root).unwrap();
    let sentinel = root.join("keep-me.txt");
    fs::write(&sentinel, "important").unwrap();

    assert!(matches!(
        Project::create(&root, "Nope", ProjectTemplate::Blank),
        Err(ProjectError::DestinationNotEmpty(_))
    ));
    assert_eq!(fs::read_to_string(sentinel).unwrap(), "important");
    assert!(!root.join(PROJECT_DESCRIPTOR_FILE).exists());
}

#[test]
fn validation_detects_missing_virtual_directory() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("missing-dir");
    let project = Project::create(&root, "Missing", ProjectTemplate::Blank).unwrap();
    fs::remove_dir_all(project.directory(VirtualDirectory::Scripts).unwrap()).unwrap();

    assert!(matches!(
        project.validate(),
        Err(ProjectError::MissingVirtualDirectory {
            directory: VirtualDirectory::Scripts,
            ..
        })
    ));
    assert!(Project::open(&root).is_err());
}

#[test]
fn unsupported_descriptor_version_is_rejected() {
    let temporary = tempdir().unwrap();
    let root = temporary.path().join("future-version");
    Project::create(&root, "Future", ProjectTemplate::Blank).unwrap();
    let descriptor_path = root.join(PROJECT_DESCRIPTOR_FILE);
    let contents = fs::read_to_string(&descriptor_path).unwrap();
    fs::write(
        &descriptor_path,
        contents.replacen("format_version: 1", "format_version: 999", 1),
    )
    .unwrap();

    assert!(matches!(
        Project::open(&root),
        Err(ProjectError::UnsupportedFormat {
            found: 999,
            expected: 1
        })
    ));
}
