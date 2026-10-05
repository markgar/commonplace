use std::collections::BTreeMap;

use serde::Serialize;

use super::documents::{Evidence, normalize_timestamp};
use super::ids::PassageId;
use crate::{CommonplaceError, Result};

pub const CANDIDATE_LIMIT: usize = 64;
pub const RERANK_LIMIT: usize = 64;
pub const DEFAULT_RESULT_LIMIT: usize = 10;
pub const MAX_RESULT_LIMIT: usize = 50;
const FUSION_CONSTANT: f64 = 60.0;

#[derive(Debug)]
pub struct SearchRequest {
    pub query: String,
    pub must_contain: Option<String>,
    pub since: Option<String>,
    pub source_types: Vec<String>,
    pub limit: usize,
}

impl SearchRequest {
    pub fn validate(&self) -> Result<SearchFilters> {
        if self.query.trim().is_empty() || self.query.contains('\0') {
            return Err(CommonplaceError::InvalidInput(
                "search query must be nonblank and contain no NUL".into(),
            ));
        }
        if self.query.len() > 4096 || self.query.split_whitespace().count() > 64 {
            return Err(CommonplaceError::LimitExceeded(
                "search query exceeds 4096 UTF-8 bytes or 64 whitespace-delimited terms".into(),
            ));
        }
        if let Some(phrase) = &self.must_contain {
            if phrase.trim().is_empty() || phrase.contains('\0') {
                return Err(CommonplaceError::InvalidInput(
                    "search --must-contain must be nonblank and contain no NUL".into(),
                ));
            }
            if phrase.len() > 4096 || phrase.split_whitespace().count() > 64 {
                return Err(CommonplaceError::LimitExceeded(
                    "search --must-contain exceeds 4096 UTF-8 bytes or 64 whitespace-delimited terms".into(),
                ));
            }
        }
        if self.limit > MAX_RESULT_LIMIT {
            return Err(CommonplaceError::LimitExceeded(format!(
                "search --limit must be between 0 and {MAX_RESULT_LIMIT}"
            )));
        }
        if self.source_types.len() > 32
            || self
                .source_types
                .iter()
                .try_fold(0usize, |sum, value| sum.checked_add(value.len()))
                .is_none_or(|size| size > 4096)
        {
            return Err(CommonplaceError::LimitExceeded(
                "search source-type filters exceed 32 values or 4096 UTF-8 bytes".into(),
            ));
        }
        if self
            .source_types
            .iter()
            .any(|value| value.is_empty() || value.contains('\0'))
        {
            return Err(CommonplaceError::InvalidInput(
                "search source-type filters must be nonempty and contain no NUL".into(),
            ));
        }
        let since = self
            .since
            .as_deref()
            .map(|value| {
                normalize_timestamp(value).map_err(|error| {
                    CommonplaceError::InvalidInput(format!("invalid --since: {error}"))
                })
            })
            .transpose()?;
        let mut source_types = self.source_types.clone();
        source_types.sort();
        source_types.dedup();
        Ok(SearchFilters {
            must_contain: self.must_contain.as_deref().map(str::to_lowercase),
            since,
            source_types,
        })
    }

    pub fn lexical_query(&self) -> String {
        self.query
            .split_whitespace()
            .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR ")
    }
}

#[derive(Debug)]
pub struct SearchFilters {
    pub must_contain: Option<String>,
    pub since: Option<String>,
    pub source_types: Vec<String>,
}

#[derive(Debug)]
pub struct Candidates {
    pub ids: Vec<PassageId>,
    pub truncated: bool,
}

pub fn fuse(lexical: &[PassageId], vector: &[PassageId]) -> Vec<PassageId> {
    let mut scores = BTreeMap::new();
    for candidates in [lexical, vector] {
        for (index, id) in candidates.iter().enumerate() {
            *scores.entry(*id).or_insert(0.0) += 1.0 / (FUSION_CONSTANT + (index + 1) as f64);
        }
    }
    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|(a_id, a_score), (b_id, b_score)| {
        b_score.total_cmp(a_score).then(a_id.cmp(b_id))
    });
    ranked.into_iter().map(|(id, _)| id).collect()
}

#[derive(Debug, Serialize)]
pub struct SearchItem {
    pub rank: usize,
    #[serde(flatten)]
    pub evidence: Evidence,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub items: Vec<SearchItem>,
    pub truncated: bool,
    pub temporal_filter: Option<TemporalFilter>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TemporalCoverage {
    pub eligible_dated: usize,
    pub timeless: usize,
    pub older_dated: usize,
    pub excluded_unknown: usize,
}

#[derive(Debug, Serialize)]
pub struct TemporalDiagnostic {
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Debug, Serialize)]
pub struct TemporalFilter {
    pub since: String,
    pub includes_timeless: bool,
    pub coverage: TemporalCoverage,
    pub diagnostics: Vec<TemporalDiagnostic>,
}
