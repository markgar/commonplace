use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use grafeo::{Config, GrafeoDB, Role, Value};
use grafeo_common::utils::error::{Error, QueryErrorKind};
use serde_json::json;

pub fn encode(db: &GrafeoDB, value: &Value) -> Result<serde_json::Value> {
    Ok(match value {
        Value::Null => json!(null),
        Value::Bool(v) => json!(v),
        Value::Int64(v) => json!(v),
        Value::Float64(v) => {
            ensure!(v.is_finite(), "nonfinite float");
            json!(v)
        }
        Value::String(v) => json!(v.as_str()),
        Value::List(v) => json!(
            v.iter()
                .map(|v| encode(db, v))
                .collect::<Result<Vec<_>>>()?
        ),
        Value::Map(v) => serde_json::Value::Object(
            v.iter()
                .map(|(k, v)| Ok((k.to_string(), encode(db, v)?)))
                .collect::<Result<_>>()?,
        ),
        Value::Node(id) => {
            let node = db.get_node(*id).context("missing graph node")?;
            let mut labels: Vec<_> = node.labels.iter().map(|s| s.to_string()).collect();
            labels.sort();
            json!({"kind":"node", "labels":labels,
                "properties":encode(db, &Value::Map(node.properties_as_btree().into()))?})
        }
        Value::Edge(id) => {
            let edge = db.get_edge(*id).context("missing graph edge")?;
            let source = encode(db, &Value::Node(edge.src))?;
            let target = encode(db, &Value::Node(edge.dst))?;
            let source_id = source["properties"]["id"]
                .as_str()
                .context("missing canonical source id")?;
            let target_id = target["properties"]["id"]
                .as_str()
                .context("missing canonical target id")?;
            json!({"kind":"relationship", "type":edge.edge_type.as_str(),
                "source":source_id, "target":target_id,
                "properties":encode(db, &Value::Map(edge.properties_as_btree().into()))?})
        }
        Value::Path { nodes, edges } => {
            json!({"kind":"path", "nodes":nodes.iter().map(|v| encode(db, v)).collect::<Result<Vec<_>>>()?,
                "relationships":edges.iter().map(|v| encode(db, v)).collect::<Result<Vec<_>>>()?})
        }
        _ => bail!("unsupported graph value: {}", value.type_name()),
    })
}

