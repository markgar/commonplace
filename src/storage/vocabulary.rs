use rusqlite::{Connection, params};

use crate::domain::ids::{EntityTypeId, IdentifierSchemeId, PredicateId};
use crate::domain::schema::{Name, ObjectKind, Predicate, SchemaPlan, Term, Vocabulary};
use crate::{CommonplaceError, Result};

use super::database::storage_error;

pub fn read(connection: &Connection) -> Result<Vocabulary> {
    let schema_version = connection
        .query_row(
            "SELECT schema_version FROM store_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let entity_types = read_terms(
        connection,
        "entity_types",
        "entity_type_id",
        EntityTypeId::stored,
    )?;
    let identifier_schemes = read_terms(
        connection,
        "identifier_schemes",
        "identifier_scheme_id",
        IdentifierSchemeId::stored,
    )?;
    let terms = read_terms(
        connection,
        "predicates",
        "predicate_id",
        PredicateId::stored,
    )?;
    let mut predicates = Vec::new();
    for term in terms {
        let kind: String = connection
            .query_row(
                "SELECT object_kind FROM predicates WHERE predicate_id = ?1",
                [term.id.value()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let object_kind = match kind.as_str() {
            "entity" => ObjectKind::Entity,
            "string" => ObjectKind::String,
            "integer" => ObjectKind::Integer,
            "boolean" => ObjectKind::Boolean,
            "timestamp" => ObjectKind::Timestamp,
            _ => {
                return Err(CommonplaceError::Storage(format!(
                    "invalid stored predicate kind {kind:?}"
                )));
            }
        };
        let subject_types = read_endpoints(connection, term.id, "subject")?;
        let object_types = read_endpoints(connection, term.id, "object")?;
        predicates.push(Predicate {
            term,
            object_kind,
            subject_types,
            object_types,
        });
    }
    Ok(Vocabulary {
        schema_version,
        entity_types,
        identifier_schemes,
        predicates,
    })
}

fn read_terms<Id>(
    connection: &Connection,
    table: &str,
    id: &str,
    make_id: fn(i64) -> Result<Id>,
) -> Result<Vec<Term<Id>>> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT {id}, name, description, introduced_version FROM {table} ORDER BY name"
        ))
        .map_err(storage_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(storage_error)?;
    rows.map(|row| {
        let (id, name, description, introduced_version) = row.map_err(storage_error)?;
        Ok(Term {
            id: make_id(id)?,
            name: Name(name),
            description,
            introduced_version,
        })
    })
    .collect()
}

fn read_endpoints(
    connection: &Connection,
    predicate: PredicateId,
    role: &str,
) -> Result<Vec<Name>> {
    let mut statement = connection
        .prepare(
            "SELECT et.name FROM predicate_entity_types pe
         JOIN entity_types et USING (entity_type_id)
         WHERE pe.predicate_id = ?1 AND pe.role = ?2 ORDER BY et.name",
        )
        .map_err(storage_error)?;
    statement
        .query_map(params![predicate.value(), role], |row| {
            row.get::<_, String>(0)
        })
        .map_err(storage_error)?
        .map(|row| row.map(Name).map_err(storage_error))
        .collect()
}

pub fn apply(connection: &Connection, plan: &SchemaPlan) -> Result<()> {
    for (table, terms) in [
        ("entity_types", &plan.entity_types),
        ("identifier_schemes", &plan.identifier_schemes),
    ] {
        for term in terms {
            connection.execute(
                &format!("INSERT INTO {table} (name, description, introduced_version) VALUES (?1, ?2, ?3)"),
                params![term.name.0, term.description, plan.schema_version],
            ).map_err(storage_error)?;
        }
    }
    for predicate in &plan.predicates {
        connection.execute(
            "INSERT INTO predicates (name, object_kind, description, introduced_version) VALUES (?1, ?2, ?3, ?4)",
            params![predicate.name.0, predicate.object_kind.as_str(), predicate.description, plan.schema_version],
        ).map_err(storage_error)?;
    }
    for endpoint in &plan.endpoints {
        connection
            .execute(
                "INSERT INTO predicate_entity_types (predicate_id, role, entity_type_id)
             VALUES ((SELECT predicate_id FROM predicates WHERE name = ?1),
                     ?2, (SELECT entity_type_id FROM entity_types WHERE name = ?3))",
                params![endpoint.predicate.0, endpoint.role, endpoint.entity_type.0],
            )
            .map_err(storage_error)?;
    }
    if plan.changed() {
        connection
            .execute(
                "UPDATE store_state SET schema_version = ?1 WHERE singleton = 1",
                [plan.schema_version],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}
