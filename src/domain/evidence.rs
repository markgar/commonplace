use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::documents::Evidence;
use crate::{CommonplaceError, Result};

/// A whole canonical passage. Assertions never select arbitrary source snippets.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = paired_offsets)]
pub struct SupportInput {
    #[schemars(regex(pattern = "^passage:[1-9][0-9]*$"))]
    pub passage_id: String,
    /// If supplied, must equal the entire stored passage text, without normalization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub quote: Option<String>,
    /// Zero-based UTF-8 byte offset in the immutable revision, not the passage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "usize")]
    pub start_byte: Option<usize>,
    /// Exclusive revision byte offset; supply both offsets or neither.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "usize")]
    pub end_byte: Option<usize>,
}

fn paired_offsets(schema: &mut schemars::Schema) {
    schema.insert(
        "dependentRequired".into(),
        serde_json::json!({"start_byte":["end_byte"],"end_byte":["start_byte"]}),
    );
}

impl SupportInput {
    pub fn validate(&self, evidence: &Evidence) -> Result<()> {
        if self
            .quote
            .as_ref()
            .is_some_and(|quote| quote != &evidence.text)
            || self.start_byte.is_some() != self.end_byte.is_some()
            || self
                .start_byte
                .is_some_and(|start| start != evidence.start_byte)
            || self.end_byte.is_some_and(|end| end != evidence.end_byte)
        {
            return Err(CommonplaceError::InvalidInput(format!(
                "support {} must match the entire canonical passage text and revision byte range",
                self.passage_id
            )));
        }
        Ok(())
    }
}
