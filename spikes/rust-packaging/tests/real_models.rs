use std::process::Command;

#[test]
#[ignore = "requires explicitly prepared immutable COMMONPLACE_MODEL_CACHE; never downloads"]
fn combined_cached_models() {
    let cache = std::env::var_os("COMMONPLACE_MODEL_CACHE")
        .expect("COMMONPLACE_MODEL_CACHE is required; missing weights are a failure, not a skip");
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rust-packaging"))
        .arg("all")
        .env("COMMONPLACE_SPIKE_DATA_DIR", root.path())
        .env("COMMONPLACE_MODEL_CACHE", cache)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["inference"]["status"], "pass");
    assert_eq!(
        report["inference"]["embedding_batches"],
        serde_json::json!([2, 1])
    );
    assert_eq!(report["inference"]["sqlite"]["dimensions"], 384);
    // The inference gate must not disguise the independent graph blockers.
    assert_eq!(report["grafeo"]["gate"], "blocked");
}
