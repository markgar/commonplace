use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use oxigraph::{
    model::{Literal, NamedNode, Term, Variable},
    sparql::{CancellationToken, QueryEvaluationError, QueryResults, SparqlEvaluator},
    store::Store,
};
use serde_json::{Value, json};

pub const PREFIX: &str = "PREFIX c: <urn:commonplace:property:> ";

pub fn term(value: &Term) -> Value {
    match value {
        Term::NamedNode(v) => json!({"type":"uri","value":v.as_str()}),
        Term::BlankNode(v) => json!({"type":"bnode","value":v.as_str()}),
        Term::Literal(v) => {
            let value = json!({"type":"literal","value":v.value(),
                "datatype":v.datatype().as_str(),"language":v.language()});
            #[cfg(feature = "rdf12-probe")]
            let value = {
                let mut value = value;
                value["direction"] = json!(v.direction().map(|d| d.to_string()));
                value
            };
            value
        }
        #[cfg(feature = "rdf12-probe")]
        Term::Triple(v) => json!({"type":"triple","subject":term(&v.subject.clone().into()),
            "predicate":term(&v.predicate.clone().into()),"object":term(&v.object)}),
    }
}

pub fn collect(results: QueryResults<'_>, limit: usize) -> Result<Value> {
    let cap = limit.checked_add(1).context("row limit overflow")?;
    Ok(match results {
        QueryResults::Solutions(mut solutions) => {
            let columns: Vec<_> = solutions
                .variables()
                .iter()
                .map(|v| v.as_str().to_owned())
                .collect();
            let mut rows = Vec::new();
            for solution in solutions.by_ref().take(cap) {
                let solution = solution?;
                rows.push(
                    columns
                        .iter()
                        .map(|v| solution.get(v.as_str()).map(term).unwrap_or(Value::Null))
                        .collect::<Vec<_>>(),
                );
            }
            let consumed = rows.len();
            let truncated = consumed > limit;
            rows.truncate(limit);
            json!({"kind":"select","columns":columns,"rows":rows,"consumed":consumed,"truncated":truncated})
        }
        QueryResults::Boolean(value) => json!({"kind":"ask","boolean":value}),
        QueryResults::Graph(mut triples) => {
            let mut rows = Vec::new();
            for triple in triples.by_ref().take(cap) {
                let triple = triple?;
                rows.push(json!({"subject":term(&triple.subject.into()),
                    "predicate":term(&triple.predicate.into()),"object":term(&triple.object)}));
            }
            let consumed = rows.len();
            rows.truncate(limit);
            json!({"kind":"graph","triples":rows,"consumed":consumed,"truncated":consumed > limit})
        }
    })
}

struct Deadline {
    finish: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Deadline {
    fn start(token: CancellationToken, timeout: Duration) -> Self {
        let (finish, wait) = mpsc::channel();
        let worker = thread::spawn(move || {
            if matches!(
                wait.recv_timeout(timeout),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                token.cancel();
            }
        });
        Self {
            finish: Some(finish),
            worker: Some(worker),
        }
    }
}
impl Drop for Deadline {
    fn drop(&mut self) {
        if let Some(finish) = self.finish.take() {
            let _ = finish.send(());
        }
        self.worker
            .take()
            .expect("deadline worker")
            .join()
            .expect("deadline worker panic");
    }
}

pub fn run(store: &Store, query: &str, limit: usize, timeout: Duration) -> Result<Value> {
    let token = CancellationToken::new();
    let _deadline = Deadline::start(token.clone(), timeout);
    let results = SparqlEvaluator::new()
        .with_cancellation_token(token)
        .parse_query(query)?
        .on_store(store)
        .execute()?;
    collect(results, limit)
}

pub fn select(store: &Store, query: &str, limit: usize) -> Result<Value> {
    run(
        store,
        &format!("{PREFIX}{query}"),
        limit,
        Duration::from_secs(10),
    )
}

pub fn expensive() -> &'static str {
    "SELECT (COUNT(*) AS ?n) WHERE {
        ?a <urn:probe:n> ?x . ?b <urn:probe:n> ?y . ?c <urn:probe:n> ?z .
        FILTER(?x + ?y + ?z < 0)
    }"
}

