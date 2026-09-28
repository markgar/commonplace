mod common;

use std::process::Command;

use common::Store;
use serde_json::{Value, json};

fn write_config(store: &Store, value: &Value) {
    let path = store.user_config_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn output_json(command: &mut Command) -> Value {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn isolated_commands_remove_inherited_path_overrides() {
    let store = Store::new();
    let command = store.command_without_store();
    for name in ["COMMONPLACE_STORE", "COMMONPLACE_MODEL_CACHE"] {
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key == name && value.is_none()),
            "{name} must be removed unless a test sets it explicitly"
        );
    }
}

#[test]
fn config_show_reports_absent_file_and_fallbacks() {
    let store = Store::new();
    let value = output_json(store.command_without_store().args(["config", "show"]));
    assert_eq!(
        value,
        json!({
            "operation": "config.show",
            "contract_version": "1",
            "status": "complete",
            "result": {
                "config_file": {
                    "path": store.user_config_path(),
                    "status": "absent"
                },
                "store": {
                    "path": ".commonplace",
                    "source": "default"
                },
                "model_cache": {
                    "path": null,
                    "source": "runtime_default"
                }
            }
        })
    );
}

#[test]
fn config_show_reports_file_environment_and_command_line_precedence() {
    let store = Store::new();
    let configured_store = store.directory.path().join("configured-store");
    let configured_models = store.directory.path().join("configured-models");
    write_config(
        &store,
        &json!({
            "format": "commonplace-user-config/1",
            "store": configured_store,
            "model_cache": configured_models
        }),
    );

    let value = output_json(store.command_without_store().args(["config", "show"]));
    assert_eq!(value["result"]["store"]["path"], json!(&configured_store));
    assert_eq!(value["result"]["store"]["source"], "user_config");
    assert_eq!(
        value["result"]["model_cache"]["path"],
        json!(&configured_models)
    );
    assert_eq!(value["result"]["model_cache"]["source"], "user_config");
    assert_eq!(value["result"]["config_file"]["status"], "loaded");

    let value = output_json(
        store
            .command_without_store()
            .env("COMMONPLACE_STORE", "environment-store")
            .env("COMMONPLACE_MODEL_CACHE", "environment-models")
            .args(["config", "show"]),
    );
    assert_eq!(value["result"]["store"]["path"], "environment-store");
    assert_eq!(value["result"]["store"]["source"], "environment");
    assert_eq!(value["result"]["model_cache"]["path"], "environment-models");
    assert_eq!(value["result"]["model_cache"]["source"], "environment");

    let value = output_json(store.command_without_store().args([
        "--store",
        "command-store",
        "--model-cache",
        "command-models",
        "config",
        "show",
    ]));
    assert_eq!(value["result"]["store"]["path"], "command-store");
    assert_eq!(value["result"]["store"]["source"], "command_line");
    assert_eq!(value["result"]["model_cache"]["path"], "command-models");
    assert_eq!(value["result"]["model_cache"]["source"], "command_line");
}

#[test]
fn invalid_config_fails_execution_but_not_help() {
    let store = Store::new();
    let path = store.user_config_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, r#"{"format":"future","unknown":true}"#).unwrap();

    let output = store
        .command_without_store()
        .env("COMMONPLACE_STORE", "override")
        .args(["--model-cache", "override-models", "config", "show"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["operation"], "config.show");
    assert_eq!(error["error"]["code"], "configuration_error");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains(&path.display().to_string())
    );

    let help = store
        .command_without_store()
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("--model-cache")
    );
}

#[test]
fn configured_paths_do_not_act_until_the_owning_command_uses_them() {
    let store = Store::new();
    let configured_store = store.directory.path().join("persistent-store");
    let missing_models = store.directory.path().join("missing-models");
    write_config(
        &store,
        &json!({
            "format": "commonplace-user-config/1",
            "store": configured_store,
            "model_cache": missing_models
        }),
    );

    let shown = output_json(store.command_without_store().args(["config", "show"]));
    assert_eq!(shown["result"]["store"]["path"], json!(&configured_store));
    assert_eq!(
        shown["result"]["model_cache"]["path"],
        json!(&missing_models)
    );
    assert!(!configured_store.exists());
    assert!(!missing_models.exists());

    let initialized = output_json(store.command_without_store().arg("init"));
    assert_eq!(initialized["result"]["path"], json!(&configured_store));
    assert!(configured_store.join("config.json").is_file());
    assert!(!missing_models.exists());

    let output = store
        .command_without_store()
        .args(["search", "needle", "--limit", "0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "model_unavailable");
    assert!(!missing_models.exists());
}

#[test]
fn configured_incompatible_store_is_rejected_without_mutation() {
    let store = Store::new();
    let incompatible = store.directory.path().join("incompatible");
    std::fs::create_dir_all(&incompatible).unwrap();
    std::fs::write(
        incompatible.join("config.json"),
        r#"{"format":"future","database":"commonplace.sqlite3","graph":"graph/current"}"#,
    )
    .unwrap();
    write_config(
        &store,
        &json!({
            "format": "commonplace-user-config/1",
            "store": incompatible
        }),
    );
    let before = std::fs::read(incompatible.join("config.json")).unwrap();
    let output = store.command_without_store().arg("init").output().unwrap();
    assert_eq!(output.status.code(), Some(3));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "conflict");
    assert_eq!(
        std::fs::read(incompatible.join("config.json")).unwrap(),
        before
    );
    assert_eq!(std::fs::read_dir(incompatible).unwrap().count(), 1);
}
