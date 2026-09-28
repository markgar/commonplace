use std::process::Command;

#[test]
fn stock_graph_acceptance_without_models() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_oxigraph-evaluation"))
        .arg("graph")
        .env(
            "COMMONPLACE_SPIKE_DATA_DIR",
            root.path().join("graph-probe"),
        )
        .env_remove("COMMONPLACE_MODEL_CACHE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["queries"]["lazy_function_calls"]["after_three_rows"],
        3
    );
    assert_eq!(report["publication"]["restore_previous"], "pass");
}

#[test]
fn sqlite_and_vectors_without_models() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_oxigraph-evaluation"))
        .arg("sqlite")
        .env(
            "COMMONPLACE_SPIKE_DATA_DIR",
            root.path().join("sqlite-probe"),
        )
        .env_remove("COMMONPLACE_MODEL_CACHE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires explicit verified COMMONPLACE_MODEL_CACHE; never downloads weights"]
fn real_combined_models() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        std::env::var_os("COMMONPLACE_MODEL_CACHE").is_some(),
        "set prepared pinned model cache"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_oxigraph-evaluation"))
        .arg("all")
        .env("COMMONPLACE_SPIKE_DATA_DIR", root.path().join("combined"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
