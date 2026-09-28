use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use oxigraph::{
    model::{GraphName, Literal, NamedNode, Quad, Term, vocab::xsd},
    store::Store,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::query;

pub const META: &str = "urn:commonplace:metadata";
pub fn id(kind: &str, id: i64) -> Result<NamedNode> {
    ensure!(id > 0, "noncanonical {kind} id {id}");
    Ok(NamedNode::new(format!("urn:commonplace:{kind}:{id}"))?)
}
pub fn property(name: &str) -> Result<NamedNode> {
    Ok(NamedNode::new(format!("urn:commonplace:property:{name}"))?)
}

pub fn fixture(path: &Path) -> Result<Connection> {
    let db = Connection::open(path)?;
    db.execute_batch(include_str!("../fixture.sql"))?;
    for (rid, number, text) in [(1, 1, "café supports Atlas"), (2, 2, "Atlas revised")] {
        db.execute(
            "INSERT INTO revisions VALUES (?1,1,?2,?3,?4)",
            params![
                rid,
                number,
                text,
                format!("{:x}", Sha256::digest(text.as_bytes()))
            ],
        )?;
    }
    db.execute("INSERT INTO passages VALUES (1,1,0,5,'café'),(2,1,6,20,'supports Atlas'),(3,2,0,5,'Atlas')",[])?;
    db.execute_batch("INSERT INTO evidence VALUES (1,1),(2,2),(3,1),(3,2),(4,1),(5,1),(6,1),(7,1),(8,1),(9,1),(10,3)")?;
    Ok(db)
}

pub fn version(db: &Connection) -> Result<i64> {
    Ok(db.query_row("SELECT version FROM state", [], |r| r.get(0))?)
}

pub fn graph_version(store: &Store) -> Result<i64> {
    let result = query::select(
        store,
        "SELECT ?v WHERE { GRAPH <urn:commonplace:metadata> { <urn:commonplace:store> c:knowledge_version ?v } }",
        1,
    )?;
    ensure!(
        result["consumed"] == 1,
        "missing or ambiguous graph version"
    );
    let v = &result["rows"][0][0];
    ensure!(
        v["datatype"] == xsd::INTEGER.as_str(),
        "untyped graph version"
    );
    let version = v["value"].as_str().context("version literal")?.parse()?;
    ensure!(version >= 0, "negative graph version");
    Ok(version)
}

pub fn open_checked(path: &Path, expected: i64) -> Result<Store> {
    let store = Store::open_read_only(path)?;
    ensure!(
        graph_version(&store)? == expected,
        "graph/SQLite version mismatch"
    );
    Ok(store)
}

/// Reads all projection queries from the caller's one SQLite transaction.
pub fn build(db: &Connection, path: &Path) -> Result<()> {
    ensure!(
        !db.is_autocommit(),
        "snapshot must be inside SQLite transaction"
    );
    ensure!(!path.exists(), "candidate already exists");
    let store = Store::open(path)?;
    let mut batch = Vec::<Quad>::new();
    let mut emit = |s: NamedNode, p: &str, o: Term| -> Result<()> {
        batch.push(Quad::new(s, property(p)?, o, GraphName::DefaultGraph));
        if batch.len() >= 128 {
            store.extend(batch.drain(..))?;
        }
        Ok(())
    };
    let mut statement = db.prepare(
        "SELECT k.id,k.kind,k.subject,k.vocab,k.object_entity,k.literal,k.literal_kind
        FROM knowledge k WHERE k.withdrawn=0 ORDER BY k.id",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let kid: i64 = row.get(0)?;
        let kind: String = row.get(1)?;
        let subject: i64 = row.get(2)?;
        let vocab: i64 = row.get(3)?;
        let object: Option<i64> = row.get(4)?;
        let literal: Option<String> = row.get(5)?;
        let literal_kind: Option<String> = row.get(6)?;
        let knowledge = id("knowledge", kid)?;
        emit(
            knowledge.clone(),
            "kind",
            Literal::from(kind.as_str()).into(),
        )?;
        emit(knowledge.clone(), "subject", id("entity", subject)?.into())?;
        if kind == "type_membership" {
            ensure!(object.is_none() && literal.is_none(), "invalid membership");
            emit(knowledge.clone(), "entity_type", id("type", vocab)?.into())?;
        } else {
            ensure!(
                kind == "fact" && (object.is_some() != literal.is_some()),
                "invalid fact subtype"
            );
            emit(
                knowledge.clone(),
                "predicate",
                id("predicate", vocab)?.into(),
            )?;
            let value: Term = if let Some(object) = object {
                id("entity", object)?.into()
            } else {
                let raw: Value = serde_json::from_str(literal.as_deref().context("literal")?)?;
                match literal_kind.as_deref() {
                    Some("integer") => {
                        Literal::from(raw.as_i64().context("signed 64-bit integer")?).into()
                    }
                    Some("boolean") => Literal::from(raw.as_bool().context("boolean")?).into(),
                    Some("string") => Literal::from(raw.as_str().context("string")?).into(),
                    Some("timestamp") => Literal::new_typed_literal(
                        raw.as_str().context("timestamp")?,
                        xsd::DATE_TIME,
                    )
                    .into(),
                    _ => bail!("unsupported literal kind"),
                }
            };
            emit(knowledge.clone(), "object", value)?;
        }
    }
    for (table, kind, filter) in [
        (
            "entities",
            "entity",
            "id IN (SELECT subject FROM knowledge WHERE withdrawn=0 UNION SELECT object_entity FROM knowledge WHERE withdrawn=0)",
        ),
        (
            "types",
            "type",
            "id IN (SELECT vocab FROM knowledge WHERE withdrawn=0 AND kind='type_membership')",
        ),
        (
            "predicates",
            "predicate",
            "id IN (SELECT vocab FROM knowledge WHERE withdrawn=0 AND kind='fact')",
        ),
    ] {
        let mut statement = db.prepare(&format!(
            "SELECT id,name FROM {table} WHERE {filter} ORDER BY id"
        ))?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            emit(
                id(kind, row.get(0)?)?,
                "name",
                Literal::from(row.get::<_, String>(1)?).into(),
            )?;
        }
    }
    let mut statement = db.prepare("SELECT DISTINCT p.id,p.revision,p.start,p.end,p.text,r.document,r.number,r.text,r.digest,d.source
        FROM evidence e JOIN knowledge k ON k.id=e.knowledge JOIN passages p ON p.id=e.passage
        JOIN revisions r ON r.id=p.revision JOIN documents d ON d.id=r.document
        WHERE k.withdrawn=0 ORDER BY p.id")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let pid: i64 = row.get(0)?;
        let rid: i64 = row.get(1)?;
        let start: i64 = row.get(2)?;
        let end: i64 = row.get(3)?;
        let text: String = row.get(4)?;
        let did: i64 = row.get(5)?;
        let number: i64 = row.get(6)?;
        let source_text: String = row.get(7)?;
        let digest: String = row.get(8)?;
        ensure!(
            source_text.get(usize::try_from(start)?..usize::try_from(end)?) == Some(text.as_str()),
            "invalid canonical evidence offsets"
        );
        emit(id("passage", pid)?, "revision", id("revision", rid)?.into())?;
        emit(
            id("passage", pid)?,
            "start_byte",
            Literal::from(start).into(),
        )?;
        emit(id("passage", pid)?, "end_byte", Literal::from(end).into())?;
        emit(id("passage", pid)?, "text", Literal::from(text).into())?;
        emit(id("revision", rid)?, "document", id("doc", did)?.into())?;
        emit(
            id("revision", rid)?,
            "revision_number",
            Literal::from(number).into(),
        )?;
        emit(id("revision", rid)?, "digest", Literal::from(digest).into())?;
        emit(
            id("doc", did)?,
            "source_key",
            Literal::from(row.get::<_, String>(9)?).into(),
        )?;
    }
    let mut statement = db.prepare("SELECT e.knowledge,e.passage FROM evidence e JOIN knowledge k ON e.knowledge=k.id WHERE k.withdrawn=0 ORDER BY e.knowledge,e.passage")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        emit(
            id("knowledge", row.get(0)?)?,
            "evidence",
            id("passage", row.get(1)?)?.into(),
        )?;
    }
    store.extend(batch)?;
    store.insert(&Quad::new(
        NamedNode::new("urn:commonplace:store")?,
        property("knowledge_version")?,
        Literal::from(version(db)?),
        NamedNode::new(META)?,
    ))?;
    store.flush()?;
    drop(store);
    let reopened = open_checked(path, version(db)?)?;
    ensure!(reopened.len()? > 0, "empty graph metadata missing");
    drop(reopened);
    Ok(())
}

