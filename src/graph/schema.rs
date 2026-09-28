use oxigraph::model::{NamedNode, NamedNodeRef};
use serde::Serialize;
use serde_json::{Value, json};

use crate::domain::ids::*;
use crate::{CommonplaceError, Result};

pub(crate) const METADATA: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("urn:commonplace:metadata");
pub(crate) const STORE: NamedNodeRef<'_> = NamedNodeRef::new_unchecked("urn:commonplace:store");
pub(crate) const PROPERTY_NAMESPACE: &str = "urn:commonplace:property:";
pub(crate) const XSD: &str = "http://www.w3.org/2001/XMLSchema#";

#[derive(Clone, Copy)]
pub(crate) enum Resource {
    Document,
    Revision,
    Passage,
    Entity,
    Knowledge,
    EntityType,
    Predicate,
}

impl Resource {
    fn prefix(self) -> &'static str {
        match self {
            Self::Document => "doc",
            Self::Revision => "revision",
            Self::Passage => "passage",
            Self::Entity => "entity",
            Self::Knowledge => "knowledge",
            Self::EntityType => "type",
            Self::Predicate => "predicate",
        }
    }

    pub(crate) fn tagged_id(self, id: i64) -> Result<String> {
        let result = match self {
            Self::Document => DocumentId::new(id).map(|id| id.to_string()),
            Self::Revision => RevisionId::new(id).map(|id| id.to_string()),
            Self::Passage => PassageId::new(id).map(|id| id.to_string()),
            Self::Entity => EntityId::new(id).map(|id| id.to_string()),
            Self::Knowledge => KnowledgeItemId::new(id).map(|id| id.to_string()),
            Self::EntityType => EntityTypeId::new(id).map(|id| id.to_string()),
            Self::Predicate => PredicateId::new(id).map(|id| id.to_string()),
        };
        result.map_err(|error| CommonplaceError::Graph(format!("invalid snapshot ID: {error}")))
    }

    pub(crate) fn node(self, id: i64) -> Result<NamedNode> {
        self.tagged_id(id)?;
        Ok(NamedNode::new_unchecked(format!(
            "urn:commonplace:{}:{id}",
            self.prefix()
        )))
    }
}

macro_rules! properties {
    ($($variant:ident => ($name:literal, $datatype:literal)),+ $(,)?) => {
        #[derive(Clone, Copy)]
        pub(crate) enum Property { $($variant),+ }
        impl Property {
            pub(crate) fn name(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }
            pub(crate) fn node(self) -> NamedNode {
                NamedNode::new_unchecked(format!("{PROPERTY_NAMESPACE}{}", self.name()))
            }
        }
        fn properties() -> Vec<Value> {
            vec![$(json!({"name": $name, "iri": Property::$variant.node().as_str(),
                "value": $datatype})),+]
        }
    };
}

properties! {
    Id => ("id", "xsd:string"),
    Name => ("name", "xsd:string"),
    Kind => ("kind", "xsd:string"),
    SchemaVersion => ("schema_version", "xsd:integer"),
    Subject => ("subject", "entity IRI"),
    EntityType => ("entity_type", "entity type IRI"),
    Evidence => ("evidence", "passage IRI"),
    Revision => ("revision", "revision IRI"),
    Ordinal => ("ordinal", "xsd:integer"),
    StartByte => ("start_byte", "xsd:integer"),
    EndByte => ("end_byte", "xsd:integer"),
    Text => ("text", "xsd:string"),
    Document => ("document", "document IRI"),
    RevisionNumber => ("revision_number", "xsd:integer"),
    RevisionDigest => ("revision_digest", "xsd:string"),
    SourceType => ("source_type", "xsd:string"),
    MetadataJson => ("metadata_json", "xsd:string"),
    Title => ("title", "xsd:string"),
    OccurredAt => ("occurred_at", "xsd:dateTime"),
    SourceKey => ("source_key", "xsd:string"),
    KnowledgeVersion => ("knowledge_version", "xsd:integer"),
    Predicate => ("predicate", "predicate IRI"),
    Object => ("object", "entity IRI or typed literal"),
    ObjectKind => ("object_kind", "xsd:string"),
    LiteralKind => ("literal_kind", "xsd:string"),
    LiteralJson => ("literal_json", "xsd:string"),
}