pub fn probe(store: &Store) -> Result<Value> {
    let mut cases = Vec::new();
    for query in [
        "SELECT ?s ?x WHERE { ?s <urn:probe:n> ?x }",
        "SELECT ?x WHERE { ?s <urn:probe:n> ?x } ORDER BY DESC(?x)",
        "SELECT DISTINCT ?x WHERE { VALUES ?x { 2 1 2 3 4 } } ORDER BY ?x",
        "SELECT (SUM(?x) AS ?n) WHERE { VALUES ?x { 1 2 3 4 } }",
        "SELECT ?x WHERE { { VALUES ?x { 1 2 } } UNION { VALUES ?x { 3 4 } } }",
        "SELECT ?x WHERE { ?s <urn:probe:n> ?x } ORDER BY ?x OFFSET 2 LIMIT 5",
    ] {
        let full = select(store, query, 3000)?;
        let bounded = select(store, query, 2)?;
        ensure!(bounded["columns"] == full["columns"]);
        let all = full["rows"].as_array().context("rows")?;
        ensure!(
            bounded["rows"] == json!(&all[..all.len().min(2)]),
            "prefix changed: {query}"
        );
        ensure!(bounded["consumed"].as_u64().context("consumed")? <= 3);
        cases.push(json!({"query":query,"bounded":bounded,"full_count":all.len()}));
    }
    for value in [7_i64, 19] {
        let result = SparqlEvaluator::new()
            .parse_query("SELECT ?x WHERE { FILTER(?x > 0) }")?
            .substitute_variable(Variable::new("x")?, Literal::from(value))
            .on_store(store)
            .execute()?;
        ensure!(collect(result, 2)?["rows"][0][0]["value"] == value.to_string());
    }
    for value in [i64::MIN, i64::MAX] {
        let result = SparqlEvaluator::new()
            .parse_query("SELECT ?x WHERE {}")?
            .substitute_variable(Variable::new("x")?, Literal::from(value))
            .on_store(store)
            .execute()?;
        ensure!(collect(result, 1)?["rows"][0][0]["value"] == value.to_string());
    }
    let bare_minimum = select(
        store,
        "SELECT (-9223372036854775808 AS ?minimum) WHERE {}",
        1,
    )?;
    // This supported custom function is instrumentation only, never the production evaluator.
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let results = SparqlEvaluator::new()
        .with_custom_function(NamedNode::new("urn:probe:tick")?, move |args| {
            counted.fetch_add(1, Ordering::SeqCst);
            args.first().cloned()
        })
        .parse_query("SELECT (<urn:probe:tick>(?x) AS ?v) WHERE { ?s <urn:probe:n> ?x }")?
        .on_store(store)
        .execute()?;
    let before = calls.load(Ordering::SeqCst);
    let result = collect(results, 2)?;
    let after = calls.load(Ordering::SeqCst);
    ensure!(
        before == 0 && after == 3,
        "not lazy: before={before}, after={after}"
    );
    ensure!(result["consumed"] == 3);

    let mut denied = Vec::new();
    for update in [
        "INSERT DATA { <urn:x> <urn:p> 1 }",
        "DELETE WHERE { ?s ?p ?o }",
        "CLEAR ALL",
        "LOAD <http://127.0.0.1:9/data>",
    ] {
        ensure!(SparqlEvaluator::new().parse_query(update).is_err());
        let error = SparqlEvaluator::new()
            .parse_update(update)?
            .on_store(store)
            .execute()
            .unwrap_err();
        denied.push(error.to_string());
    }
    let direct = store
        .insert(&oxigraph::model::Quad::new(
            NamedNode::new("urn:x")?,
            NamedNode::new("urn:p")?,
            Literal::from(1),
            oxigraph::model::GraphName::DefaultGraph,
        ))
        .unwrap_err();
    ensure!(store.clear().is_err());
    let service = select(
        store,
        "SELECT * WHERE { SERVICE <http://127.0.0.1:9/sparql> { ?s ?p ?o } }",
        2,
    )
    .unwrap_err();
    let silent = select(
        store,
        "SELECT * WHERE { SERVICE SILENT <http://127.0.0.1:9/sparql> { ?s ?p ?o } }",
        2,
    )?;
    let mut cancellation = Vec::new();
    for q in [
        expensive(),
        "SELECT ?x WHERE { ?a <urn:probe:n> ?x . ?b <urn:probe:n> ?y . ?c <urn:probe:n> ?z . FILTER(?x + ?y + ?z < 0) }",
    ] {
        let start = Instant::now();
        let error = run(store, q, 2, Duration::from_millis(20)).unwrap_err();
        ensure!(
            error
                .downcast_ref::<QueryEvaluationError>()
                .is_some_and(|e| matches!(e, QueryEvaluationError::Cancelled)),
            "not native cancellation: {error:#}"
        );
        ensure!(
            start.elapsed() < Duration::from_secs(3),
            "cancellation unbounded"
        );
        cancellation.push(json!({"query":q,"error":error.to_string(),"elapsed_ms":start.elapsed().as_secs_f64()*1000.0}));
    }
    let start = Instant::now();
    let ask = run(
        store,
        "ASK { ?s <urn:probe:n> ?x }",
        2,
        Duration::from_secs(30),
    )?;
    ensure!(
        ask["boolean"] == true && start.elapsed() < Duration::from_secs(1),
        "fast query waited for timer"
    );
    let fast_timer_join_ms = start.elapsed().as_secs_f64() * 1000.0;
    let construct = select(
        store,
        "CONSTRUCT { ?s <urn:probe:copy> ?x } WHERE { ?s <urn:probe:n> ?x }",
        2,
    )?;
    ensure!(construct["consumed"] == 3 && construct["truncated"] == true);
    let typed = select(
        store,
        "SELECT ?uri ?blank ?lo ?hi ?bool ?text ?date ?missing WHERE {
        BIND(<urn:commonplace:entity:1> AS ?uri) BIND(BNODE('local') AS ?blank)
        BIND('-9223372036854775808'^^<http://www.w3.org/2001/XMLSchema#integer> AS ?lo) BIND(9223372036854775807 AS ?hi)
        BIND(true AS ?bool) BIND('café'@fr AS ?text)
        BIND('2026-09-28T00:00:00Z'^^<http://www.w3.org/2001/XMLSchema#dateTime> AS ?date) }",
        2,
    )?;
    ensure!(typed["rows"][0][2]["value"] == i64::MIN.to_string());
    ensure!(typed["rows"][0][3]["value"] == i64::MAX.to_string());
    ensure!(typed["rows"][0][7].is_null());
    #[cfg(feature = "rdf12-probe")]
    let triple = {
        let result = select(
            store,
            "SELECT (TRIPLE(<urn:s>,<urn:p>,1) AS ?triple) WHERE {}",
            1,
        )?;
        ensure!(result["rows"][0][0]["type"] == "triple");
        result
    };
    #[cfg(not(feature = "rdf12-probe"))]
    let triple = json!({"enabled":false,"reason":"ordinary reification needs RDF 1.1 only"});
    Ok(
        json!({"queries":cases,"lazy_function_calls":{"before_execute":before,"after_three_rows":after},
        "read_only_update_errors":denied,"read_only_insert_error":direct.to_string(),
        "service_error":service.to_string(),"service_silent":silent,"cancellation":cancellation,
        "fast_timer_join_ms":fast_timer_join_ms,"ask":ask,"construct":construct,"typed":typed,"optional_triple":triple,
        "bare_minimum_expression":bare_minimum,"typed_bound_i64_extrema":"pass"}),
    )
}
