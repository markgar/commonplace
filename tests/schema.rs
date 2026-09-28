mod common;

use std::time::{Duration, Instant};

use common::{LockHolder, Store};
use serde_json::{Value, json};

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
#[ignore = "child-process helper, invoked by lock tests"]
fn lock_holder() {
    common::hold_writer_lock();
}
