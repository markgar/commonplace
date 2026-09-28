use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::{DocumentId, KnowledgeItemId};
use crate::{CommonplaceError, Result};

/// An exact stored key supplied through --source-key, not a JSON input mode.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveInput {
    #[schemars(length(min = 1), regex(pattern = "^[^\\u0000]+$"))]
    pub source_key: String,
}

impl RemoveInput {
    pub fn validate(&self) -> Result<()> {
        if self.source_key.is_empty() || self.source_key.contains('\0') {
            return Err(CommonplaceError::InvalidInput(
                "source_key must be nonempty and contain no NUL".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct RemovedSource {
    pub source_key: String,
    pub document_id: DocumentId,
    pub deleted_revisions: usize,
    pub deleted_passages: usize,
    pub detached_evidence: usize,
    pub affected_knowledge_ids: Vec<KnowledgeItemId>,
}
