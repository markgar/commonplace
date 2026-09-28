use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::domain::documents::Evidence;
use crate::domain::ids::{EntityId, EntityTypeId, KnowledgeItemId, PredicateId};
use crate::domain::knowledge::{FactObject, KnowledgeDetail, RecordInput, RecordItem};
use crate::storage::{
    database::{SqliteDatabase, storage_error},
    graph_snapshot, knowledge,
};
use crate::{CommonplaceError, Result};

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub items: usize,
    pub entities_created: usize,
    pub metadata_changed: usize,
    pub memberships_created: usize,
    pub facts_created: usize,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemResult {
    Entity {
        index: usize,
        entity_id: EntityId,
        #[serde(skip_serializing_if = "Option::is_none")]
        r#ref: Option<String>,
    },
    EntityMetadata {
        index: usize,
        entity_id: EntityId,
        changed: bool,
    },
    TypeMembership {
        index: usize,
        entity_id: EntityId,
        entity_type_id: EntityTypeId,
        knowledge_id: KnowledgeItemId,
        support: Vec<Evidence>,
    },
    Fact {
        index: usize,
        subject_entity_id: EntityId,
        predicate_id: PredicateId,
        knowledge_id: KnowledgeItemId,
        object: FactObject,
        support: Vec<Evidence>,
    },
}

#[derive(Debug, Serialize)]
pub struct RecordResult {
    pub schema_version: i64,
    pub knowledge_version: i64,
    pub summary: Summary,
    pub items: Vec<ItemResult>,
    #[serde(skip)]
    pub receipt: String,
}

