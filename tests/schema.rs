mod common;

use std::time::{Duration, Instant};

use common::{LockHolder, Store};
use serde_json::{Value, json};

#[test]
fn freeze_is_permanent_repeatable_and_preserves_read_only_schema_operations() {
    let store = Store::new();
    store.apply(&Store::vocabulary(), false);
    let before_vocabulary = store.success(&["schema", "show"])["result"].clone();
    let before_graph = store.graph_files();
    let frozen = store.freeze();
    assert_eq!(
        frozen,
        json!({
            "operation":"schema.freeze", "contract_version":"1", "status":"complete",
            "result":{"schema_version":1,"frozen":true}
        })
    );
    let marker = store.root.join("schema-freeze.json");
    assert_eq!(
        std::fs::read(&marker).unwrap(),
        b"{\"format\":\"commonplace-schema-freeze/1\",\"schema_version\":1}\n"
    );
    let marker_before = std::fs::read(&marker).unwrap();

    let additions = json!({
        "entity_types":[{"name":"team"}],
        "identifier_schemes":[{"name":"team_id"}],
        "predicates":[
            {"name":"works_at","object_kind":"entity","subject_types":["person","company"],"object_types":["company"]},
            {"name":"team_name","object_kind":"string","subject_types":["team"]}
        ]
    });
    let path = store.input(&additions);
    let failure = store.failure(
        &["schema", "apply", path.to_str().unwrap(), "--json"],
        "conflict",
        3,
    );
    assert!(
        failure["error"]["message"]
            .as_str()
            .unwrap()
            .contains("create a fresh store")
    );
    assert_eq!(
        store.success(&["schema", "show"])["result"],
        before_vocabulary
    );
    let checked = store.apply(&additions, true);
    assert_eq!(checked["status"], "checked");
    assert_eq!(checked["result"]["schema_version"], 1);
    assert_eq!(checked["result"]["projected_schema_version"], 2);
    assert_eq!(checked["result"]["changed"], true);
    assert_eq!(
        store.apply(&Store::vocabulary(), false)["status"],
        "unchanged"
    );

    let repeated = store.freeze();
    assert_eq!(repeated["status"], "unchanged");
    assert_eq!(
        repeated["result"],
        json!({"schema_version":1,"frozen":true})
    );
    assert_eq!(std::fs::read(&marker).unwrap(), marker_before);
    assert_eq!(store.graph_files(), before_graph);
    let database = store.database();
    assert_eq!(
        database
            .query_row(
                "SELECT schema_version, knowledge_version FROM store_state",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            )
            .unwrap(),
        (1, 0)
    );
}

#[test]
fn freeze_supports_version_zero_and_replaces_pending_scratch() {
    for pending in [
        b"{".as_slice(),
        b"{\"format\":\"commonplace-schema-freeze/1\",\"schema_version\":0}\n".as_slice(),
        b"{\"format\":\"commonplace-schema-freeze/1\",\"schema_version\":99}\n".as_slice(),
    ] {
        let store = Store::new();
        std::fs::write(store.root.join(".schema-freeze.pending"), pending).unwrap();
        let path = store.input(&json!({}));
        store.failure(&["schema", "apply", path.to_str().unwrap()], "conflict", 3);
        assert_eq!(store.freeze()["result"]["schema_version"], 0);
        assert!(!store.root.join(".schema-freeze.pending").exists());
        assert_eq!(
            std::fs::read(store.root.join("schema-freeze.json")).unwrap(),
            b"{\"format\":\"commonplace-schema-freeze/1\",\"schema_version\":0}\n"
        );
        assert_eq!(store.apply(&json!({}), false)["status"], "unchanged");
    }
}

#[test]
fn freeze_replaces_empty_directory_and_symlink_pending_scratch() {
    let store = Store::new();
    std::fs::create_dir(store.root.join(".schema-freeze.pending")).unwrap();
    assert_eq!(store.freeze()["status"], "complete");
    assert!(!store.root.join(".schema-freeze.pending").exists());

    #[cfg(unix)]
    {
        let store = Store::new();
        let target = store.directory.path().join("outside");
        std::fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, store.root.join(".schema-freeze.pending")).unwrap();
        assert_eq!(store.freeze()["status"], "complete");
        assert_eq!(std::fs::read(target).unwrap(), b"keep");
        assert!(!store.root.join(".schema-freeze.pending").exists());
    }
}