#[derive(Debug, Serialize)]
pub struct GraphSchema {
    rdf: &'static str,
    namespaces: Value,
    metadata: Value,
    resources: Vec<Value>,
    properties: Vec<Value>,
    projection: Value,
    examples: Vec<&'static str>,
}

pub fn describe() -> GraphSchema {
    let resources = [
        (Resource::Document, "document", vec!["id", "source_key"]),
        (
            Resource::Revision,
            "revision",
            vec![
                "id",
                "document",
                "revision_number",
                "revision_digest",
                "source_type",
                "metadata_json",
                "title",
                "occurred_at",
            ],
        ),
        (
            Resource::Passage,
            "passage",
            vec![
                "id",
                "revision",
                "ordinal",
                "start_byte",
                "end_byte",
                "text",
            ],
        ),
        (Resource::Entity, "entity", vec!["id", "name"]),
        (
            Resource::Knowledge,
            "knowledge",
            vec![
                "id",
                "kind",
                "schema_version",
                "subject",
                "entity_type",
                "evidence",
            ],
        ),
        (Resource::EntityType, "entity_type", vec!["id", "name"]),
        (
            Resource::Predicate,
            "predicate",
            vec!["id", "name", "object_kind"],
        ),
    ]
    .into_iter()
    .map(|(resource, name, properties)| {
        let tagged = resource.tagged_id(1).expect("positive example ID");
        json!({
            "resource": name,
            "iri_pattern": format!("urn:commonplace:{}:N", resource.prefix()),
            "id_pattern": format!("{}:N", tagged.split_once(':').expect("tagged ID").0),
            "properties": properties,
        })
    })
    .collect();
    GraphSchema {
        rdf: "1.1",
        namespaces: json!({"c": PROPERTY_NAMESPACE, "xsd": XSD, "resources": "urn:commonplace:"}),
        metadata: json!({"graph": METADATA.as_str(), "subject": STORE.as_str(),
            "predicate": Property::KnowledgeVersion.node().as_str(), "datatype": format!("{XSD}integer"),
            "cardinality": 1, "range": "nonnegative signed i64", "must_equal": "SQLite knowledge_version"}),
        resources,
        properties: properties(),
        projection: json!({
            "graph": "default",
            "supported_knowledge_kinds": ["type_membership"],
            "unsupported_active_facts": "error; fact projection and authoring are delivered by P6",
            "fact_mapping": {"properties": ["id", "kind", "schema_version", "subject", "predicate", "object", "evidence"],
                "literal_properties": ["literal_kind", "literal_json"],
                "datatypes": {"string":"xsd:string", "integer":"xsd:integer", "boolean":"xsd:boolean", "timestamp":"xsd:dateTime"}},
            "optional_properties": ["title", "occurred_at"],
            "multivalued_properties": ["evidence"],
            "selection": "active items and only their referenced entities, vocabulary and cited passages/revisions/documents",
            "ids": "N is the positive decimal SQLite primary key without leading zeros; no blank-node canonical resources",
            "evidence": "zero or more exact passages; byte offsets are zero-based half-open UTF-8; metadata belongs to the cited revision",
            "excluded": ["full revision text", "aliases", "identifiers", "vocabulary descriptions", "endpoint constraints"],
            "duplicate_items": "distinct knowledge IRIs; no shortcut triples or synthetic value nodes"
        }),
        examples: vec![
            "PREFIX c: <urn:commonplace:property:> SELECT ?knowledge ?entity ?type WHERE { ?knowledge c:kind \"type_membership\"; c:subject ?entity; c:entity_type ?type } ORDER BY ?knowledge",
            "PREFIX c: <urn:commonplace:property:> SELECT ?knowledge ?passage ?revision ?document ?start ?end ?quote WHERE { ?knowledge c:evidence ?passage . ?passage c:revision ?revision; c:start_byte ?start; c:end_byte ?end; c:text ?quote . ?revision c:document ?document } ORDER BY ?knowledge ?passage",
            "SELECT ?version WHERE { GRAPH <urn:commonplace:metadata> { <urn:commonplace:store> <urn:commonplace:property:knowledge_version> ?version } }",
        ],
    }
}
