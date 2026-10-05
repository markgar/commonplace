use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use oxigraph::model::Term;
use oxigraph::sparql::{CancellationToken, QueryEvaluationError, QueryResults, SparqlEvaluator};
use oxigraph::store::Store;
use serde::Serialize;
use spargebra::{Query, SparqlParser};

use crate::{CommonplaceError, Result};

#[derive(Debug, Clone, Copy)]
pub struct QueryConfig {
    pub row_limit: usize,
    pub timeout: Duration,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            row_limit: 1000,
            timeout: Duration::from_secs(5),
        }
    }
}

impl QueryConfig {
    pub(crate) fn validate(self) -> Result<()> {
        if self.row_limit.checked_add(1).is_none() {
            return Err(CommonplaceError::InvalidInput(
                "row limit is too large".into(),
            ));
        }
        if self.timeout.is_zero()
            || std::time::Instant::now()
                .checked_add(self.timeout)
                .is_none()
        {
            return Err(CommonplaceError::InvalidInput(
                "query timeout must be positive and fit the monotonic clock".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct SelectResult {
    pub kind: &'static str,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<RdfTerm>>>,
    pub truncated: bool,
}

impl SelectResult {
    pub fn document_scope(self, column: &str) -> Result<crate::domain::search::DocumentScope> {
        let index = self
            .columns
            .iter()
            .position(|name| name == column)
            .ok_or_else(|| {
                CommonplaceError::InvalidInput(format!(
                    "SELECT has no column {column:?}; pass its variable name without ?"
                ))
            })?;
        let document_ids = self
            .rows
            .into_iter()
            .enumerate()
            .map(|(row, values)| {
                let Some(Some(RdfTerm::Uri { value })) = values.get(index) else {
                    return Err(CommonplaceError::InvalidInput(format!(
                        "row {row} column {column:?} must bind a canonical document IRI"
                    )));
                };
                let value = value.strip_prefix("urn:commonplace:").ok_or_else(|| {
                    CommonplaceError::InvalidInput(format!(
                        "row {row}: not a Commonplace document IRI"
                    ))
                })?;
                crate::domain::search::canonical_document_id(value).map(|id| id.to_string())
            })
            .collect::<Result<Vec<_>>>()?;
        let scope = crate::domain::search::DocumentScope {
            document_ids,
            truncated: self.truncated,
        };
        scope.validate()?;
        Ok(scope)
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RdfTerm {
    Uri {
        value: String,
    },
    Bnode {
        value: String,
    },
    Literal {
        value: String,
        datatype: String,
        language: Option<String>,
    },
}

impl From<&Term> for RdfTerm {
    fn from(term: &Term) -> Self {
        match term {
            Term::NamedNode(node) => Self::Uri {
                value: node.as_str().into(),
            },
            Term::BlankNode(node) => Self::Bnode {
                value: node.as_str().into(),
            },
            Term::Literal(literal) => Self::Literal {
                value: literal.value().into(),
                datatype: literal.datatype().as_str().into(),
                language: literal.language().map(str::to_owned),
            },
        }
    }
}

pub(crate) fn parse(text: &str) -> Result<Query> {
    let query = SparqlParser::new().parse_query(text).map_err(|error| {
        CommonplaceError::InvalidInput(format!("invalid SPARQL SELECT: {error}"))
    })?;
    if !matches!(query, Query::Select { .. }) {
        return Err(CommonplaceError::InvalidInput(
            "only SPARQL SELECT is supported; ASK, CONSTRUCT, DESCRIBE and Update are not accepted"
                .into(),
        ));
    }
    Ok(query)
}

pub(crate) fn execute(store: &Store, query: Query, config: QueryConfig) -> Result<SelectResult> {
    config.validate()?;
    let token = CancellationToken::new();
    let prepared = SparqlEvaluator::new()
        .with_cancellation_token(token.clone())
        .for_query(query)
        .on_store(store);
    let deadline = Deadline::start(token.clone(), config.timeout)?;
    let result = prepared
        .execute()
        .map_err(evaluation_error)
        .and_then(|results| collect(results, config.row_limit));
    deadline.finish()?;
    if token.is_cancelled() {
        return Err(CommonplaceError::LimitExceeded(
            "graph query evaluation budget expired".into(),
        ));
    }
    result
}

fn collect(results: QueryResults<'_>, limit: usize) -> Result<SelectResult> {
    let QueryResults::Solutions(mut solutions) = results else {
        return Err(CommonplaceError::Graph(
            "SELECT returned a non-solution result".into(),
        ));
    };
    let columns: Vec<String> = solutions
        .variables()
        .iter()
        .map(|v| v.as_str().to_owned())
        .collect();
    let mut rows = Vec::new();
    let mut truncated = false;
    for solution in solutions.by_ref().take(limit + 1) {
        let solution = solution.map_err(evaluation_error)?;
        if rows.len() == limit {
            truncated = true;
            break;
        }
        rows.push(
            columns
                .iter()
                .map(|column| solution.get(column.as_str()).map(RdfTerm::from))
                .collect(),
        );
    }
    Ok(SelectResult {
        kind: "select",
        columns,
        rows,
        truncated,
    })
}

fn evaluation_error(error: QueryEvaluationError) -> CommonplaceError {
    match error {
        QueryEvaluationError::Cancelled => {
            CommonplaceError::LimitExceeded("graph query evaluation budget expired".into())
        }
        _ => CommonplaceError::Graph(format!("SPARQL evaluation failed: {error}")),
    }
}

struct Deadline {
    finish: mpsc::Sender<()>,
    worker: thread::JoinHandle<()>,
}

impl Deadline {
    fn start(token: CancellationToken, timeout: Duration) -> Result<Self> {
        let (finish, wait) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("graph-query-budget".into())
            .spawn(move || {
                if matches!(
                    wait.recv_timeout(timeout),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    token.cancel();
                }
            })?;
        Ok(Self { finish, worker })
    }

    fn finish(self) -> Result<()> {
        // Disconnection means the timer already expired; joining still releases it.
        let _ = self.finish.send(());
        self.worker
            .join()
            .map_err(|_| CommonplaceError::Graph("graph query timer failed".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::NamedNode;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn collector_consumes_only_limit_plus_one_native_solutions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native");
        let store = Store::open(&path).unwrap();
        store.flush().unwrap();
        drop(store);
        let store = Store::open_read_only(path).unwrap();
        for limit in [0, 1, 2, 3, 4] {
            let count = Arc::new(AtomicUsize::new(0));
            let observed = Arc::clone(&count);
            // Instrumentation is test-only; the production evaluator registers no functions.
            let results = SparqlEvaluator::new()
                .with_custom_function(NamedNode::new("urn:test:count").unwrap(), move |args| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    args.first().cloned()
                })
                .parse_query("SELECT (<urn:test:count>(?x) AS ?n) WHERE {VALUES ?x {1 2 3 4}}")
                .unwrap()
                .on_store(&store)
                .execute()
                .unwrap();
            let result = collect(results, limit).unwrap();
            assert_eq!(result.rows.len(), limit.min(4));
            assert_eq!(count.load(Ordering::SeqCst), (limit + 1).min(4));
            assert_eq!(result.truncated, limit < 4);
        }
    }

    #[test]
    fn completed_and_expired_timers_join_promptly() {
        let start = std::time::Instant::now();
        let token = CancellationToken::new();
        Deadline::start(token.clone(), Duration::from_secs(30))
            .unwrap()
            .finish()
            .unwrap();
        assert!(!token.is_cancelled());
        assert!(start.elapsed() < Duration::from_secs(1));
        let token = CancellationToken::new();
        let deadline = Deadline::start(token.clone(), Duration::from_millis(1)).unwrap();
        let start = std::time::Instant::now();
        while !token.is_cancelled() {
            assert!(start.elapsed() < Duration::from_secs(2));
            thread::sleep(Duration::from_millis(1));
        }
        deadline.finish().unwrap();
    }
}
