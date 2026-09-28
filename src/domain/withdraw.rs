use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::KnowledgeItemId;
use crate::{CommonplaceError, Result};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct KnowledgeIdInput(#[schemars(regex(pattern = "^knowledge:[1-9][0-9]*$"))] pub String);

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WithdrawInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", length(min = 1, max = 128))]
    pub withdrawn_by: Option<String>,
    #[schemars(length(min = 1, max = 1000))]
    pub knowledge_ids: Vec<KnowledgeIdInput>,
}

pub fn input_schema() -> schemars::Schema {
    schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<WithdrawInput>()
}

impl WithdrawInput {
    pub fn validate(&self) -> Result<Vec<KnowledgeItemId>> {
        let validator = jsonschema::validator_for(input_schema().as_value())
            .map_err(|error| CommonplaceError::Storage(error.to_string()))?;
        validator
            .validate(&serde_json::to_value(self)?)
            .map_err(|error| {
                CommonplaceError::InvalidInput(format!("{}: {error}", error.instance_path))
            })?;
        let mut seen = BTreeSet::new();
        self.knowledge_ids
            .iter()
            .enumerate()
            .map(|(index, input)| {
                let id = input.0.parse::<KnowledgeItemId>().map_err(|error| {
                    CommonplaceError::InvalidInput(format!("knowledge_ids[{index}]: {error}"))
                })?;
                if !seen.insert(id) {
                    return Err(CommonplaceError::InvalidInput(format!(
                        "knowledge_ids[{index}]: duplicate {id}; include each ID only once"
                    )));
                }
                Ok(id)
            })
            .collect()
    }
}
