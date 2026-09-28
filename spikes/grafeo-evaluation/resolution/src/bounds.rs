use anyhow::{Result, ensure};
use grafeo_common::types::LogicalType;
use grafeo_core::execution::{
    DataChunk,
    operators::{Operator, OperatorResult},
};
use grafeo_engine::query::executor::Executor;
use serde_json::json;

struct CountedSource {
    pulls: usize,
    chunk_size: usize,
}

impl Operator for CountedSource {
    fn next(&mut self) -> OperatorResult {
        self.pulls += 1;
        assert!(self.pulls <= 3, "executor pulled after its result budget");
        let mut chunk = DataChunk::with_capacity(&[LogicalType::Int64], self.chunk_size);
        for value in 0..self.chunk_size {
            chunk.column_mut(0).unwrap().push_int64(value as i64);
        }
        chunk.set_count(self.chunk_size);
        Ok(Some(chunk))
    }
    fn reset(&mut self) {
        self.pulls = 0;
    }
    fn name(&self) -> &'static str {
        "CountedSource"
    }
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any + Send> {
        self
    }
}

pub fn probe() -> Result<serde_json::Value> {
    let mut cases = Vec::new();
    for (chunk_size, cap, expected_pulls) in [(1, 3, 3), (1024, 3, 1), (1024, 0, 0)] {
        let mut source = CountedSource {
            pulls: 0,
            chunk_size,
        };
        let result = Executor::new().execute_with_limit(&mut source, cap)?;
        ensure!(result.row_count() == cap);
        ensure!(source.pulls == expected_pulls);
        cases.push(
            json!({"chunk_size":chunk_size, "cap":cap, "pulls":source.pulls,
            "materialized_rows":result.row_count()}),
        );
    }
    Ok(json!(cases))
}
