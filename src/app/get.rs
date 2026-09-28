use std::path::Path;

use serde::Serialize;

use crate::domain::documents::{Document, DocumentRevision, Evidence};
use crate::domain::ids::{DocumentId, PassageId, RevisionId};
use crate::storage::{database::SqliteDatabase, evidence};
use crate::{CommonplaceError, Result};

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    Document(Document),
    Revision(DocumentRevision),
    Passage(Evidence),
}

pub fn get(root: &Path, id: &str) -> Result<Record> {
    enum Selector {
        Document(DocumentId),
        Revision(RevisionId),
        Passage(PassageId),
    }
    let selector = match id.split_once(':').map(|(tag, _)| tag) {
        Some("doc") => Selector::Document(id.parse()?),
        Some("revision") => Selector::Revision(id.parse()?),
        Some("passage") => Selector::Passage(id.parse()?),
        _ => {
            return Err(CommonplaceError::InvalidInput(
                "get currently supports doc:<id>, revision:<id>, and passage:<id> only".into(),
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
    }
}