#[test]
fn invalid_final_markers_fail_schema_writes_but_not_show_or_check() {
    for marker in [
        b"{".as_slice(),
        b"{\"format\":\"future\",\"schema_version\":0}\n".as_slice(),
        b"{\"format\":\"commonplace-schema-freeze/1\",\"schema_version\":1}\n".as_slice(),
        b"{ \"format\":\"commonplace-schema-freeze/1\", \"schema_version\":0 }\n".as_slice(),
        b"{\"format\":\"commonplace-schema-freeze/1\",\"schema_version\":0,\"extra\":true}\n"
            .as_slice(),
    ] {
        let store = Store::new();
        std::fs::write(store.root.join("schema-freeze.json"), marker).unwrap();
        assert_eq!(
            store.success(&["schema", "show"])["result"]["schema_version"],
            0
        );
        assert_eq!(store.apply(&json!({}), true)["status"], "checked");
        let path = store.input(&json!({}));
        store.failure(&["schema", "apply", path.to_str().unwrap()], "conflict", 3);
        store.failure(&["schema", "freeze"], "conflict", 3);
    }
}

#[test]
fn non_regular_freeze_paths_fail_closed() {
    let store = Store::new();
    std::fs::create_dir(store.root.join("schema-freeze.json")).unwrap();
    assert_eq!(
        store.success(&["schema", "show"])["result"]["schema_version"],
        0
    );
    let path = store.input(&json!({}));
    store.failure(&["schema", "apply", path.to_str().unwrap()], "conflict", 3);
    store.failure(&["schema", "freeze"], "conflict", 3);
}

#[test]
fn final_marker_with_pending_scratch_fails_closed_until_freeze_confirms_it() {
    let store = Store::new();
    store.freeze();
    std::fs::write(store.root.join(".schema-freeze.pending"), b"stale").unwrap();
    let path = store.input(&json!({}));
    store.failure(&["schema", "apply", path.to_str().unwrap()], "conflict", 3);
    assert_eq!(store.freeze()["status"], "unchanged");
    assert!(!store.root.join(".schema-freeze.pending").exists());
    assert_eq!(store.apply(&json!({}), false)["status"], "unchanged");
}