pub fn probe() -> Result<serde_json::Value> {
    let db = GrafeoDB::new_in_memory();
    db.execute_cypher("CREATE (:Entity {id:'entity:1', name:'Atlas'})")?;
    db.execute_cypher("CREATE (:Entity {id:'entity:2', name:'Roadmap'})")?;
    db.execute_cypher("MATCH (a:Entity {id:'entity:1'}), (b:Entity {id:'entity:2'}) CREATE (a)-[:RELATIONSHIP {predicate:'depends_on'}]->(b)")?;
    let reader = db.session_with_role(Role::ReadOnly);
    for mutation in [
        "CREATE (:Entity)",
        "MATCH (n:Entity) SET n.name = $name",
        "MATCH (n:Entity) DETACH DELETE n",
        "MERGE (:Entity {id:$name})",
    ] {
        let error = reader
            .execute_cypher_bounded(
                mutation,
                HashMap::from([("name".into(), Value::from("new"))]),
                3,
            )
            .unwrap_err();
        ensure!(
            error.to_string().contains("permission denied"),
            "authorization bypassed: {error}"
        );
    }
    let mut evidence = Vec::new();
    for (query, params) in [
        ("UNWIND range(1, 10000) AS x RETURN x", HashMap::new()),
        (
            "UNWIND range(1, 10000) AS x RETURN x ORDER BY x DESC",
            HashMap::new(),
        ),
        (
            "UNWIND [2, 1, 2, 3, 4] AS x RETURN DISTINCT x ORDER BY x",
            HashMap::new(),
        ),
        (
            "UNWIND range(1, 100) AS x RETURN sum(x) AS total",
            HashMap::new(),
        ),
        (
            "RETURN 1 AS x UNION ALL RETURN 2 AS x UNION ALL RETURN 3 AS x UNION ALL RETURN 4 AS x",
            HashMap::new(),
        ),
        (
            "UNWIND range(1, $n) AS x RETURN x ORDER BY x DESC SKIP 2 LIMIT 5",
            HashMap::from([("n".into(), Value::Int64(20))]),
        ),
    ] {
        let bounded = reader.execute_cypher_bounded(query, params.clone(), 3)?;
        let full = reader.execute_language(query, "cypher", Some(params))?;
        ensure!(
            bounded.rows() == &full.rows()[..full.row_count().min(3)],
            "query semantics differ: {query}"
        );
        ensure!(bounded.row_count() <= 3, "final result exceeds cap");
        evidence.push(json!({"query":query,"full":full.row_count(),"bounded":bounded.row_count()}));
    }
    for n in [7, 19] {
        let rows = reader.execute_cypher_bounded(
            "RETURN $n AS n",
            HashMap::from([("n".into(), Value::Int64(n))]),
            1,
        )?;
        ensure!(rows.scalar::<i64>()? == n, "parameter cache contamination");
    }
    let query = "MATCH p=(a:Entity)-[r:RELATIONSHIP]->(b:Entity)
        RETURN a AS alias, r AS edge, p AS path,
        [a, r, 7, {node:a, edge:r}] AS mixed,
        {_id:0, _labels:['Entity'], id:'entity:1', name:'Atlas'} AS lookalike";
    let result = reader.execute_cypher_bounded(query, HashMap::new(), 3)?;
    let row = &result.rows()[0];
    ensure!(
        matches!(row[0], Value::Node(_)) && matches!(row[1], Value::Edge(_)),
        "top-level identity lost: {row:?}"
    );
    let encoded = row
        .iter()
        .map(|v| encode(&db, v))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        encoded[3][0]["kind"] == "node"
            && encoded[3][1]["kind"] == "relationship"
            && encoded[3][2] == 7
            && encoded[3][3]["node"]["kind"] == "node"
            && encoded[3][3]["edge"]["kind"] == "relationship",
        "nested identity lost: {encoded:?}"
    );
    ensure!(
        encoded[4].get("kind").is_none(),
        "lookalike object mistagged"
    );
    ensure!(
        encoded[2]["nodes"][0]["kind"] == "node"
            && encoded[2]["relationships"][0]["kind"] == "relationship",
        "path identity lost: {encoded:?}"
    );
    let mut semantic_cases = Vec::new();
    for (query, expected) in [
        (
            "MATCH p=(a:Entity)-[r:RELATIONSHIP]->(b:Entity) RETURN nodes(p), relationships(p), startNode(r), endNode(r)",
            1,
        ),
        (
            "MATCH (a:Entity) WITH a AS x RETURN x, x.name ORDER BY x.name",
            2,
        ),
        ("MATCH (a:Entity) RETURN collect(a) AS nodes", 1),
        ("MATCH (a:Entity) RETURN DISTINCT a", 2),
        (
            "MATCH (a:Entity)-[r:RELATIONSHIP]->(b:Entity) UNWIND [a,r,0,{_id:0},a] AS x RETURN DISTINCT x",
            4,
        ),
        (
            "MATCH (a:Entity)-[r:RELATIONSHIP]->(b:Entity) RETURN a=a, a=b, a=r, a=0, r=r",
            1,
        ),
        (
            "MATCH (a:Entity) RETURN CASE WHEN a.name='Atlas' THEN {v:a} ELSE [a] END AS mixed",
            2,
        ),
        (
            "MATCH (a:Entity)-[r:RELATIONSHIP]->(b:Entity) RETURN a AS x UNION ALL MATCH (a:Entity)-[r:RELATIONSHIP]->(b:Entity) RETURN r AS x",
            2,
        ),
    ] {
        let result = reader
            .execute_cypher_bounded(query, HashMap::new(), 10)
            .with_context(|| query)?;
        ensure!(
            result.row_count() == expected,
            "semantic row count: {query}: {:?}",
            result.rows()
        );
        let rows = result
            .rows()
            .iter()
            .map(|row| {
                row.iter()
                    .map(|v| encode(&db, v))
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        semantic_cases.push(json!({"query":query,"rows":rows}));
    }
    let path_functions = &semantic_cases[0]["rows"][0];
    ensure!(
        path_functions[0][0]["kind"] == "node"
            && path_functions[1][0]["kind"] == "relationship"
            && path_functions[2]["kind"] == "node"
            && path_functions[3]["kind"] == "node",
        "path function tags: {path_functions}"
    );
    ensure!(
        semantic_cases[1]["rows"][0][0]["kind"] == "node"
            && semantic_cases[1]["rows"][0][1] == "Atlas",
        "WITH/property semantics"
    );
    ensure!(
        semantic_cases[2]["rows"][0][0][0]["kind"] == "node",
        "collect erased identity"
    );
    ensure!(
        semantic_cases[5]["rows"][0] == json!([true, false, false, false, true]),
        "identity equality"
    );
    ensure!(
        semantic_cases[6]["rows"]
            .as_array()
            .context("CASE rows")?
            .iter()
            .all(|row| row[0]["v"]["kind"] == "node" || row[0][0]["kind"] == "node"),
        "CASE identity"
    );
    ensure!(
        semantic_cases[7]["rows"][0][0]["kind"] == "node"
            && semantic_cases[7]["rows"][1][0]["kind"] == "relationship",
        "UNION identity"
    );
    for query in ["EXPLAIN MATCH (n) RETURN n", "PROFILE MATCH (n) RETURN n"] {
        let err = reader
            .execute_cypher_bounded(query, HashMap::new(), 3)
            .unwrap_err();
        ensure!(
            err.to_string().contains("excludes EXPLAIN/PROFILE"),
            "special query not explicit: {err}"
        );
    }
    ensure!(encode(&db, &Value::Float64(f64::NAN)).is_err());
    let unsupported =
        reader.execute_cypher_bounded("RETURN date('2025-01-01')", HashMap::new(), 1)?;
    ensure!(
        encode(&db, &unsupported.rows()[0][0]).is_err(),
        "unsupported value silently converted"
    );
    let timed =
        GrafeoDB::with_config(Config::in_memory().with_query_timeout(Duration::from_millis(1)))?;
    let error = timed
        .session_with_role(Role::ReadOnly)
        .execute_cypher_bounded(
            "UNWIND range(1,$n) AS x UNWIND range(1,$n) AS y RETURN sum(x*y)",
            HashMap::from([("n".into(), Value::Int64(1000000))]),
            3,
        )
        .unwrap_err();
    ensure!(
        matches!(&error, Error::Query(query) if query.kind == QueryErrorKind::Timeout),
        "native deadline not enforced: {error}"
    );
    Ok(
        json!({"query_regressions":evidence, "semantic_row":encoded, "semantic_cases":semantic_cases, "authorization":"pass",
        "parameter_binding":"pass", "deadline":error.to_string()}),
    )
}
