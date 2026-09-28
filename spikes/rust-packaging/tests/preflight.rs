use std::process::Command;

#[test]
fn native_commands_do_not_load_models() {
    for command in ["sqlite", "grafeo"] {
        let root = tempfile::tempdir().unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_rust-packaging"))
            .arg(command)
            .env("COMMONPLACE_SPIKE_DATA_DIR", root.path())
            .env("COMMONPLACE_MODEL_CACHE", root.path().join("absent"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!root.path().join("absent").exists());
    }
}

#[test]
fn missing_models_fail_without_downloading() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rust-packaging"))
        .arg("models")
        .env("COMMONPLACE_SPIKE_DATA_DIR", root.path())
        .env("COMMONPLACE_MODEL_CACHE", root.path().join("absent"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing model artifact"));
    assert!(!root.path().join("absent").exists());
}

#[test]
fn incompatible_models_fail_without_downloading() {
    let root = tempfile::tempdir().unwrap();
    let revision = root.path().join("8f518e882455312b086101e60691f5e6e2f05c3c");
    std::fs::create_dir(&revision).unwrap();
    std::fs::write(revision.join("model.onnx"), b"incompatible").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rust-packaging"))
        .arg("models")
        .env("COMMONPLACE_SPIKE_DATA_DIR", root.path())
        .env("COMMONPLACE_MODEL_CACHE", root.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("incompatible model artifact"));
}
