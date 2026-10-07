mod common;

use common::Store;
use serde_json::{Value, json};

fn message(value: &Value) -> &str {
    value["error"]["message"].as_str().unwrap()
}

#[test]
fn six_file_modes_identify_missing_wrong_kind_malformed_and_oversized_inputs() {
    let store = Store::new();
    let missing = store.directory.path().join("missing.json");
    let input = store.directory.path().join("input.json");
    let modes = [
        vec!["search", "query", "--scope"],
        vec!["schema", "apply"],
        vec!["record"],
        vec!["record", "--jsonl"],
        vec!["withdraw"],
        vec!["ingest", "--jsonl"],
    ];
    for prefix in &modes {
        for path in [&missing, store.directory.path()] {
            let mut args = prefix.clone();
            args.push(path.to_str().unwrap());
            let error = store.failure(&args, "invalid_input", 2);
            assert!(message(&error).contains(path.to_str().unwrap()), "{error}");
            let context = if prefix[0] == "search" {
                "--scope"
            } else {
                prefix[0]
            };
            assert!(message(&error).contains(context), "{error}");
        }
        std::fs::write(&input, b"{").unwrap();
        let mut args = prefix.clone();
        args.push(input.to_str().unwrap());
        if prefix[0] == "ingest" {
            let output = store.run(&args);
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stderr.is_empty());
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            let item = &result["result"]["items"][0];
            assert_eq!(item["error"]["code"], "invalid_input");
            assert!(
                item["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains(input.to_str().unwrap())
            );
        } else {
            let error = store.failure(&args, "invalid_input", 2);
            assert!(message(&error).contains(input.to_str().unwrap()), "{error}");
        }
        let limit = if prefix[0] == "search" {
            65536
        } else {
            1048576
        };
        std::fs::write(&input, vec![b' '; limit + 1]).unwrap();
        if prefix[0] == "ingest" {
            let output = store.run(&args);
            assert_eq!(output.status.code(), Some(2));
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                result["result"]["items"][0]["error"]["code"],
                "limit_exceeded"
            );
        } else {
            let error = store.failure(&args, "limit_exceeded", 2);
            assert!(message(&error).contains(input.to_str().unwrap()), "{error}");
        }
    }
    let error = store.failure(
        &["search", "q", "--scope", r#"{"document_ids":[]}"#],
        "invalid_input",
        2,
    );
    assert!(message(&error).contains("not inline JSON"));
    assert!(message(&error).contains("--scope -"));
}

