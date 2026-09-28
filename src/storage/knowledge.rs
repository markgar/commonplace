use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, params};

use crate::domain::documents::Evidence;
use crate::domain::evidence::SupportInput;
use crate::domain::ids::{EntityId, EntityTypeId, IdentifierSchemeId, KnowledgeItemId, PassageId};
use crate::domain::knowledge::{
    Alias, Entity, EntityIdentifier, EntityReference, Identifier, IdentityText, Knowledge,
};
use crate::{CommonplaceError, Result};

use super::{database::storage_error, evidence};

pub fn resolve(
    db: &Connection,
    reference: &EntityReference,
    refs: &BTreeMap<String, EntityId>,
) -> Result<EntityId> {
    let id = match reference {
        EntityReference::Id { id } => id.parse()?,
        EntityReference::Ref { r#ref } => *refs.get(&r#ref.0).ok_or_else(|| {
            CommonplaceError::InvalidInput(format!("undefined request-local ref {:?}", r#ref.0))
        })?,
        EntityReference::Identifier { identifier } => {
            let scheme = scheme_id(db, &identifier.scheme.0)?;
            let id = db
                .query_row(
                    "SELECT entity_id FROM entity_identifiers WHERE identifier_scheme_id=?1 AND value=?2",
                    params![scheme, identifier.value.0],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(storage_error)?
                .ok_or_else(|| CommonplaceError::NotFound("identifier does not resolve an entity".into()))?;
            EntityId::new(id)?
        }
        EntityReference::Name { name } => {
            let ids = ids(
                db,
                "SELECT entity_id FROM entities WHERE canonical_name=?1
                 UNION SELECT entity_id FROM entity_aliases WHERE alias=?1 ORDER BY entity_id LIMIT 2",
                &name.0,
            )?;
            match ids.as_slice() {
                [id] => EntityId::new(*id)?,
                [] => {
                    return Err(CommonplaceError::NotFound(format!(
                        "no entity matches name or alias {:?}",
                        name.0
                    )));
                }
                _ => {
                    return Err(CommonplaceError::Conflict(format!(
                        "ambiguous name or alias {:?}; use canonical entity ID or exact identifier",
                        name.0
                    )));
                }
            }
        }
    };
    require_entity(db, id)?;
    Ok(id)
}

pub fn require_entity(db: &Connection, id: EntityId) -> Result<()> {
    let found: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM entities WHERE entity_id=?1)",
            [id.value()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if !found {
        return Err(CommonplaceError::NotFound(format!(
            "{id} does not exist; use an ID returned by record or get"
        )));
    }
    Ok(())
}

pub fn create_entity(
    db: &Connection,
    name: &str,
    timestamp: &str,
    created_by: Option<&str>,
) -> Result<EntityId> {
    db.execute(
        "INSERT INTO entities(canonical_name,created_at,created_by) VALUES (?1,?2,?3)",
        params![name, timestamp, created_by],
    )
    .map_err(storage_error)?;
    EntityId::new(db.last_insert_rowid())
}

pub struct MetadataChange<'a> {
    pub add_aliases: &'a [IdentityText],
    pub remove_aliases: &'a [IdentityText],
    pub add_identifiers: &'a [Identifier],
    pub remove_identifiers: &'a [Identifier],
}

pub fn metadata(
    db: &Connection,
    entity: EntityId,
    change: MetadataChange<'_>,
    timestamp: &str,
    created_by: Option<&str>,
) -> Result<bool> {
    require_entity(db, entity)?;
    let mut aliases = BTreeSet::new();
    for alias in change.add_aliases.iter().chain(change.remove_aliases) {
        if !aliases.insert(&alias.0) {
            return Err(CommonplaceError::InvalidInput(
                "duplicate or conflicting alias additions/removals".into(),
            ));
        }
    }
    let mut identifiers = BTreeSet::new();
    for identifier in change
        .add_identifiers
        .iter()
        .chain(change.remove_identifiers)
    {
        if !identifiers.insert((&identifier.scheme.0, &identifier.value.0)) {
            return Err(CommonplaceError::InvalidInput(
                "duplicate or conflicting identifier additions/removals".into(),
            ));
        }
    }
    let mut changed = false;
    for alias in change.remove_aliases {
        let count = db
            .execute(
                "DELETE FROM entity_aliases WHERE entity_id=?1 AND alias=?2",
                params![entity.value(), alias.0],
            )
            .map_err(storage_error)?;
        if count == 0 {
            return Err(CommonplaceError::NotFound(format!(
                "alias {:?} does not belong to {entity}",
                alias.0
            )));
        }
        changed = true;
    }
    for identifier in change.remove_identifiers {
        let scheme = scheme_id(db, &identifier.scheme.0)?;
        let count = db.execute(
            "DELETE FROM entity_identifiers WHERE entity_id=?1 AND identifier_scheme_id=?2 AND value=?3",
            params![entity.value(), scheme, identifier.value.0],
        ).map_err(storage_error)?;
        if count == 0 {
            return Err(CommonplaceError::NotFound(format!(
                "identifier {}:{:?} does not belong to {entity}",
                identifier.scheme.0, identifier.value.0
            )));
        }
        changed = true;
    }
    for alias in change.add_aliases {
        changed |= db.execute(
            "INSERT INTO entity_aliases(entity_id,alias,created_at,created_by) VALUES (?1,?2,?3,?4)
             ON CONFLICT(entity_id,alias) DO NOTHING",
            params![entity.value(), alias.0, timestamp, created_by],
        ).map_err(storage_error)? > 0;
    }
    for identifier in change.add_identifiers {
        let scheme = scheme_id(db, &identifier.scheme.0)?;
        let owner: Option<i64> = db.query_row(
            "SELECT entity_id FROM entity_identifiers WHERE identifier_scheme_id=?1 AND value=?2",
            params![scheme, identifier.value.0], |row| row.get(0),
        ).optional().map_err(storage_error)?;
        match owner {
            Some(owner) if owner != entity.value() => {
                return Err(CommonplaceError::Conflict(format!(
                    "identifier {}:{:?} already belongs to entity:{owner}",
                    identifier.scheme.0, identifier.value.0
                )));
            }
            Some(_) => {}
            None => {
                db.execute(
                    "INSERT INTO entity_identifiers(entity_id,identifier_scheme_id,value,created_at,created_by)
                     VALUES (?1,?2,?3,?4,?5)",
                    params![entity.value(), scheme, identifier.value.0, timestamp, created_by],
                ).map_err(storage_error)?;
                changed = true;
            }
        }
    }
    Ok(changed)
}

fn scheme_id(db: &Connection, scheme: &str) -> Result<i64> {
    db.query_row(
        "SELECT identifier_scheme_id FROM identifier_schemes WHERE name=?1",
        [scheme],
        |row| row.get(0),
    )
    .optional()
    .map_err(storage_error)?
    .ok_or_else(|| CommonplaceError::InvalidInput(format!("unknown identifier scheme {scheme:?}")))
}

pub fn membership(
    db: &Connection,
    entity: EntityId,
    entity_type: &str,
    support: &[SupportInput],
    timestamp: &str,
    created_by: Option<&str>,
    schema_version: i64,
) -> Result<Knowledge> {
    let type_id = db
        .query_row(
            "SELECT entity_type_id FROM entity_types WHERE name=?1",
            [entity_type],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(|| {
            CommonplaceError::InvalidInput(format!("unknown entity type {entity_type:?}"))
        })?;
    let mut seen = BTreeSet::new();
    let mut hydrated = Vec::with_capacity(support.len());
    for assertion in support {
        let id: PassageId = assertion.passage_id.parse()?;
        if !seen.insert(id) {
            return Err(CommonplaceError::InvalidInput(format!(
                "duplicate support passage {id}"
            )));
        }
        let passage = evidence::passage(db, id)?;
        assertion.validate(&passage)?;
        hydrated.push(passage);
    }
    hydrated.sort_by_key(|passage| passage.passage_id);
    db.execute(
        "INSERT INTO knowledge_items(kind,schema_version,created_at,created_by)
         VALUES ('type_membership',?1,?2,?3)",
        params![schema_version, timestamp, created_by],
    )
    .map_err(storage_error)?;
    let id = KnowledgeItemId::new(db.last_insert_rowid())?;
    db.execute(
        "INSERT INTO entity_type_memberships(knowledge_item_id,entity_id,entity_type_id) VALUES (?1,?2,?3)",
        params![id.value(), entity.value(), type_id],
    ).map_err(storage_error)?;
    for passage in &hydrated {
        db.execute(
            "INSERT INTO knowledge_item_evidence(knowledge_item_id,passage_id) VALUES (?1,?2)",
            params![id.value(), passage.passage_id.value()],
        )
        .map_err(storage_error)?;
    }
    Ok(Knowledge {
        knowledge_id: id,
        subtype: "type_membership",
        schema_version,
        created_at: timestamp.into(),
        created_by: created_by.map(str::to_owned),
        withdrawn_at: None,
        withdrawn_by: None,
        entity_id: entity,
        entity_type_id: EntityTypeId::new(type_id)?,
        support: hydrated,
    })
}

pub fn entity(db: &Connection, id: EntityId) -> Result<Entity> {
    require_entity(db, id)?;
    let (canonical_name, created_at, created_by) = db
        .query_row(
            "SELECT canonical_name,created_at,created_by FROM entities WHERE entity_id=?1",
            [id.value()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(storage_error)?;
    let mut statement = db
        .prepare("SELECT alias,created_at,created_by FROM entity_aliases WHERE entity_id=?1 ORDER BY alias")
        .map_err(storage_error)?;
    let aliases = statement
        .query_map([id.value()], |row| {
            Ok(Alias {
                alias: row.get(0)?,
                created_at: row.get(1)?,
                created_by: row.get(2)?,
            })
        })
        .map_err(storage_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage_error)?;
    let mut statement = db
        .prepare(
            "SELECT identifier_scheme_id,s.name,i.value,i.created_at,i.created_by
         FROM entity_identifiers i JOIN identifier_schemes s USING(identifier_scheme_id)
         WHERE entity_id=?1 ORDER BY s.name,i.value",
        )
        .map_err(storage_error)?;
    let identifiers = statement
        .query_map([id.value()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .map_err(storage_error)?
        .map(|row| {
            let (scheme, name, value, created_at, created_by) = row.map_err(storage_error)?;
            Ok(EntityIdentifier {
                identifier_scheme_id: IdentifierSchemeId::new(scheme)?,
                scheme: name,
                value,
                created_at,
                created_by,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Entity {
        entity_id: id,
        canonical_name,
        created_at,
        created_by,
        aliases,
        identifiers,
        active_type_ids: ids(db,
            "SELECT DISTINCT entity_type_id FROM entity_type_memberships JOIN knowledge_items USING(knowledge_item_id)
             WHERE entity_id=?1 AND withdrawn_at IS NULL ORDER BY entity_type_id", id.value())?
            .into_iter().map(EntityTypeId::new).collect::<Result<_>>()?,
        active_type_membership_ids: ids(db,
            "SELECT knowledge_item_id FROM entity_type_memberships JOIN knowledge_items USING(knowledge_item_id)
             WHERE entity_id=?1 AND withdrawn_at IS NULL ORDER BY knowledge_item_id", id.value())?
            .into_iter().map(KnowledgeItemId::new).collect::<Result<_>>()?,
        active_fact_ids: ids(db,
            "SELECT knowledge_item_id FROM facts JOIN knowledge_items USING(knowledge_item_id)
             WHERE (subject_entity_id=?1 OR object_entity_id=?1) AND withdrawn_at IS NULL ORDER BY knowledge_item_id", id.value())?
            .into_iter().map(KnowledgeItemId::new).collect::<Result<_>>()?,
    })
}

pub fn knowledge(db: &Connection, id: KnowledgeItemId) -> Result<Knowledge> {
    let row = db.query_row(
        "SELECT k.kind,k.schema_version,k.created_at,k.created_by,k.withdrawn_at,k.withdrawn_by,
                m.entity_id,m.entity_type_id,f.knowledge_item_id
         FROM knowledge_items k LEFT JOIN entity_type_memberships m USING(knowledge_item_id)
         LEFT JOIN facts f USING(knowledge_item_id) WHERE k.knowledge_item_id=?1",
        [id.value()],
        |row| Ok((
            row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?, row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<i64>>(7)?, row.get::<_, Option<i64>>(8)?,
        )),
    ).optional().map_err(storage_error)?
        .ok_or_else(|| CommonplaceError::NotFound(format!("{id} does not exist; use an ID returned by record")))?;
    let (
        kind,
        schema_version,
        created_at,
        created_by,
        withdrawn_at,
        withdrawn_by,
        entity,
        entity_type,
        fact,
    ) = row;
    if !matches!(
        (kind.as_str(), entity, entity_type, fact),
        ("type_membership", Some(_), Some(_), None) | ("fact", None, None, Some(_))
    ) {
        return Err(CommonplaceError::Storage(format!(
            "{id} has invalid subtype cardinality or kind"
        )));
    }
    let (Some(entity), Some(entity_type)) = (entity, entity_type) else {
        return Err(CommonplaceError::InvalidInput(
            "fact reads are not supported by this release".into(),
        ));
    };
    let support: Vec<Evidence> = ids(db,
        "SELECT passage_id FROM knowledge_item_evidence WHERE knowledge_item_id=?1 ORDER BY passage_id",
        id.value())?.into_iter().map(|passage| evidence::passage(db, PassageId::new(passage)?)).collect::<Result<_>>()?;
    Ok(Knowledge {
        knowledge_id: id,
        subtype: "type_membership",
        schema_version,
        created_at,
        created_by,
        withdrawn_at,
        withdrawn_by,
        entity_id: EntityId::new(entity)?,
        entity_type_id: EntityTypeId::new(entity_type)?,
        support,
    })
}

fn ids(db: &Connection, sql: &str, parameter: impl rusqlite::ToSql) -> Result<Vec<i64>> {
    db.prepare(sql)
        .map_err(storage_error)?
        .query_map([parameter], |row| row.get(0))
        .map_err(storage_error)?
        .collect::<rusqlite::Result<_>>()
        .map_err(storage_error)
}
