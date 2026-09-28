#[test]
fn local_resolution_regressions() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_grafeo-resolution-prototype"))
        .output()
        .expect("launch local prototype");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(evidence["query"]["authorization"], "pass");
    assert_eq!(evidence["bounded_executor"].as_array().unwrap().len(), 3);
    assert_eq!(evidence["version_carriers"].as_array().unwrap().len(), 2);
}
