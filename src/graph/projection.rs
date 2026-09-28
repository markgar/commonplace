use std::path::Path;

use oxigraph::model::{GraphName, Literal, NamedNode, Quad, Term, vocab::xsd};
use oxigraph::store::Store;

use super::graph_error;
use super::schema::{METADATA, Property as P, Resource as R, STORE};
use crate::Result;
use crate::domain::knowledge::{FactObject, LiteralValue};
use crate::domain::schema::ObjectKind;
use crate::storage::graph_snapshot::{GraphSnapshot, SnapshotRecord};

const BATCH_SIZE: usize = 512;

pub(crate) fn metadata(version: i64) -> Quad {
    Quad::new(
        STORE,
        P::KnowledgeVersion.node(),
        Literal::from(version),
        METADATA,
    )
}

pub(crate) fn open_verified(path: &Path, expected: i64) -> Result<Store> {
    if !path.is_dir() {
        return Err(graph_error(format!(
            "missing graph directory {}",
            path.display()
        )));
    }
    let store = Store::open_read_only(path).map_err(graph_error)?;
    verify_version(&store, expected)?;
    Ok(store)
}

fn verify_version(store: &Store, expected: i64) -> Result<()> {
    let predicate = P::KnowledgeVersion.node();
    let mut quads = store.quads_for_pattern(
        Some(STORE.into()),
        Some(predicate.as_ref()),
        None,
        Some(METADATA.into()),
    );
    let quad = quads
        .next()
        .transpose()
        .map_err(graph_error)?
        .ok_or_else(|| graph_error("missing graph knowledge_version"))?;
    if quads.next().transpose().map_err(graph_error)?.is_some() {
        return Err(graph_error("multiple graph knowledge_version values"));
    }
    let Term::Literal(value) = quad.object else {
        return Err(graph_error("graph knowledge_version is not a literal"));
    };
    let version = value
        .value()
        .parse::<i64>()
        .map_err(|_| graph_error("malformed graph knowledge_version"))?;
    if value.datatype() != xsd::INTEGER || version < 0 || version != expected {
        return Err(graph_error(format!(
            "invalid graph knowledge_version; expected {expected}"
        )));
    }
    Ok(())
}

pub(crate) fn build(snapshot: &GraphSnapshot<'_>, path: &Path) -> Result<()> {
    // A candidate must be new, never a writable open of any existing generation.
    std::fs::create_dir(path).map_err(graph_error)?;
    let store = Store::open(path).map_err(graph_error)?;
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    let mut count = 0_usize;
    project(snapshot, |quad| {
        batch.push(quad);
        count = count
            .checked_add(1)
            .ok_or_else(|| graph_error("graph cardinality overflow"))?;
        if batch.len() == BATCH_SIZE {
            store.extend(batch.drain(..)).map_err(graph_error)?;
        }
        Ok(())
    })?;
    store.extend(batch).map_err(graph_error)?;
    store.flush().map_err(graph_error)?;
    drop(store);
    let store = open_verified(path, snapshot.knowledge_version)?;
    if store.len().map_err(graph_error)? != count {
        return Err(graph_error(
            "candidate graph cardinality differs from SQLite projection",
        ));
    }
    project(snapshot, |quad| {
        if !store.contains(&quad).map_err(graph_error)? {
            return Err(graph_error(
                "candidate graph content differs from SQLite projection",
            ));
        }
        Ok(())
    })?;
    // All verification iterators and the read-only handle close before activation.
    drop(store);
    Ok(())
}