pub fn record(root: &Path, input: RecordInput, timeout: Duration) -> Result<RecordResult> {
    input.validate()?;
    let mut writer = SqliteDatabase::write(root, timeout)?;
    let timestamp = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| CommonplaceError::Storage(error.to_string()))?;
    let transaction = writer.transaction()?;
    let operation = (|| {
        let schema_version: i64 = transaction
            .query_row(
                "SELECT schema_version FROM store_state WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let mut result = RecordResult {
            schema_version,
            knowledge_version: graph_snapshot::version(&transaction)?,
            summary: Summary {
                items: input.items.len(),
                ..Summary::default()
            },
            items: Vec::with_capacity(input.items.len()),
            receipt: String::new(),
        };
        let mut refs = BTreeMap::new();
        for (index, item) in input.items.iter().enumerate() {
            let outcome = (|| match item {
                RecordItem::Entity {
                    r#ref,
                    name,
                    aliases,
                    identifiers,
                } => {
                    if r#ref
                        .as_ref()
                        .is_some_and(|name| refs.contains_key(&name.0))
                    {
                        return Err(CommonplaceError::InvalidInput(
                            "duplicate request-local ref".into(),
                        ));
                    }
                    let entity_id = knowledge::create_entity(
                        &transaction,
                        &name.0,
                        &timestamp,
                        input.created_by.as_deref(),
                    )?;
                    knowledge::metadata(
                        &transaction,
                        entity_id,
                        knowledge::MetadataChange {
                            add_aliases: aliases,
                            remove_aliases: &[],
                            add_identifiers: identifiers,
                            remove_identifiers: &[],
                        },
                        &timestamp,
                        input.created_by.as_deref(),
                    )?;
                    if let Some(name) = r#ref {
                        refs.insert(name.0.clone(), entity_id);
                    }
                    result.summary.entities_created += 1;
                    Ok(ItemResult::Entity {
                        index,
                        entity_id,
                        r#ref: r#ref.as_ref().map(|name| name.0.clone()),
                    })
                }
                RecordItem::EntityMetadata {
                    entity_id,
                    add_aliases,
                    remove_aliases,
                    add_identifiers,
                    remove_identifiers,
                } => {
                    let entity_id = entity_id.parse()?;
                    let changed = knowledge::metadata(
                        &transaction,
                        entity_id,
                        knowledge::MetadataChange {
                            add_aliases,
                            remove_aliases,
                            add_identifiers,
                            remove_identifiers,
                        },
                        &timestamp,
                        input.created_by.as_deref(),
                    )?;
                    result.summary.metadata_changed += usize::from(changed);
                    Ok(ItemResult::EntityMetadata {
                        index,
                        entity_id,
                        changed,
                    })
                }
                RecordItem::TypeMembership {
                    entity,
                    entity_type,
                    support,
                } => {
                    let entity_id = knowledge::resolve(&transaction, entity, &refs)?;
                    let item = knowledge::membership(
                        &transaction,
                        entity_id,
                        &entity_type.0,
                        support,
                        &timestamp,
                        input.created_by.as_deref(),
                        schema_version,
                    )?;
                    result.summary.memberships_created += 1;
                    let KnowledgeDetail::TypeMembership { entity_type_id, .. } = item.detail else {
                        return Err(CommonplaceError::Storage(
                            "membership returned a fact".into(),
                        ));
                    };
                    Ok(ItemResult::TypeMembership {
                        index,
                        entity_id,
                        entity_type_id,
                        knowledge_id: item.knowledge_id,
                        support: item.support,
                    })
                }
                RecordItem::Fact {
                    subject,
                    predicate,
                    object,
                    support,
                } => {
                    let subject = knowledge::resolve(&transaction, subject, &refs)?;
                    let item = knowledge::fact(
                        &transaction,
                        knowledge::FactWrite {
                            subject,
                            predicate: &predicate.0,
                            object,
                            support,
                            timestamp: &timestamp,
                            created_by: input.created_by.as_deref(),
                            schema_version,
                        },
                        &refs,
                    )?;
                    let KnowledgeDetail::Fact {
                        subject_entity_id,
                        predicate_id,
                        object,
                    } = item.detail
                    else {
                        return Err(CommonplaceError::Storage(
                            "fact returned a membership".into(),
                        ));
                    };
                    result.summary.facts_created += 1;
                    Ok(ItemResult::Fact {
                        index,
                        subject_entity_id,
                        predicate_id,
                        knowledge_id: item.knowledge_id,
                        object,
                        support: item.support,
                    })
                }
            })()
            .map_err(|error| item_error(index, error))?;
            result.items.push(outcome);
        }
        if let Some(id) = knowledge::invalid_active_endpoint(&transaction)? {
            let index = result.items.iter().position(
                |item| matches!(item, ItemResult::Fact { knowledge_id, .. } if *knowledge_id == id),
            );
            let error = CommonplaceError::InvalidInput(format!(
                "{id} requires a permitted active type on each predicate endpoint"
            ));
            return Err(match index {
                Some(index) => item_error(index, error),
                None => error,
            });
        }
        let projected_change =
            result.summary.memberships_created > 0 || result.summary.facts_created > 0;
        if projected_change {
            result.knowledge_version = result
                .knowledge_version
                .checked_add(1)
                .ok_or_else(|| CommonplaceError::Storage("knowledge version exhausted".into()))?;
            transaction
                .execute(
                    "UPDATE store_state SET knowledge_version=?1 WHERE singleton=1",
                    [result.knowledge_version],
                )
                .map_err(storage_error)?;
        }
        let ids: Vec<String> = result
            .items
            .iter()
            .map(|item| match item {
                ItemResult::Entity { entity_id, .. }
                | ItemResult::EntityMetadata { entity_id, .. } => entity_id.to_string(),
                ItemResult::TypeMembership { knowledge_id, .. }
                | ItemResult::Fact { knowledge_id, .. } => knowledge_id.to_string(),
            })
            .collect();
        result.receipt = format!(
            "record committed at knowledge_version {} ({})",
            result.knowledge_version,
            ids.join(", ")
        );
        if projected_change {
            crate::graph::publish(root, &transaction, timeout, &result.receipt)?;
        } else {
            transaction.execute_batch("COMMIT").map_err(storage_error)?;
        }
        Ok(result)
    })();
    if let Err(error) = &operation
        && !transaction.is_autocommit()
        && let Err(rollback) = transaction.execute_batch("ROLLBACK")
    {
        return Err(CommonplaceError::Storage(format!(
            "{error}; SQLite rollback failed: {rollback}"
        )));
    }
    operation
}

fn item_error(index: usize, error: CommonplaceError) -> CommonplaceError {
    let message = format!("items[{index}] validation/write: {error}");
    match error {
        CommonplaceError::InvalidInput(_) => CommonplaceError::InvalidInput(message),
        CommonplaceError::NotFound(_) => CommonplaceError::NotFound(message),
        CommonplaceError::Conflict(_) => CommonplaceError::Conflict(message),
        _ => CommonplaceError::Storage(message),
    }
}
