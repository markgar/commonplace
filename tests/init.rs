mod common;

use common::Store;
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[test]
fn binary_preserves_init_contract_and_existing_store() {
    let store = Store::new();
    let database = store.root.join("commonplace.sqlite3");
    let before = std::fs::read(&database).unwrap();
    assert_eq!(
        store.success(&["init"]),
        json!({
            "operation": "init",
            "contract_version": "1",
            "status": "unchanged",
            "result": {
                "path": store.root,
                "format": "commonplace-store/3",
                "created": false
            }

        })
    );
    assert_eq!(before, std::fs::read(database).unwrap());
    assert!(store.directory.path().is_dir());
}

#[test]
fn initialization_timestamp_is_application_supplied_and_preserved() {
    let before = OffsetDateTime::now_utc().format(&Rfc3339).unwrap();
    let store = Store::new();
    let after = OffsetDateTime::now_utc().format(&Rfc3339).unwrap();
    let created_at: String = store
        .database()
        .query_row("SELECT created_at FROM store_state", [], |row| row.get(0))
        .unwrap();
    assert!(created_at >= before && created_at <= after);
    assert!(created_at.ends_with('Z'));
    store.success(&["init", "--json"]);
    store.apply(&Store::vocabulary(), false);
    assert_eq!(
        created_at,
        store
            .database()
            .query_row("SELECT created_at FROM store_state", [], |row| row
                .get::<_, String>(0))
            .unwrap()
    );
}