#[cfg(unix)]
#[test]
fn inaccessible_inputs_and_database_report_the_correct_boundary() {
    use std::os::unix::fs::PermissionsExt;
    let store = Store::new();
    let input = store.input(&json!({}));
    std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0)).unwrap();
    // Root and privileged test runners can read mode-000 files.
    if std::fs::File::open(&input).is_err() {
        for prefix in [
            vec!["search", "q", "--scope"],
            vec!["schema", "apply"],
            vec!["record"],
            vec!["record", "--jsonl"],
            vec!["withdraw"],
            vec!["ingest", "--jsonl"],
        ] {
            let mut args = prefix;
            args.push(input.to_str().unwrap());
            let error = store.failure(&args, "invalid_input", 2);
            assert!(message(&error).contains(input.to_str().unwrap()));
        }
    }
    std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o600)).unwrap();
    let database = store.root.join("commonplace.sqlite3");
    std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0)).unwrap();
    if std::fs::File::open(&database).is_err() {
        let error = store.failure(&["schema", "show"], "internal_error", 1);
        assert!(message(&error).contains("commonplace.sqlite3"));
    }
    std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn union_diagnostics_name_the_actual_field_and_keep_line_context() {
    let store = Store::new();
    for (item, expected) in [
        (
            json!({"kind":"entity","name":null}),
            vec!["/items/0/name", "string"],
        ),
        (
            json!({"kind":"entity","name":""}),
            vec!["/items/0/name", "1"],
        ),
        (
            json!({"kind":"entity","name":"N","surprise":true}),
            vec!["surprise"],
        ),
        (
            json!({"kind":"type_membership","entity":{"name":null},"entity_type":"person"}),
            vec!["/items/0/entity/name", "string"],
        ),
        (
            json!({"kind":"fact","subject":{"id":"entity:1"},"predicate":"p","object":{"literal":[]}}),
            vec!["/items/0/object/literal", "string", "integer", "boolean"],
        ),
    ] {
        let input = store.input(&json!({"items":[item]}));
        let error = store.failure(&["record", input.to_str().unwrap()], "invalid_input", 2);
        for text in &expected {
            assert!(message(&error).contains(text), "{error}");
        }
        let invalid = std::fs::read(&input).unwrap();
        std::fs::write(&input, [b"{\"items\":[]}\n".as_slice(), &invalid].concat()).unwrap();
        let error = store.failure(
            &["record", "--jsonl", input.to_str().unwrap()],
            "invalid_input",
            2,
        );
        assert!(message(&error).contains("JSONL line 2"), "{error}");
    }
    let input = store.input(&json!({"document_ids":["doc:1","doc:0"],"truncated":false}));
    let error = store.failure(
        &["search", "q", "--scope", input.to_str().unwrap()],
        "invalid_input",
        2,
    );
    assert!(message(&error).contains("document_ids[1]"), "{error}");
    assert!(message(&error).contains("doc:0"), "{error}");

    std::fs::write(&input, b"{\"source_key\":\"ok\",\"text\":\"\",\"temporal_state\":\"unknown\"}\n{\"source_key\":\"\",\"text\":\"\",\"temporal_state\":\"unknown\"}\n").unwrap();
    let output = store.run(&["ingest", "--jsonl", input.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "partial");
    let error = result["result"]["items"][1]["error"]["message"]
        .as_str()
        .unwrap();
    assert!(error.contains(":2"), "{error}");
    assert!(error.contains("/source_key"), "{error}");
    assert!(!error.contains("occurred_at"), "{error}");
    assert!(!error.contains("temporal_state"), "{error}");
    store.success(&["get", "doc:1"]);
}

#[test]
fn corrupt_stored_ids_metadata_and_wrong_kind_database_do_not_blame_input() {
    let store = Store::new();
    store.database().execute(
        "INSERT INTO entities(entity_id,canonical_name,created_at) VALUES (-1,'corrupt','2026-01-01T00:00:00Z')", []
    ).unwrap();
    let error = store.failure(
        &["entity", "resolve", "--name", "corrupt"],
        "internal_error",
        1,
    );
    assert!(message(&error).contains("stored entity ID -1"), "{error}");
    let input = store.input(&json!({"source_key":"metadata","text":"","temporal_state":"unknown"}));
    store.success(&["ingest", "--jsonl", input.to_str().unwrap()]);
    store.database().execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE document_revisions SET metadata_json='not-json';").unwrap();
    let error = store.failure(&["get", "revision:1"], "internal_error", 1);
    assert!(
        message(&error).contains("stored revision:1 metadata_json"),
        "{error}"
    );
    let database = store.root.join("commonplace.sqlite3");
    std::fs::remove_file(&database).unwrap();
    std::fs::create_dir(&database).unwrap();
    let error = store.failure(&["schema", "show"], "conflict", 3);
    assert!(message(&error).contains("not a regular file"), "{error}");
    assert!(!message(&error).contains("missing"), "{error}");
}

#[test]
fn broken_stdout_never_panics_and_mutation_receipts_match_persisted_state() {
    use std::process::Stdio;
    let store = Store::new();
    let broken = |args: &[&str]| {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let mut command = if args.first() == Some(&"--store") {
            store.command_without_store()
        } else {
            store.command()
        };
        let output = command
            .args(args)
            .stdout(Stdio::from(writer))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "internal_error", "{error}");
        assert!(
            message(&error).contains("response delivery failed"),
            "{error}"
        );
        assert!(!message(&error).contains("cleanup"), "{error}");
        error
    };
    let fresh = store.directory.path().join("fresh");
    let error = broken(&["--store", fresh.to_str().unwrap(), "init"]);
    assert!(message(&error).contains("store created"));
    commonplace::graph::GraphRuntime::open(&fresh).unwrap();
    let input = store.input(&json!({"entity_types":[{"name":"person"}]}));
    let error = broken(&["schema", "apply", input.to_str().unwrap()]);
    assert!(message(&error).contains("schema apply committed"));
    assert_eq!(
        store.success(&["schema", "show"])["result"]["schema_version"],
        1
    );
    std::fs::write(
        &input,
        "{\"source_key\":\"empty\",\"text\":\"\",\"temporal_state\":\"unknown\"}\n{}\n",
    )
    .unwrap();
    let error = broken(&["ingest", "--jsonl", input.to_str().unwrap()]);
    for text in [
        "1 added",
        "1 failed",
        "doc:1",
        "revision:1",
        "do not blindly replay",
    ] {
        assert!(message(&error).contains(text), "{error}");
    }
    store.success(&["get", "doc:1"]);
    let input = store.input(&json!({"items":[
        {"kind":"entity","ref":"p","name":"P"},
        {"kind":"type_membership","entity":{"ref":"p"},"entity_type":"person"}
    ]}));
    let error = broken(&["record", input.to_str().unwrap()]);
    assert!(message(&error).contains("Do not retry record"));
    store.success(&["get", "knowledge:1"]);
    let input = store.input(&json!({"knowledge_ids":["knowledge:1"]}));
    let error = broken(&["withdraw", input.to_str().unwrap()]);
    assert!(message(&error).contains("Do not retry withdraw"));
    assert!(store.success(&["get", "knowledge:1"])["result"]["withdrawn_at"].is_string());
    let error = broken(&["remove", "--source-key", "empty"]);
    assert!(message(&error).contains("Do not retry remove"));
    store.failure(&["get", "doc:1"], "not_found", 2);
    let error = broken(&["graph", "rebuild"]);
    assert!(message(&error).contains("graph rebuild completed"));
    commonplace::graph::GraphRuntime::open(&store.root).unwrap();
    let error = broken(&["schema", "freeze"]);
    assert!(message(&error).contains("Rerun schema freeze"));
    assert!(store.root.join("schema-freeze.json").is_file());
    for args in [
        vec!["graph", "query", "SELECT ?s WHERE {?s ?p ?o}"],
        vec!["schema", "show"],
        vec!["get", "knowledge:1"],
        vec!["schema", "apply", "--describe"],
        vec!["record", "--describe"],
        vec!["ingest", "--describe"],
        vec!["withdraw", "--describe"],
        vec!["remove", "--describe"],
        vec!["search", "--describe-scope"],
        vec!["graph", "schema"],
        vec!["config", "show"],
    ] {
        let error = broken(&args);
        assert!(message(&error).contains("without mutation"), "{error}");
        assert!(!message(&error).contains("committed"), "{error}");
        assert!(!message(&error).contains("rebuild"), "{error}");
    }
}
