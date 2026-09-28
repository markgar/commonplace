use anyhow::{Context, Result, bail, ensure};
use grafeo::{EdgeId, GrafeoDB, NodeId, QueryResult, Value as GraphValue};
use serde_json::{Value, json};

fn json_native(value: &GraphValue) -> Result<Value> {
    Ok(match value {
        GraphValue::Null => Value::Null,
        GraphValue::Bool(value) => json!(value),
        GraphValue::Int64(value) => json!(value),
        GraphValue::Float64(value) => {
            ensure!(value.is_finite(), "non-finite graph float");
            json!(value)
        }
        GraphValue::String(value) => json!(value.as_str()),
        GraphValue::List(values) => {
            Value::Array(values.iter().map(json_native).collect::<Result<_>>()?)
        }
        GraphValue::Map(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.to_string(), json_native(value)?)))
                .collect::<Result<_>>()?,
        ),
        _ => bail!("unsupported graph value (not a JSON-native scalar, list or object)"),
    })
}

fn engine_id(value: &GraphValue) -> Result<u64> {
    if let GraphValue::Int64(id) = value {
        return Ok((*id).try_into()?);
    }
    let GraphValue::Map(map) = value else {
        bail!("expected fixture graph map")
    };
    let id = map
        .iter()
        .find(|(key, _)| key.as_str() == "_id")
        .and_then(|(_, value)| value.as_int64())
        .context("missing engine ID")?;
    Ok(id.try_into()?)
}

fn node(db: &GrafeoDB, id: u64) -> Result<Value> {
    let node = db
        .get_node(NodeId::new(id))
        .context("missing fixture node")?;
    let mut labels: Vec<_> = node.labels.iter().map(|label| label.to_string()).collect();
    labels.sort();
    let properties = json_native(&GraphValue::Map(node.properties_as_btree().into()))?;
    Ok(json!({"kind":"node", "labels":labels, "properties":properties}))
}

fn edge(db: &GrafeoDB, id: u64) -> Result<Value> {
    let edge = db
        .get_edge(EdgeId::new(id))
        .context("missing fixture edge")?;
    let source = node(db, edge.src.as_u64())?;
    let target = node(db, edge.dst.as_u64())?;
    let properties = json_native(&GraphValue::Map(edge.properties_as_btree().into()))?;
    Ok(
        json!({"kind":"relationship", "type":edge.edge_type.as_str(),
        "source":source["properties"]["id"], "target":target["properties"]["id"],
        "properties":properties}),
    )
}

// Fixture positions are known from the literal query, not inferred from map keys.
// Arbitrary Cypher needs unambiguous engine value tags; see the collision probe.
pub fn fixture(db: &GrafeoDB, result: &QueryResult) -> Result<Value> {
    let row = &result.rows()[0];
    let GraphValue::Path { nodes, edges } = &row[2] else {
        bail!("expected graph path")
    };
    let path = json!({
        "kind":"path",
        "nodes":nodes.iter().map(|v| node(db, engine_id(v)?)).collect::<Result<Vec<_>>>()?,
        "relationships":edges.iter().map(|v| edge(db, engine_id(v)?)).collect::<Result<Vec<_>>>()?
    });
    let mut converted = vec![
        node(db, engine_id(&row[0])?)?,
        edge(db, engine_id(&row[1])?)?,
        path,
    ];
    converted.extend(
        row[3..]
            .iter()
            .map(json_native)
            .collect::<Result<Vec<_>>>()?,
    );
    ensure!(
        converted[1]["source"] == "entity:1" && converted[1]["target"] == "entity:2",
        "canonical endpoints not preserved"
    );
    ensure!(
        converted[2]["nodes"]
            .as_array()
            .context("path nodes")?
            .len()
            == 2,
        "path ordering missing"
    );
    ensure!(
        converted[5] == json!([1, true, null]) && converted[6] == json!({"a":"text","z":2}),
        "recursive scalar/list/object serialization failed"
    );
    ensure!(
        json_native(&GraphValue::Bytes(vec![1_u8].into())).is_err(),
        "unsupported bytes accepted"
    );
    ensure!(
        json_native(&GraphValue::Float64(f64::NAN)).is_err(),
        "non-finite float accepted"
    );
    Ok(json!({"columns":result.columns, "rows":[converted]}))
}
