use std::path::Path;

use serde::Serialize;

use crate::domain::documents::{Document, DocumentRevision, Evidence};
use crate::domain::ids::{DocumentId, EntityId, KnowledgeItemId, PassageId, RevisionId};
use crate::domain::knowledge::{Entity, Knowledge};
use crate::storage::{database::SqliteDatabase, evidence, knowledge};
use crate::{CommonplaceError, Result};

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    Document(Document),
    Revision(DocumentRevision),
    Passage(Evidence),
    Entity(Entity),
    Knowledge(Knowledge),
}

pub fn get(root: &Path, id: &str) -> Result<Record> {
    enum Selector {
        Document(DocumentId),
        Revision(RevisionId),
        Passage(PassageId),
        Entity(EntityId),
        Knowledge(KnowledgeItemId),
    }
    let selector = match id.split_once(':').map(|(tag, _)| tag) {
        Some("doc") => Selector::Document(id.parse()?),
        Some("revision") => Selector::Revision(id.parse()?),
        Some("passage") => Selector::Passage(id.parse()?),
        Some("entity") => Selector::Entity(id.parse()?),
        Some("knowledge") => Selector::Knowledge(id.parse()?),
        _ => {
            return Err(CommonplaceError::InvalidInput(
                "get supports doc:<id>, revision:<id>, passage:<id>, entity:<id>, and knowledge:<id>".into(),
            ));
        }
    };
    let session = SqliteDatabase::read(root)?;
    match selector {
        Selector::Document(id) => {
            evidence::document(session.connection(), id).map(Record::Document)
        }
        Selector::Revision(id) => {
            evidence::revision(session.connection(), id).map(Record::Revision)
        }
        Selector::Passage(id) => evidence::passage(session.connection(), id).map(Record::Passage),
        Selector::Entity(id) => knowledge::entity(session.connection(), id).map(Record::Entity),
        Selector::Knowledge(id) => {
            knowledge::knowledge(session.connection(), id).map(Record::Knowledge)
        }
    }
}
