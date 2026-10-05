use rusqlite::{Connection, Row, params};

use super::database::storage_error;
use super::knowledge;
use crate::domain::documents::TemporalState;
use crate::domain::knowledge::FactObject;
use crate::{CommonplaceError, Result};

const PAGE_SIZE: i64 = 256;
const ACTIVE: &str = "WITH active AS (
    SELECT k.knowledge_item_id, k.schema_version
    FROM knowledge_items k
    WHERE k.withdrawn_at IS NULL
), memberships AS (
    SELECT a.*, m.entity_id, m.entity_type_id FROM active a
    JOIN entity_type_memberships m USING(knowledge_item_id)
), active_facts AS (
    SELECT a.*, f.subject_entity_id, f.predicate_id, f.object_entity_id, f.literal_json
    FROM active a JOIN facts f USING(knowledge_item_id)
), referenced_entities AS (
    SELECT entity_id FROM memberships
    UNION SELECT subject_entity_id FROM active_facts
    UNION SELECT object_entity_id FROM active_facts WHERE object_entity_id IS NOT NULL
), cited AS (
    SELECT DISTINCT e.passage_id FROM knowledge_item_evidence e
    JOIN active a USING (knowledge_item_id)
)";

pub(crate) enum SnapshotRecord {
    Membership {
        id: i64,
        schema_version: i64,
        entity: i64,
        entity_type: i64,
    },
    Fact {
        id: i64,
        schema_version: i64,
        subject: i64,
        predicate: i64,
        object: FactObject,
    },
    Predicate {
        id: i64,
        name: String,
        object_kind: String,
    },
    Entity {
        id: i64,
        name: String,
    },
    EntityType {
        id: i64,
        name: String,
    },
    Passage {
        id: i64,
        revision: i64,
        ordinal: i64,
        start: i64,
        end: i64,
        text: String,
    },
    Revision {
        id: i64,
        document: i64,
        number: i64,
        digest: String,
        source_type: String,
        temporal_state: TemporalState,
        metadata: String,
        title: Option<String>,
        occurred_at: Option<String>,
    },
    Document {
        id: i64,
        source_key: String,
    },
    Evidence {
        knowledge: i64,
        passage: i64,
    },
}

pub(crate) struct GraphSnapshot<'a> {
    connection: &'a Connection,
    pub(crate) knowledge_version: i64,
}

