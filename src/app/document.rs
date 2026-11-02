use std::path::Path;

use serde::Serialize;

use crate::domain::documents::DocumentListItem;
use crate::domain::ids::DocumentId;
use crate::storage::{database::SqliteDatabase, documents};
use crate::{CommonplaceError, Result};

#[derive(Debug, Serialize)]
pub struct DocumentList {
    pub items: Vec<DocumentListItem>,
    pub truncated: bool,
    pub next_after: Option<DocumentId>,
}

pub fn list(root: &Path, after: Option<&str>, limit: usize) -> Result<DocumentList> {
    if !(1..=1000).contains(&limit) {
        return Err(CommonplaceError::LimitExceeded(
            "document list --limit must be between 1 and 1000".into(),
        ));
    }
    let after = after.map(str::parse::<DocumentId>).transpose()?;
    let session = SqliteDatabase::read(root)?;
    let mut ids = documents::list(session.connection(), after, limit + 1)?;
    let truncated = ids.len() > limit;
    ids.truncate(limit);
    let next_after = if truncated {
        ids.last().map(|item| item.document_id)
    } else {
        None
    };
    Ok(DocumentList {
        items: ids,
        truncated,
        next_after,
    })
}
