use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::{EntityTypeId, IdentifierSchemeId, PredicateId};
use crate::{CommonplaceError, Result};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Name(#[schemars(regex(pattern = "^[a-z][a-z0-9_]*$"))] pub String);

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TermInput {
    pub name: Name,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Entity,
    String,
    Integer,
    Boolean,
    Timestamp,
}

impl ObjectKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Entity => "entity",
            Self::String => "string",
            Self::Integer => "integer",
            Self::Boolean => "boolean",
            Self::Timestamp => "timestamp",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = predicate_endpoints)]
pub struct PredicateInput {
    pub name: Name,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub description: Option<String>,
    pub object_kind: ObjectKind,
    #[schemars(length(min = 1))]
    pub subject_types: Vec<Name>,
    #[serde(default)]
    pub object_types: Vec<Name>,
}

fn predicate_endpoints(schema: &mut schemars::Schema) {
    schema.insert(
        "if".into(),
        serde_json::json!({"properties":{"object_kind":{"const":"entity"}}}),
    );
    schema.insert(
        "then".into(),
        serde_json::json!({"required":["object_types"],"properties":{"object_types":{"minItems":1}}}),
    );
    schema.insert(
        "else".into(),
        serde_json::json!({"properties":{"object_types":{"maxItems":0}}}),
    );
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SchemaInput {
    #[serde(default)]
    pub entity_types: Vec<TermInput>,
    #[serde(default)]
    pub identifier_schemes: Vec<TermInput>,
    #[serde(default)]
    pub predicates: Vec<PredicateInput>,
}

#[derive(Debug, Serialize)]
pub struct Term<Id> {
    pub id: Id,
    pub name: Name,
    pub description: Option<String>,
    pub introduced_version: i64,
}

#[derive(Debug, Serialize)]
pub struct Predicate {
    #[serde(flatten)]
    pub term: Term<PredicateId>,
    pub object_kind: ObjectKind,
    pub subject_types: Vec<Name>,
    pub object_types: Vec<Name>,
}

#[derive(Debug, Serialize)]
pub struct Vocabulary {
    pub schema_version: i64,
    pub entity_types: Vec<Term<EntityTypeId>>,
    pub identifier_schemes: Vec<Term<IdentifierSchemeId>>,
    pub predicates: Vec<Predicate>,
}

#[derive(Debug)]
pub struct EndpointAddition {
    pub predicate: Name,
    pub role: &'static str,
    pub entity_type: Name,
}

#[derive(Debug, Default, Serialize)]
pub struct Additions {
    pub entity_types: usize,
    pub identifier_schemes: usize,
    pub predicates: usize,
    pub endpoints: usize,
}

#[derive(Debug)]
pub struct SchemaPlan {
    pub schema_version: i64,
    pub entity_types: Vec<TermInput>,
    pub identifier_schemes: Vec<TermInput>,
    pub predicates: Vec<PredicateInput>,
    pub endpoints: Vec<EndpointAddition>,
}

impl SchemaPlan {
    pub fn changed(&self) -> bool {
        !self.entity_types.is_empty()
            || !self.identifier_schemes.is_empty()
            || !self.predicates.is_empty()
            || !self.endpoints.is_empty()
    }

    pub fn summary(&self) -> Additions {
        Additions {
            entity_types: self.entity_types.len(),
            identifier_schemes: self.identifier_schemes.len(),
            predicates: self.predicates.len(),
            endpoints: self.endpoints.len(),
        }
    }
}

pub fn plan(input: SchemaInput, current: &Vocabulary) -> Result<SchemaPlan> {
    let entity_types = added_terms(input.entity_types, &current.entity_types)?;
    let identifier_schemes = added_terms(input.identifier_schemes, &current.identifier_schemes)?;
    let known_types: BTreeSet<_> = current
        .entity_types
        .iter()
        .map(|term| &term.name)
        .chain(entity_types.iter().map(|term| &term.name))
        .collect();
    let known_predicates: BTreeMap<_, _> = current
        .predicates
        .iter()
        .map(|predicate| (&predicate.term.name, predicate))
        .collect();
    let mut seen = BTreeSet::new();
    let mut predicates = Vec::new();
    let mut endpoints = Vec::new();
    for predicate in input.predicates {
        if !seen.insert(predicate.name.clone()) {
            return Err(invalid(format!(
                "duplicate predicate {:?}",
                predicate.name.0
            )));
        }
        let existing = known_predicates.get(&predicate.name);
        if let Some(existing) = existing {
            check_description(
                &predicate.name,
                &predicate.description,
                &existing.term.description,
            )?;
            if predicate.object_kind != existing.object_kind {
                return Err(invalid(format!(
                    "cannot redefine predicate {:?}",
                    predicate.name.0
                )));
            }
        }
        for (role, types) in [
            ("subject", &predicate.subject_types),
            ("object", &predicate.object_types),
        ] {
            let mut seen_types = BTreeSet::new();
            for name in types {
                if !seen_types.insert(name) {
                    return Err(invalid(format!("duplicate {role} endpoint {:?}", name.0)));
                }
                if !known_types.contains(name) {
                    return Err(invalid(format!(
                        "unknown endpoint entity type {:?}",
                        name.0
                    )));
                }
                let present = existing.is_some_and(|existing| {
                    let types = if role == "subject" {
                        &existing.subject_types
                    } else {
                        &existing.object_types
                    };
                    types.contains(name)
                });
                if !present {
                    endpoints.push(EndpointAddition {
                        predicate: predicate.name.clone(),
                        role,
                        entity_type: name.clone(),
                    });
                }
            }
        }
        if existing.is_none() {
            predicates.push(predicate);
        }
    }
    let mut plan = SchemaPlan {
        schema_version: current.schema_version,
        entity_types,
        identifier_schemes,
        predicates,
        endpoints,
    };
    if plan.changed() {
        plan.schema_version = plan.schema_version.checked_add(1).ok_or_else(|| {
            CommonplaceError::Conflict("schema version counter is exhausted".into())
        })?;
    }
    Ok(plan)
}

fn added_terms<Id>(input: Vec<TermInput>, current: &[Term<Id>]) -> Result<Vec<TermInput>> {
    let known: BTreeMap<_, _> = current.iter().map(|term| (&term.name, term)).collect();
    let mut seen = BTreeSet::new();
    let mut additions = Vec::new();
    for term in input {
        if !seen.insert(term.name.clone()) {
            return Err(invalid(format!("duplicate term {:?}", term.name.0)));
        }
        match known.get(&term.name) {
            Some(existing) => {
                check_description(&term.name, &term.description, &existing.description)?;
            }
            None => additions.push(term),
        }
    }
    Ok(additions)
}

fn check_description(
    name: &Name,
    requested: &Option<String>,
    existing: &Option<String>,
) -> Result<()> {
    if requested.is_some() && requested != existing {
        return Err(invalid(format!(
            "cannot redefine description of {:?}",
            name.0
        )));
    }
    Ok(())
}

fn invalid(message: String) -> CommonplaceError {
    CommonplaceError::InvalidInput(message)
}