#[test]
fn apply_reopen_noop_and_endpoint_only_versioning() {
    let store = Store::new();
    let graph = store.graph_files();
    let applied = store.apply(&Store::vocabulary(), false);
    assert_eq!(
        applied,
        json!({
            "operation":"schema.apply", "contract_version":"1", "status":"complete",
            "result":{"schema_version":1,"projected_schema_version":1,"changed":true,"checked":false,
                "summary":{"entity_types":2,"identifier_schemes":1,"predicates":5,"endpoints":6}}
        })
    );
    let shown = store.success(&["schema", "show", "--json"]);
    let result = &shown["result"];
    assert_eq!(result["schema_version"], 1);
    assert_eq!(
        result["entity_types"],
        json!([
            {"id":"entity-type:2","name":"company","description":null,"introduced_version":1},
            {"id":"entity-type:1","name":"person","description":"A person","introduced_version":1}
        ])
    );
    assert_eq!(result["identifier_schemes"][0]["id"], "identifier-scheme:1");
    assert_eq!(result["predicates"].as_array().unwrap().len(), 5);
    assert_eq!(result["predicates"][4]["name"], "works_at");
    assert_eq!(result["predicates"][4]["subject_types"], json!(["person"]));
    assert_eq!(result["predicates"][4]["object_types"], json!(["company"]));
    for predicate in result["predicates"].as_array().unwrap() {
        assert_eq!(predicate["introduced_version"], 1);
        assert!(predicate["id"].as_str().unwrap().starts_with("predicate:"));
    }
    let before = store.files();
    let noop = store.apply(&Store::vocabulary(), false);
    assert_eq!(noop["status"], "unchanged");
    assert_eq!(noop["result"]["schema_version"], 1);
    store.assert_files(&before);
    let endpoint = json!({"predicates":[
        {"name":"works_at","object_kind":"entity","subject_types":["company"],"object_types":["person"]}
    ]});
    let changed = store.apply(&endpoint, false);
    assert_eq!(changed["result"]["schema_version"], 2);
    assert_eq!(
        changed["result"]["summary"],
        json!({"entity_types":0,"identifier_schemes":0,"predicates":0,"endpoints":2})
    );
    let after = store.success(&["schema", "show"]);
    let works_at = &after["result"]["predicates"][4];
    assert_eq!(works_at["introduced_version"], 1);
    assert_eq!(works_at["subject_types"], json!(["company", "person"]));
    assert_eq!(works_at["object_types"], json!(["company", "person"]));
    assert_eq!(store.apply(&endpoint, false)["status"], "unchanged");
    let third = json!({
        "entity_types":[{"name":"team"}],
        "identifier_schemes":[{"name":"team_id"}],
        "predicates":[{"name":"team_name","object_kind":"string","subject_types":["team"]}]
    });
    assert_eq!(store.apply(&third, false)["result"]["schema_version"], 3);
    let third = store.success(&["schema", "show"]);
    assert_eq!(third["result"]["entity_types"][2]["introduced_version"], 3);
    assert_eq!(
        third["result"]["identifier_schemes"][1]["introduced_version"],
        3
    );
    assert_eq!(third["result"]["predicates"][4]["introduced_version"], 3);
    assert_eq!(graph, store.graph_files());
    let database = store.database();
    assert_eq!(
        database
            .query_row("SELECT knowledge_version FROM store_state", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
    assert_eq!(
        database
            .query_row("SELECT COUNT(*) FROM knowledge_items", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn check_is_a_projection_and_does_not_change_store_files() {
    let store = Store::new();
    let before = store.files();
    let checked = store.apply(&Store::vocabulary(), true);
    assert_eq!(checked["status"], "checked");
    assert_eq!(checked["result"]["schema_version"], 0);
    assert_eq!(checked["result"]["projected_schema_version"], 1);
    assert_eq!(checked["result"]["changed"], true);
    store.assert_files(&before);
    assert!(!store.root.join("writer.lock").exists());
    assert_eq!(
        store.success(&["schema", "show"])["result"]["entity_types"],
        json!([])
    );
    store.apply(&Store::vocabulary(), false);
    let before = store.files();
    let noop = store.apply(&Store::vocabulary(), true);
    assert_eq!(noop["result"]["changed"], false);
    assert_eq!(noop["result"]["schema_version"], 1);
    assert_eq!(noop["result"]["projected_schema_version"], 1);
    store.assert_files(&before);
}

#[test]
fn description_is_executable_and_never_opens_a_store() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("missing");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_commonplace"))
        .arg("--store")
        .arg(&root)
        .args(["schema", "apply", "--describe", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!root.exists());
    let description: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(description["operation"], "schema.apply.describe");
    let schema = &description["result"]["input_schema"];
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    let validator = jsonschema::validator_for(schema).unwrap();
    let example = &description["result"]["example"];
    assert!(validator.is_valid(example));
    let store = Store::new();
    assert_eq!(store.apply(example, false)["result"]["schema_version"], 1);
    for input in [
        json!({"unknown":[]}),
        json!({"entity_types":[{"name":"Bad"}]}),
        json!({"entity_types":[{"name":"person\n"}]}),
        json!({"entity_types":[{"name":"person","extra":true}]}),
        json!({"entity_types":[{"name":"person","description":null}]}),
        json!({"predicates":[{"name":"likes","object_kind":"entity","subject_types":["person"]}]}),
        json!({"predicates":[{"name":"age","object_kind":"integer","subject_types":["person"],"object_types":["person"]}]}),
        json!({"predicates":[{"name":"age","object_kind":"float","subject_types":["person"]}]}),
        json!({"predicates":[{"name":"age","object_kind":"integer","subject_types":[]}]}),
        json!([]),
    ] {
        assert!(!validator.is_valid(&input), "{input}");
        let path = store.input(&input);
        store.failure(
            &["schema", "apply", path.to_str().unwrap()],
            "invalid_input",
            2,
        );
    }
}

#[test]
fn invalid_requests_are_atomic_and_descriptions_are_immutable() {
    let store = Store::new();
    store.apply(&Store::vocabulary(), false);
    for invalid in [
        json!({"entity_types":[{"name":"valid_new"},{"name":"person","description":"changed"}]}),
        json!({"identifier_schemes":[{"name":"new_scheme"},{"name":"email","description":"changed"}]}),
        json!({"entity_types":[{"name":"valid_new"}],"predicates":[{"name":"age","object_kind":"string","subject_types":["person"]}]}),
        json!({"entity_types":[{"name":"valid_new"}],"predicates":[{"name":"valid_predicate","object_kind":"entity","subject_types":["person"],"object_types":["missing"]}]}),
        json!({"entity_types":[{"name":"duplicate"},{"name":"duplicate"}]}),
        json!({"identifier_schemes":[{"name":"duplicate"},{"name":"duplicate","description":"different"}]}),
        json!({"predicates":[
            {"name":"new","object_kind":"string","subject_types":["person"]},
            {"name":"new","object_kind":"integer","subject_types":["person"]}
        ]}),
        json!({"predicates":[{"name":"new","object_kind":"string","subject_types":["person","person"]}]}),
    ] {
        let path = store.input(&invalid);
        let before = store.files();
        for suffix in [vec![], vec!["--check"]] {
            let mut args = vec!["schema", "apply", path.to_str().unwrap()];
            args.extend(suffix);
            store.failure(&args, "invalid_input", 2);
            store.assert_files(&before);
        }
    }
    let omitted =
        json!({"entity_types":[{"name":"person"}],"identifier_schemes":[{"name":"email"}]});
    assert_eq!(store.apply(&omitted, false)["status"], "unchanged");
    let shown = store.success(&["schema", "show"]);
    assert_eq!(
        shown["result"]["entity_types"][1]["description"],
        "A person"
    );
    assert_eq!(
        shown["result"]["identifier_schemes"][0]["description"],
        "Email address"
    );
}

#[test]
fn database_failure_rolls_back_all_rows_and_version() {
    let store = Store::new();
    let database = store.database();
    database.execute_batch("CREATE TRIGGER fail_predicate BEFORE INSERT ON predicates BEGIN SELECT RAISE(ABORT, 'injected constraint'); END;").unwrap();
    drop(database);
    let path = store.input(&Store::vocabulary());
    store.failure(
        &["schema", "apply", path.to_str().unwrap()],
        "internal_error",
        1,
    );
    let shown = store.success(&["schema", "show"]);
    assert_eq!(
        shown["result"],
        json!({"schema_version":0,"entity_types":[],"identifier_schemes":[],"predicates":[]})
    );
}

#[test]
fn input_limit_is_enforced_through_binary() {
    let store = Store::new();
    let path = store.directory.path().join("large.json");
    for size in [1024 * 1024 - 1, 1024 * 1024, 1024 * 1024 + 1] {
        let input = format!("{{}}{}", " ".repeat(size - 2));
        std::fs::write(&path, input).unwrap();
        let args = ["schema", "apply", path.to_str().unwrap(), "--check"];
        if size <= 1024 * 1024 {
            assert_eq!(store.success(&args)["result"]["changed"], false);
        } else {
            store.failure(&args, "limit_exceeded", 2);
        }
    }
    assert!(!store.root.join("writer.lock").exists());
}

#[test]
fn malformed_and_duplicate_json_fields_fail_without_mutation() {
    let store = Store::new();
    let before = store.files();
    let path = store.directory.path().join("invalid.json");
    for text in [
        "",
        "{",
        "{} {}",
        r#"{"entity_types":[],"entity_types":[{"name":"person"}]}"#,
        r#"{"entity_types":[{"name":"person","name":"company"}]}"#,
        r#"{"entity_types":[{"name":"person","description":"one","description":"two"}]}"#,
    ] {
        std::fs::write(&path, text).unwrap();
        store.failure(
            &["schema", "apply", path.to_str().unwrap()],
            "invalid_input",
            2,
        );
        store.assert_files(&before);
    }
    std::fs::write(&path, [0xff, 0xfe]).unwrap();
    store.failure(
        &["schema", "apply", path.to_str().unwrap()],
        "invalid_input",
        2,
    );
    assert!(!store.root.join("writer.lock").exists());
}

#[test]
fn exhausted_version_allows_noop_but_rejects_changes_atomically() {
    let store = Store::new();
    store
        .database()
        .execute("UPDATE store_state SET schema_version = ?1", [i64::MAX])
        .unwrap();
    let before = store.files();
    assert_eq!(
        store.apply(&json!({}), true)["result"]["schema_version"],
        i64::MAX
    );
    let path = store.input(&Store::vocabulary());
    store.failure(
        &["schema", "apply", path.to_str().unwrap(), "--check"],
        "conflict",
        3,
    );
    store.assert_files(&before);
    store.failure(&["schema", "apply", path.to_str().unwrap()], "conflict", 3);
    assert_eq!(
        store.success(&["schema", "show"])["result"]["entity_types"],
        json!([])
    );
    assert_eq!(store.apply(&json!({}), false)["status"], "unchanged");
}

#[test]
fn writer_contention_is_bounded_and_termination_releases_lock() {
    let store = Store::new();
    let holder = LockHolder::start(&store);
    assert_eq!(
        store.success(&["schema", "show"])["result"]["schema_version"],
        0
    );
    assert_eq!(
        store.apply(&Store::vocabulary(), true)["result"]["projected_schema_version"],
        1
    );
    let path = store.input(&Store::vocabulary());
    let start = Instant::now();
    store.failure(&["schema", "apply", path.to_str().unwrap()], "conflict", 3);
    assert!(start.elapsed() >= Duration::from_secs(2));
    assert!(start.elapsed() < Duration::from_secs(6));
    drop(holder);
    assert_eq!(
        store.apply(&Store::vocabulary(), false)["result"]["schema_version"],
        1
    );
}

#[test]
fn freeze_uses_the_bounded_writer_lock() {
    let store = Store::new();
    let holder = LockHolder::start(&store);
    assert_eq!(
        store.success(&["schema", "show"])["result"]["schema_version"],
        0
    );
    assert_eq!(store.apply(&json!({}), true)["status"], "checked");
    let start = Instant::now();
    store.failure(&["schema", "freeze"], "conflict", 3);
    assert!(start.elapsed() >= Duration::from_secs(2));
    assert!(start.elapsed() < Duration::from_secs(6));
    drop(holder);
    assert_eq!(store.freeze()["status"], "complete");
}

#[test]
#[ignore = "child-process helper, invoked by lock tests"]
fn lock_holder() {
    common::hold_writer_lock();
}