pub(crate) fn version(connection: &Connection) -> Result<i64> {
    connection
        .query_row(
            "SELECT knowledge_version FROM store_state WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(storage_error)
}

impl<'a> GraphSnapshot<'a> {
    pub(crate) fn read(connection: &'a Connection) -> Result<Self> {
        if connection.is_autocommit() {
            return Err(CommonplaceError::Graph(
                "graph snapshot requires the caller's SQLite transaction".into(),
            ));
        }
        let knowledge_version = version(connection)?;
        if knowledge_version < 0 {
            return Err(CommonplaceError::Graph(
                "negative SQLite knowledge version".into(),
            ));
        }
        let invalid: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM knowledge_items k
                LEFT JOIN entity_type_memberships m USING (knowledge_item_id)
                LEFT JOIN facts f USING (knowledge_item_id)
                WHERE k.withdrawn_at IS NULL AND (k.knowledge_item_id <= 0 OR k.schema_version <= 0 OR NOT (
                    (k.kind='type_membership' AND m.knowledge_item_id IS NOT NULL AND f.knowledge_item_id IS NULL)
                    OR (k.kind='fact' AND f.knowledge_item_id IS NOT NULL AND m.knowledge_item_id IS NULL)
                )))", [], |r| r.get(0)).map_err(storage_error)?;
        if invalid {
            return Err(CommonplaceError::Graph(
                "active knowledge has invalid subtype cardinality or kind".into(),
            ));
        }
        if let Some(id) = knowledge::invalid_active_endpoint(connection)? {
            return Err(CommonplaceError::Graph(format!(
                "{id} has no permitted active type on a required predicate endpoint"
            )));
        }
        let mut check = connection
            .prepare("PRAGMA foreign_key_check")
            .map_err(storage_error)?;
        if check
            .query([])
            .map_err(storage_error)?
            .next()
            .map_err(storage_error)?
            .is_some()
        {
            return Err(CommonplaceError::Graph(
                "SQLite foreign key corruption prevents graph projection".into(),
            ));
        }
        Ok(Self {
            connection,
            knowledge_version,
        })
    }

    pub(crate) fn visit(&self, mut emit: impl FnMut(SnapshotRecord) -> Result<()>) -> Result<()> {
        self.pages(
            "SELECT knowledge_item_id, schema_version, entity_id, entity_type_id FROM memberships
             WHERE knowledge_item_id>?1 ORDER BY knowledge_item_id LIMIT ?2",
            |r| {
                Ok(SnapshotRecord::Membership {
                    id: r.get(0)?,
                    schema_version: r.get(1)?,
                    entity: r.get(2)?,
                    entity_type: r.get(3)?,
                })
            },
            &mut emit,
        )?;
        self.pages(
            "SELECT knowledge_item_id,schema_version,subject_entity_id,predicate_id,
                    object_kind,object_entity_id,literal_json
             FROM active_facts JOIN predicates USING(predicate_id)
             WHERE knowledge_item_id>?1 ORDER BY knowledge_item_id LIMIT ?2",
            |r| {
                let kind: String = r.get(4)?;
                let literal: Option<String> = r.get(6)?;
                let object = knowledge::stored_object(&kind, r.get(5)?, literal.as_deref())
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            6,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                Ok(SnapshotRecord::Fact {
                    id: r.get(0)?,
                    schema_version: r.get(1)?,
                    subject: r.get(2)?,
                    predicate: r.get(3)?,
                    object,
                })
            },
            &mut emit,
        )?;
        self.pages(
            "SELECT entity_id, canonical_name FROM entities
             WHERE entity_id>?1 AND entity_id IN (SELECT entity_id FROM referenced_entities)
             ORDER BY entity_id LIMIT ?2",
            |r| {
                Ok(SnapshotRecord::Entity {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            },
            &mut emit,
        )?;
        self.pages(
            "SELECT entity_type_id, name FROM entity_types
             WHERE entity_type_id>?1 AND entity_type_id IN (SELECT entity_type_id FROM memberships)
             ORDER BY entity_type_id LIMIT ?2",
            |r| {
                Ok(SnapshotRecord::EntityType {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            },
            &mut emit,
        )?;
        self.pages(
            "SELECT predicate_id,name,object_kind FROM predicates
             WHERE predicate_id>?1 AND predicate_id IN (SELECT predicate_id FROM active_facts)
             ORDER BY predicate_id LIMIT ?2",
            |r| {
                Ok(SnapshotRecord::Predicate {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    object_kind: r.get(2)?,
                })
            },
            &mut emit,
        )?;
        self.pages(
            "SELECT p.passage_id, p.revision_id, p.ordinal, p.start_byte, p.end_byte, p.text, r.text
             FROM passages p JOIN document_revisions r USING (revision_id)
             WHERE p.passage_id>?1 AND p.passage_id IN (SELECT passage_id FROM cited)
             ORDER BY p.passage_id LIMIT ?2",
            |r| {
                let start: i64 = r.get(3)?;
                let end: i64 = r.get(4)?;
                let text: String = r.get(5)?;
                let source: String = r.get(6)?;
                let valid = usize::try_from(start).ok().zip(usize::try_from(end).ok())
                    .and_then(|(start,end)| source.get(start..end)) == Some(text.as_str());
                if !valid {
                    return Err(rusqlite::Error::FromSqlConversionFailure(5,
                        rusqlite::types::Type::Text, "passage text/UTF-8 byte offsets do not match its revision".into()));
                }
                Ok(SnapshotRecord::Passage { id:r.get(0)?, revision:r.get(1)?, ordinal:r.get(2)?, start, end, text })
            }, &mut emit)?;
        self.pages(
            "SELECT revision_id, document_id, revision_number, revision_digest, source_type,
                    metadata_json, title, occurred_at, temporal_state FROM document_revisions
             WHERE revision_id>?1 AND revision_id IN (
                 SELECT revision_id FROM passages WHERE passage_id IN (SELECT passage_id FROM cited))
             ORDER BY revision_id LIMIT ?2",
            |r| {
                let occurred_at: Option<String> = r.get(7)?;
                let state: String = r.get(8)?;
                let temporal_state = super::evidence::decode_temporal_state(&state, occurred_at.as_deref())
                    .map_err(|error| rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(error)))?;
                Ok(SnapshotRecord::Revision {
                id:r.get(0)?, document:r.get(1)?, number:r.get(2)?, digest:r.get(3)?,
                source_type:r.get(4)?, temporal_state, metadata:r.get(5)?, title:r.get(6)?, occurred_at,
            })}, &mut emit)?;
        self.pages(
            "SELECT document_id, source_key FROM documents
             WHERE document_id>?1 AND document_id IN (
                 SELECT document_id FROM document_revisions WHERE revision_id IN (
                     SELECT revision_id FROM passages WHERE passage_id IN (SELECT passage_id FROM cited)))
             ORDER BY document_id LIMIT ?2",
            |r| Ok(SnapshotRecord::Document { id:r.get(0)?, source_key:r.get(1)? }), &mut emit)?;
        let mut statement = self
            .connection
            .prepare(&format!(
                "{ACTIVE}
            SELECT e.knowledge_item_id, e.passage_id FROM knowledge_item_evidence e
            JOIN active a USING (knowledge_item_id)
            WHERE (e.knowledge_item_id,e.passage_id)>(?1,?2)
            ORDER BY e.knowledge_item_id,e.passage_id LIMIT ?3"
            ))
            .map_err(storage_error)?;
        let mut last = (0_i64, 0_i64);
        loop {
            let mut rows = statement
                .query(params![last.0, last.1, PAGE_SIZE])
                .map_err(storage_error)?;
            let mut count = 0;
            while let Some(row) = rows.next().map_err(storage_error)? {
                last = (
                    row.get(0).map_err(storage_error)?,
                    row.get(1).map_err(storage_error)?,
                );
                emit(SnapshotRecord::Evidence {
                    knowledge: last.0,
                    passage: last.1,
                })?;
                count += 1;
            }
            if count < PAGE_SIZE {
                break;
            }
        }
        Ok(())
    }

    fn pages(
        &self,
        sql: &str,
        decode: impl Fn(&Row<'_>) -> rusqlite::Result<SnapshotRecord>,
        emit: &mut impl FnMut(SnapshotRecord) -> Result<()>,
    ) -> Result<()> {
        let mut statement = self
            .connection
            .prepare(&format!("{ACTIVE} {sql}"))
            .map_err(storage_error)?;
        let mut last = 0_i64;
        loop {
            let mut rows = statement
                .query(params![last, PAGE_SIZE])
                .map_err(storage_error)?;
            let mut count = 0;
            while let Some(row) = rows.next().map_err(storage_error)? {
                last = row.get(0).map_err(storage_error)?;
                emit(decode(row).map_err(storage_error)?)?;
                count += 1;
            }
            if count < PAGE_SIZE {
                break;
            }
        }
        Ok(())
    }
}
