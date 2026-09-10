use engine_assets::{DerivedArtifact, ImportRequest, IsolatedImporter};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn isolated_worker_imports_obj_and_is_reaped() {
    let temporary = tempdir().unwrap();
    let importer = IsolatedImporter {
        program: PathBuf::from(env!("CARGO_BIN_EXE_rustic-asset-worker")),
        working_directory: temporary.path().to_path_buf(),
        timeout: Duration::from_secs(10),
    };
    let request = ImportRequest::new(
        "triangle.obj",
        b"v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nvn 0 0 1\nf 1/1/1 2/2/1 3/3/1\n"
            .to_vec(),
    );
    let artifact = importer.import(request).unwrap();
    assert!(matches!(artifact, DerivedArtifact::Model(_)));
}

#[test]
fn worker_smoke_failures_are_nonzero() {
    let status = Command::new(Path::new(env!("CARGO_BIN_EXE_rustic-asset-worker")))
        .arg("--headless-smoke-fail")
        .status()
        .unwrap();
    assert!(!status.success());
}
