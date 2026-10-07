use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, params};

use crate::domain::documents::Evidence;
use crate::domain::evidence::SupportInput;
use crate::domain::ids::{
    EntityId, EntityTypeId, IdentifierSchemeId, KnowledgeItemId, PassageId, PredicateId,
};
use crate::domain::knowledge::{
    Alias, CanonicalLiteral, Entity, EntityIdentifier, EntityReference, FactObject,
    FactObjectInput, Identifier, IdentityText, Knowledge, KnowledgeDetail,
};
use crate::domain::schema::ObjectKind;
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
            EntityId::stored(id)?
        }
        EntityReference::Name { name } => {
            let ids = ids(
                db,
                "SELECT entity_id FROM entities WHERE canonical_name=?1
                 UNION SELECT entity_id FROM entity_aliases WHERE alias=?1 ORDER BY entity_id LIMIT 2",
                &name.0,
            )?;
            match ids.as_slice() {
                [id] => EntityId::stored(*id)?,
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
    EntityId::stored(db.last_insert_rowid())
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

pub fn list_entities(
    db: &Connection,
    entity_type: Option<&str>,
    missing_identifier: Option<&str>,
    after: Option<EntityId>,
    limit: usize,
) -> Result<Vec<EntityId>> {
    let type_id = entity_type
        .map(|name| entity_type_id(db, name))
        .transpose()?;
    let scheme = missing_identifier
        .map(|name| scheme_id(db, name))
        .transpose()?;
    let mut statement = db
        .prepare(
            "SELECT e.entity_id FROM entities e WHERE e.entity_id > ?1
         AND (?2 IS NULL OR EXISTS (
             SELECT 1 FROM entity_type_memberships m JOIN knowledge_items k USING(knowledge_item_id)
             WHERE m.entity_id=e.entity_id AND m.entity_type_id=?2 AND k.withdrawn_at IS NULL))
         AND (?3 IS NULL OR NOT EXISTS (
             SELECT 1 FROM entity_identifiers i
             WHERE i.entity_id=e.entity_id AND i.identifier_scheme_id=?3))
         ORDER BY e.entity_id LIMIT ?4",
        )
        .map_err(storage_error)?;
    statement
        .query_map(
            params![after.map_or(0, EntityId::value), type_id, scheme, limit],
            |row| row.get::<_, i64>(0),
        )
        .map_err(storage_error)?
        .map(|row| EntityId::stored(row.map_err(storage_error)?))
        .collect()
}

fn entity_type_id(db: &Connection, name: &str) -> Result<i64> {
    db.query_row(
        "SELECT entity_type_id FROM entity_types WHERE name=?1",
        [name],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(storage_error)?
    .ok_or_else(|| CommonplaceError::InvalidInput(format!("unknown entity type {name:?}")))
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
    let type_id = entity_type_id(db, entity_type)?;
    let hydrated = hydrate_support(db, support)?;
    db.execute(
        "INSERT INTO knowledge_items(kind,schema_version,created_at,created_by)
         VALUES ('type_membership',?1,?2,?3)",
        params![schema_version, timestamp, created_by],
    )
    .map_err(storage_error)?;
    let id = KnowledgeItemId::stored(db.last_insert_rowid())?;
    db.execute(
        "INSERT INTO entity_type_memberships(knowledge_item_id,entity_id,entity_type_id) VALUES (?1,?2,?3)",
        params![id.value(), entity.value(), type_id],
    ).map_err(storage_error)?;
    attach_support(db, id, &hydrated)?;
    Ok(Knowledge {
        knowledge_id: id,
        schema_version,
        created_at: timestamp.into(),
        created_by: created_by.map(str::to_owned),
        withdrawn_at: None,
        withdrawn_by: None,
        detail: KnowledgeDetail::TypeMembership {
            entity_id: entity,
            entity_type_id: EntityTypeId::stored(type_id)?,
        },
        support: hydrated,
    })
}

fn hydrate_support(db: &Connection, support: &[SupportInput]) -> Result<Vec<Evidence>> {
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
    Ok(hydrated)
}

fn attach_support(db: &Connection, id: KnowledgeItemId, support: &[Evidence]) -> Result<()> {
    for passage in support {
        db.execute(
            "INSERT INTO knowledge_item_evidence(knowledge_item_id,passage_id) VALUES (?1,?2)",
            params![id.value(), passage.passage_id.value()],
        )
        .map_err(storage_error)?;
    }
    Ok(())
}

pub struct FactWrite<'a> {
    pub subject: EntityId,
    pub predicate: &'a str,
    pub object: &'a FactObjectInput,
    pub support: &'a [SupportInput],
    pub timestamp: &'a str,
    pub created_by: Option<&'a str>,
    pub schema_version: i64,
}

pub fn fact(
    db: &Connection,
    input: FactWrite<'_>,
    refs: &BTreeMap<String, EntityId>,
) -> Result<Knowledge> {
    let (predicate_id, kind) = db
        .query_row(
            "SELECT predicate_id, object_kind FROM predicates WHERE name=?1",
            [input.predicate],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or_else(|| {
            CommonplaceError::InvalidInput(format!("unknown predicate {:?}", input.predicate))
        })?;
    let kind = object_kind(&kind)?;
    let object = match (kind, input.object) {
        (ObjectKind::Entity, FactObjectInput::Entity { entity }) => FactObject::Entity {
            entity_id: resolve(db, entity, refs)?,
        },
        (ObjectKind::Entity, _) | (_, FactObjectInput::Entity { .. }) => {
            return Err(CommonplaceError::InvalidInput(format!(
                "object must match predicate kind {}",
                kind.as_str()
            )));
        }
        (_, FactObjectInput::Literal { literal }) => {
            FactObject::Literal(CanonicalLiteral::new(kind, literal)?)
        }
    };
    let hydrated = hydrate_support(db, input.support)?;
    db.execute(
        "INSERT INTO knowledge_items(kind,schema_version,created_at,created_by) VALUES ('fact',?1,?2,?3)",
        params![input.schema_version, input.timestamp, input.created_by],
    ).map_err(storage_error)?;
    let id = KnowledgeItemId::stored(db.last_insert_rowid())?;
    let (entity, literal) = match &object {
        FactObject::Entity { entity_id } => (Some(entity_id.value()), None),
        FactObject::Literal(value) => (None, Some(value.literal_json.as_str())),
    };
    db.execute(
        "INSERT INTO facts(knowledge_item_id,subject_entity_id,predicate_id,object_entity_id,literal_json)
         VALUES (?1,?2,?3,?4,?5)",
        params![id.value(), input.subject.value(), predicate_id, entity, literal],
    ).map_err(storage_error)?;
    attach_support(db, id, &hydrated)?;
    Ok(Knowledge {
        knowledge_id: id,
        schema_version: input.schema_version,
        created_at: input.timestamp.into(),
        created_by: input.created_by.map(str::to_owned),
        withdrawn_at: None,
        withdrawn_by: None,
        detail: KnowledgeDetail::Fact {
            subject_entity_id: input.subject,
            predicate_id: PredicateId::stored(predicate_id)?,
            object,
        },
        support: hydrated,
    })
}

pub(crate) fn object_kind(kind: &str) -> Result<ObjectKind> {
    match kind {
        "entity" => Ok(ObjectKind::Entity),
        "string" => Ok(ObjectKind::String),
        "integer" => Ok(ObjectKind::Integer),
        "boolean" => Ok(ObjectKind::Boolean),
        "timestamp" => Ok(ObjectKind::Timestamp),
        _ => Err(CommonplaceError::Storage(format!(
            "invalid stored predicate kind {kind:?}"
        ))),
    }
}

pub(crate) fn stored_object(
    kind: &str,
    entity: Option<i64>,
    literal: Option<&str>,
) -> Result<FactObject> {
    match (object_kind(kind)?, entity, literal) {
        (ObjectKind::Entity, Some(id), None) => Ok(FactObject::Entity {
            entity_id: EntityId::stored(id)?,
        }),
        (kind, None, Some(json)) if kind != ObjectKind::Entity => {
            Ok(FactObject::Literal(CanonicalLiteral::stored(kind, json)?))
        }
        _ => Err(CommonplaceError::Storage(
            "stored fact object does not match its predicate kind".into(),
        )),
    }
}

pub(crate) fn invalid_active_endpoint(db: &Connection) -> Result<Option<KnowledgeItemId>> {
    invalid_endpoint_excluding(db, &[])
}

fn invalid_endpoint_excluding(
    db: &Connection,
    excluded: &[KnowledgeItemId],
) -> Result<Option<KnowledgeItemId>> {
    let excluded =
        serde_json::to_string(&excluded.iter().map(|id| id.value()).collect::<Vec<_>>())?;
    db.query_row(
        "WITH excluded AS (SELECT value AS id FROM json_each(?1))
         SELECT f.knowledge_item_id FROM facts f
         JOIN knowledge_items k USING(knowledge_item_id)
         JOIN predicates p USING(predicate_id)
         WHERE k.withdrawn_at IS NULL
           AND k.knowledge_item_id NOT IN (SELECT id FROM excluded) AND (
           NOT EXISTS (
             SELECT 1 FROM entity_type_memberships m
             JOIN knowledge_items mk USING(knowledge_item_id)
             JOIN predicate_entity_types e USING(entity_type_id)
             WHERE m.entity_id=f.subject_entity_id AND mk.withdrawn_at IS NULL
               AND mk.knowledge_item_id NOT IN (SELECT id FROM excluded)
               AND e.predicate_id=f.predicate_id AND e.role='subject')
           OR (p.object_kind='entity' AND NOT EXISTS (
             SELECT 1 FROM entity_type_memberships m
             JOIN knowledge_items mk USING(knowledge_item_id)
             JOIN predicate_entity_types e USING(entity_type_id)
             WHERE m.entity_id=f.object_entity_id AND mk.withdrawn_at IS NULL
               AND mk.knowledge_item_id NOT IN (SELECT id FROM excluded)
               AND e.predicate_id=f.predicate_id AND e.role='object')))
         ORDER BY f.knowledge_item_id LIMIT 1",
        [excluded],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(storage_error)?
    .map(KnowledgeItemId::stored)
    .transpose()
}

pub(crate) fn withdraw(
    db: &Connection,
    ids: &[KnowledgeItemId],
    timestamp: &str,
    withdrawn_by: Option<&str>,
) -> Result<Vec<Knowledge>> {
    let mut items = ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let item = knowledge(db, *id)
                .map_err(|error| error.context(format!("knowledge_ids[{index}] ({id})")))?;
            if item.withdrawn_at.is_some() {
                return Err(CommonplaceError::InvalidInput(format!(
                    "knowledge_ids[{index}]: {id} is already withdrawn; inspect it with get"
                )));
            }
            Ok(item)
        })
        .collect::<Result<Vec<_>>>()?;
    if let Some(id) = invalid_endpoint_excluding(db, ids)? {
        return Err(CommonplaceError::InvalidInput(format!(
            "{id} would have no permitted active type on a required predicate endpoint; \
             retain a permitted type or include this dependent fact in knowledge_ids"
        )));
    }
    for item in &mut items {
        db.execute(
            "UPDATE knowledge_items SET withdrawn_at=?1,withdrawn_by=?2 WHERE knowledge_item_id=?3",
            params![timestamp, withdrawn_by, item.knowledge_id.value()],
        )
        .map_err(storage_error)?;
        item.withdrawn_at = Some(timestamp.into());
        item.withdrawn_by = withdrawn_by.map(str::to_owned);
    }
    Ok(items)
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
                identifier_scheme_id: IdentifierSchemeId::stored(scheme)?,
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
            .into_iter().map(EntityTypeId::stored).collect::<Result<_>>()?,
        active_type_membership_ids: ids(db,
            "SELECT knowledge_item_id FROM entity_type_memberships JOIN knowledge_items USING(knowledge_item_id)
             WHERE entity_id=?1 AND withdrawn_at IS NULL ORDER BY knowledge_item_id", id.value())?
            .into_iter().map(KnowledgeItemId::stored).collect::<Result<_>>()?,
        active_fact_ids: ids(db,
            "SELECT knowledge_item_id FROM facts JOIN knowledge_items USING(knowledge_item_id)
             WHERE (subject_entity_id=?1 OR object_entity_id=?1) AND withdrawn_at IS NULL ORDER BY knowledge_item_id", id.value())?
            .into_iter().map(KnowledgeItemId::stored).collect::<Result<_>>()?,
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
    let detail = match (entity, entity_type) {
        (Some(entity), Some(entity_type)) => KnowledgeDetail::TypeMembership {
            entity_id: EntityId::stored(entity)?,
            entity_type_id: EntityTypeId::stored(entity_type)?,
        },
        _ => {
            let (subject, predicate, kind, entity, literal) = db.query_row(
                "SELECT subject_entity_id,predicate_id,object_kind,object_entity_id,literal_json
                 FROM facts JOIN predicates USING(predicate_id) WHERE knowledge_item_id=?1",
                [id.value()],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?, row.get::<_, Option<String>>(4)?)),
            ).map_err(storage_error)?;
            KnowledgeDetail::Fact {
                subject_entity_id: EntityId::stored(subject)?,
                predicate_id: PredicateId::stored(predicate)?,
                object: stored_object(&kind, entity, literal.as_deref())?,
            }
        }
    };
    let support: Vec<Evidence> = ids(db,
        "SELECT passage_id FROM knowledge_item_evidence WHERE knowledge_item_id=?1 ORDER BY passage_id",
        id.value())?.into_iter().map(|passage| evidence::passage(db, PassageId::stored(passage)?)).collect::<Result<_>>()?;
    Ok(Knowledge {
        knowledge_id: id,
        schema_version,
        created_at,
        created_by,
        withdrawn_at,
        withdrawn_by,
        detail,
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

#[cfg(test)]
mod failure_tests {
    use super::*;

    #[test]
    fn withdrawal_retains_real_sqlite_locked_read_category() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.sqlite3");
        crate::storage::database::SqliteDatabase::initialize(&path).unwrap();
        let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            | rusqlite::OpenFlags::SQLITE_OPEN_SHARED_CACHE;
        let locker = Connection::open_with_flags(&path, flags).unwrap();
        let reader = Connection::open_with_flags(&path, flags).unwrap();
        locker
            .execute_batch("BEGIN IMMEDIATE; UPDATE knowledge_items SET created_at=created_at;")
            .unwrap();
        let id = KnowledgeItemId::new(1).unwrap();
        assert_eq!(knowledge(&reader, id).unwrap_err().code(), "conflict");
        let error = withdraw(&reader, &[id], "2026-10-07T00:00:00Z", None).unwrap_err();
        assert_eq!(error.code(), "conflict");
        assert_eq!(error.exit_code(), 3);
        assert!(error.to_string().contains("knowledge_ids[0] (knowledge:1)"));
        locker.execute_batch("ROLLBACK").unwrap();
    }
}
