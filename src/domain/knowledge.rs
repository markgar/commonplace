use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::documents::Evidence;
use super::ids::{EntityId, EntityTypeId, IdentifierSchemeId, KnowledgeItemId};
use super::schema::Name;
use crate::{CommonplaceError, Result};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct IdentityText(
    #[schemars(length(min = 1, max = 1024), regex(pattern = "^[^\\u0000]+$"))] pub String,
);

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct LocalRef(#[schemars(regex(pattern = "^[A-Za-z][A-Za-z0-9_]{0,63}$"))] pub String);

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Identifier {
    pub scheme: Name,
    pub value: IdentityText,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum EntityReference {
    Id {
        #[schemars(regex(pattern = "^entity:[1-9][0-9]*$"))]
        id: String,
    },
    Identifier {
        identifier: Identifier,
    },
    Name {
        name: IdentityText,
    },
    Ref {
        r#ref: LocalRef,
    },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordItem {
    Entity {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#ref: Option<LocalRef>,
        name: IdentityText,
        #[serde(default)]
        #[schemars(length(max = 1000))]
        aliases: Vec<IdentityText>,
        #[serde(default)]
        #[schemars(length(max = 1000))]
        identifiers: Vec<Identifier>,
    },
    EntityMetadata {
        #[schemars(regex(pattern = "^entity:[1-9][0-9]*$"))]
        entity_id: String,
        #[serde(default)]
        #[schemars(length(max = 1000))]
        add_aliases: Vec<IdentityText>,
        #[serde(default)]
        #[schemars(length(max = 1000))]
        remove_aliases: Vec<IdentityText>,
        #[serde(default)]
        #[schemars(length(max = 1000))]
        add_identifiers: Vec<Identifier>,
        #[serde(default)]
        #[schemars(length(max = 1000))]
        remove_identifiers: Vec<Identifier>,
    },
    TypeMembership {
        entity: EntityReference,
        entity_type: Name,
        #[serde(default)]
        #[schemars(length(max = 1000))]
        support: Vec<super::evidence::SupportInput>,
    },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String", length(min = 1, max = 128))]
    pub created_by: Option<String>,
    #[schemars(length(max = 1000))]
    pub items: Vec<RecordItem>,
}

pub fn input_schema() -> schemars::Schema {
    schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<RecordInput>()
}

impl RecordInput {
    pub fn validate(&self) -> Result<()> {
        let validator = jsonschema::validator_for(input_schema().as_value())
            .map_err(|error| CommonplaceError::Storage(error.to_string()))?;
        validator
            .validate(&serde_json::to_value(self)?)
            .map_err(|error| {
                CommonplaceError::InvalidInput(format!("{}: {error}", error.instance_path))
            })
    }
}

#[derive(Debug, Serialize)]
pub struct Alias {
    pub alias: String,
    pub created_at: String,
    pub created_by: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EntityIdentifier {
    pub identifier_scheme_id: IdentifierSchemeId,
    pub scheme: String,
    pub value: String,
    pub created_at: String,
    pub created_by: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Entity {
    pub entity_id: EntityId,
    pub canonical_name: String,
    pub created_at: String,
    pub created_by: Option<String>,
    pub aliases: Vec<Alias>,
    pub identifiers: Vec<EntityIdentifier>,
    pub active_type_ids: Vec<EntityTypeId>,
    pub active_type_membership_ids: Vec<KnowledgeItemId>,
    pub active_fact_ids: Vec<KnowledgeItemId>,
}

#[derive(Debug, Serialize)]
pub struct Knowledge {
    pub knowledge_id: KnowledgeItemId,
    pub subtype: &'static str,
    pub schema_version: i64,
    pub created_at: String,
    pub created_by: Option<String>,
    pub withdrawn_at: Option<String>,
    pub withdrawn_by: Option<String>,
    pub entity_id: EntityId,
    pub entity_type_id: EntityTypeId,
    pub support: Vec<Evidence>,
}