fn project(snapshot: &GraphSnapshot<'_>, mut emit: impl FnMut(Quad) -> Result<()>) -> Result<()> {
    emit(metadata(snapshot.knowledge_version))?;
    snapshot.visit(|record| {
        let (resource, id) = match &record {
            SnapshotRecord::Membership { id, .. } | SnapshotRecord::Fact { id, .. } => {
                (R::Knowledge, *id)
            }
            SnapshotRecord::Predicate { id, .. } => (R::Predicate, *id),
            SnapshotRecord::Entity { id, .. } => (R::Entity, *id),
            SnapshotRecord::EntityType { id, .. } => (R::EntityType, *id),
            SnapshotRecord::Passage { id, .. } => (R::Passage, *id),
            SnapshotRecord::Revision { id, .. } => (R::Revision, *id),
            SnapshotRecord::Document { id, .. } => (R::Document, *id),
            SnapshotRecord::Evidence { knowledge, passage } => {
                emit(quad(
                    &R::Knowledge.node(*knowledge)?,
                    P::Evidence,
                    R::Passage.node(*passage)?,
                ))?;
                return Ok(());
            }
        };
        let subject = resource.node(id)?;
        emit(quad(
            &subject,
            P::Id,
            Literal::new_simple_literal(resource.tagged_id(id)?),
        ))?;
        let mut put = |property, object: Term| emit(quad(&subject, property, object));
        match record {
            SnapshotRecord::Membership {
                schema_version,
                entity,
                entity_type,
                ..
            } => {
                put(
                    P::Kind,
                    Literal::new_simple_literal("type_membership").into(),
                )?;
                put(P::SchemaVersion, Literal::from(schema_version).into())?;
                put(P::Subject, R::Entity.node(entity)?.into())?;
                put(P::EntityType, R::EntityType.node(entity_type)?.into())?;
            }
            SnapshotRecord::Entity { name, .. } | SnapshotRecord::EntityType { name, .. } => {
                put(P::Name, Literal::new_simple_literal(name).into())?;
            }
            SnapshotRecord::Predicate {
                name, object_kind, ..
            } => {
                put(P::Name, Literal::new_simple_literal(name).into())?;
                put(
                    P::ObjectKind,
                    Literal::new_simple_literal(object_kind).into(),
                )?;
            }
            SnapshotRecord::Fact {
                schema_version,
                subject,
                predicate,
                object,
                ..
            } => {
                put(P::Kind, Literal::new_simple_literal("fact").into())?;
                put(P::SchemaVersion, Literal::from(schema_version).into())?;
                put(P::Subject, R::Entity.node(subject)?.into())?;
                put(P::Predicate, R::Predicate.node(predicate)?.into())?;
                let object = match object {
                    FactObject::Entity { entity_id } => R::Entity.node(entity_id.value())?.into(),
                    FactObject::Literal(value) => {
                        put(
                            P::LiteralKind,
                            Literal::new_simple_literal(value.literal_kind.as_str()).into(),
                        )?;
                        put(
                            P::LiteralJson,
                            Literal::new_simple_literal(value.literal_json).into(),
                        )?;
                        let literal = match value.literal {
                            LiteralValue::String(text)
                                if value.literal_kind == ObjectKind::Timestamp =>
                            {
                                Literal::new_typed_literal(text, xsd::DATE_TIME)
                            }
                            LiteralValue::String(text) => Literal::new_simple_literal(text),
                            LiteralValue::Integer(value) => Literal::from(value),
                            LiteralValue::Boolean(value) => Literal::from(value),
                        };
                        literal.into()
                    }
                };
                put(P::Object, object)?;
            }
            SnapshotRecord::Passage {
                revision,
                ordinal,
                start,
                end,
                text,
                ..
            } => {
                put(P::Revision, R::Revision.node(revision)?.into())?;
                put(P::Ordinal, Literal::from(ordinal).into())?;
                put(P::StartByte, Literal::from(start).into())?;
                put(P::EndByte, Literal::from(end).into())?;
                put(P::Text, Literal::new_simple_literal(text).into())?;
            }
            SnapshotRecord::Revision {
                document,
                number,
                digest,
                source_type,
                metadata,
                title,
                occurred_at,
                ..
            } => {
                put(P::Document, R::Document.node(document)?.into())?;
                put(P::RevisionNumber, Literal::from(number).into())?;
                put(
                    P::RevisionDigest,
                    Literal::new_simple_literal(digest).into(),
                )?;
                put(
                    P::SourceType,
                    Literal::new_simple_literal(source_type).into(),
                )?;
                put(
                    P::MetadataJson,
                    Literal::new_simple_literal(metadata).into(),
                )?;
                if let Some(title) = title {
                    put(P::Title, Literal::new_simple_literal(title).into())?;
                }
                if let Some(time) = occurred_at {
                    put(
                        P::OccurredAt,
                        Literal::new_typed_literal(time, xsd::DATE_TIME).into(),
                    )?;
                }
            }
            SnapshotRecord::Document { source_key, .. } => {
                put(P::SourceKey, Literal::new_simple_literal(source_key).into())?;
            }
            SnapshotRecord::Evidence { .. } => unreachable!("evidence handled separately"),
        }
        Ok(())
    })
}

fn quad(subject: &NamedNode, property: P, object: impl Into<Term>) -> Quad {
    Quad::new(
        subject.clone(),
        property.node(),
        object,
        GraphName::DefaultGraph,
    )
}
