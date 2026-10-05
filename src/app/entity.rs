use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::domain::ids::EntityId;
use crate::domain::knowledge::{Entity, EntityReference};
use crate::storage::{database::SqliteDatabase, knowledge};
use crate::{CommonplaceError, Result};

#[derive(Debug, Serialize)]
pub struct EntityList {
    pub items: Vec<Entity>,
    pub truncated: bool,
    pub next_after: Option<EntityId>,
}

pub fn list(
    root: &Path,
    entity_type: Option<&str>,
    missing_identifier: Option<&str>,
    after: Option<&str>,
    limit: usize,
) -> Result<EntityList> {
    if limit > 1000 {
        return Err(CommonplaceError::LimitExceeded(
            "entity list --limit must be between 0 and 1000".into(),
        ));
    }
    let after = after.map(str::parse::<EntityId>).transpose()?;
    let session = SqliteDatabase::read(root)?;
    let mut ids = knowledge::list_entities(
        session.connection(),
        entity_type,
        missing_identifier,
        after,
        limit + 1,
    )?;
    let truncated = ids.len() > limit;
    ids.truncate(limit);
    let next_after = if truncated { ids.last().copied() } else { None };
    let items = ids
        .into_iter()
        .map(|id| knowledge::entity(session.connection(), id))
        .collect::<Result<_>>()?;
    Ok(EntityList {
        items,
        truncated,
        next_after,
    })
}

pub fn resolve(root: &Path, reference: &EntityReference) -> Result<Entity> {
    reference.validate()?;
    if matches!(reference, EntityReference::Ref { .. }) {
        return Err(CommonplaceError::InvalidInput(
            "request-local references are not valid read-only entity selectors".into(),
        ));
    }
    let session = SqliteDatabase::read(root)?;
    let id = knowledge::resolve(session.connection(), reference, &BTreeMap::new())?;
    knowledge::entity(session.connection(), id)
}