pub const EVIDENCE_QUERY: &str = "SELECT ?k ?subject ?object ?passage ?revision ?document ?start ?end ?quote WHERE {
    ?k c:kind 'fact'; c:predicate <urn:commonplace:predicate:1>; c:subject ?subject; c:object ?object; c:evidence ?passage .
    ?membership c:subject ?subject; c:entity_type <urn:commonplace:type:1> .
    ?passage c:revision ?revision; c:start_byte ?start; c:end_byte ?end; c:text ?quote .
    ?revision c:document ?document .
} ORDER BY ?k ?passage";

pub fn verify(db: &Connection, store: &Store) -> Result<Value> {
    ensure!(graph_version(store)? == version(db)?);
    let evidence = query::select(store, EVIDENCE_QUERY, 20)?;
    ensure!(
        evidence["rows"].as_array().context("evidence rows")?.len() == 3,
        "equal facts or citations lost: {evidence}"
    );
    let rows = evidence["rows"].as_array().context("rows")?;
    ensure!(
        rows[0][0]["value"] == "urn:commonplace:knowledge:3"
            && rows[1][0]["value"] == "urn:commonplace:knowledge:3"
            && rows[2][0]["value"] == "urn:commonplace:knowledge:4"
    );
    for row in rows {
        let pid: i64 = row[3]["value"]
            .as_str()
            .context("passage IRI")?
            .rsplit(':')
            .next()
            .context("id")?
            .parse()?;
        let exact: String =
            db.query_row("SELECT text FROM passages WHERE id=?1", [pid], |r| r.get(0))?;
        ensure!(row[8]["value"] == exact);
        ensure!(
            row[4]["value"] == "urn:commonplace:revision:1",
            "old revision identity lost"
        );
    }
    let missing = query::select(
        store,
        "ASK { VALUES ?s { <urn:commonplace:knowledge:10> <urn:commonplace:type:3> <urn:commonplace:predicate:99> <urn:commonplace:entity:99> <urn:commonplace:revision:2> <urn:commonplace:passage:3> } ?s ?p ?o }",
        1,
    )?;
    ensure!(
        missing["boolean"] == false,
        "inactive or unused content included"
    );
    let values = query::select(
        store,
        "SELECT ?k ?v WHERE { ?k c:object ?v FILTER(isLiteral(?v)) } ORDER BY ?k",
        10,
    )?;
    let rows = values["rows"].as_array().context("literal rows")?;
    ensure!(
        rows.len() == 5
            && rows[0][1]["value"] == "-9223372036854775808"
            && rows[1][1]["value"] == "9223372036854775807"
    );
    ensure!(
        rows[2][1]["datatype"] == xsd::BOOLEAN.as_str()
            && rows[3][1]["datatype"] == xsd::DATE_TIME.as_str()
    );
    Ok(
        json!({"evidence_query":EVIDENCE_QUERY,"evidence":evidence,"literals":values,"active_only":true,"version":graph_version(store)?,"quads":store.len()?}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_must_be_present_unique_typed_and_nonnegative() -> Result<()> {
        let store = Store::new()?;
        ensure!(graph_version(&store).is_err());
        for value in [Literal::from("1"), Literal::from(-1_i64)] {
            store.insert(&Quad::new(
                NamedNode::new("urn:commonplace:store")?,
                property("knowledge_version")?,
                value,
                NamedNode::new(META)?,
            ))?;
            ensure!(graph_version(&store).is_err());
            store.clear()?;
        }
        for version in [1_i64, 2] {
            store.insert(&Quad::new(
                NamedNode::new("urn:commonplace:store")?,
                property("knowledge_version")?,
                Literal::from(version),
                NamedNode::new(META)?,
            ))?;
        }
        ensure!(graph_version(&store).is_err());
        Ok(())
    }
}
