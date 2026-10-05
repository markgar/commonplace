use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::documents::{Evidence, normalize_timestamp};
use super::ids::{DocumentId, PassageId, RevisionId};
use crate::{CommonplaceError, Result};

pub const CANDIDATE_LIMIT: usize = 64;
pub const RERANK_LIMIT: usize = 64;
pub const DEFAULT_RESULT_LIMIT: usize = 10;
pub const MAX_RESULT_LIMIT: usize = 50;
const FUSION_CONSTANT: f64 = 60.0;
pub const MAX_SCOPE_IDS: usize = 1024;
pub const MAX_SCOPE_BYTES: usize = 65536;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentScope {
    #[schemars(length(max = 1024))]
    #[schemars(with = "Vec<ScopeId>")]
    pub document_ids: Vec<String>,
    pub truncated: bool,
}

#[derive(JsonSchema)]
#[serde(transparent)]
pub struct ScopeId(#[schemars(regex(pattern = "^doc:[1-9][0-9]*$"))] pub String);

impl DocumentScope {
    pub fn validate(&self) -> Result<Vec<DocumentId>> {
        if self.document_ids.len() > MAX_SCOPE_IDS {
            return Err(CommonplaceError::LimitExceeded(format!(
                "scope exceeds {MAX_SCOPE_IDS} supplied document IDs"
            )));
        }
        let mut ids = self
            .document_ids
            .iter()
            .map(|value| canonical_document_id(value))
            .collect::<Result<Vec<_>>>()?;
        ids.sort();
        ids.dedup();
        Ok(ids)
    }
}

pub fn canonical_document_id(value: &str) -> Result<DocumentId> {
    let id: DocumentId = value.parse()?;
    if id.to_string() != value {
        return Err(CommonplaceError::InvalidInput(format!(
            "noncanonical document ID {value:?}; expected {id}"
        )));
    }
    Ok(id)
}

#[derive(Debug)]
pub struct SearchRequest {
    pub query: String,
    pub must_contain: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub scope: Option<DocumentScope>,
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
        let until = self
            .until
            .as_deref()
            .map(|value| {
                normalize_timestamp(value).map_err(|error| {
                    CommonplaceError::InvalidInput(format!("invalid --until: {error}"))
                })
            })
            .transpose()?;
        if let (Some(since), Some(until)) = (&since, &until)
            && since.trim_end_matches('Z') > until.trim_end_matches('Z')
        {
            return Err(CommonplaceError::InvalidInput(
                "--since must not be after --until".into(),
            ));
        }
        let mut source_types = self.source_types.clone();
        source_types.sort();
        source_types.dedup();
        Ok(SearchFilters {
            must_contain: self.must_contain.as_deref().map(str::to_lowercase),
            since,
            until,
            document_ids: self
                .scope
                .as_ref()
                .map(DocumentScope::validate)
                .transpose()?,
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
    pub until: Option<String>,
    pub document_ids: Option<Vec<DocumentId>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeCoverage>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TemporalCoverage {
    pub eligible_dated: usize,
    pub timeless: usize,
    pub older_dated: usize,
    pub newer_dated: usize,
    pub excluded_unknown: usize,
}

#[derive(Debug, Serialize)]
pub struct TemporalDiagnostic {
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Debug, Serialize)]
pub struct TemporalFilter {
    pub since: Option<String>,
    pub until: Option<String>,
    pub includes_timeless: bool,
    pub coverage: TemporalCoverage,
    pub diagnostics: Vec<TemporalDiagnostic>,
}

#[derive(Debug, Serialize)]
pub struct ScopeCoverage {
    pub supplied_ids: usize,
    pub duplicate_ids: usize,
    pub selected_sources: usize,
    pub missing_document_ids: Vec<DocumentId>,
    pub existing_sources: usize,
    pub excluded_source_type: usize,
    pub eligible_sources: usize,
    pub eligible_passages: usize,
    pub selection_truncated: bool,
    pub lexical_truncated: bool,
    pub vector_truncated: bool,
    pub fusion_truncated: bool,
    pub result_truncated: bool,
    pub diagnostics: Vec<TemporalDiagnostic>,
}

#[derive(Debug, Serialize)]
pub struct GroupedSearchResult {
    pub groups: Vec<SourceGroup>,
    pub truncated: bool,
    pub temporal_filter: Option<TemporalFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeCoverage>,
}

#[derive(Debug, Serialize)]
pub struct SourceGroup {
    pub document_id: DocumentId,
    pub revision_id: RevisionId,
    pub source_key: String,
    pub title: Option<String>,
    pub source_type: String,
    pub temporal_state: super::documents::TemporalState,
    pub occurred_at: Option<String>,
    pub passages: Vec<GroupedPassage>,
}

#[derive(Debug, Serialize)]
pub struct GroupedPassage {
    pub passage_id: PassageId,
    pub rank: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub text: String,
}

impl SearchResult {
    pub fn grouped(self) -> GroupedSearchResult {
        let mut groups: Vec<SourceGroup> = Vec::new();
        for item in self.items {
            let evidence = item.evidence;
            let passage = GroupedPassage {
                passage_id: evidence.passage_id,
                rank: item.rank,
                start_byte: evidence.start_byte,
                end_byte: evidence.end_byte,
                text: evidence.text,
            };
            if let Some(group) = groups
                .iter_mut()
                .find(|group| group.document_id == evidence.document_id)
            {
                group.passages.push(passage);
            } else {
                groups.push(SourceGroup {
                    document_id: evidence.document_id,
                    revision_id: evidence.revision_id,
                    source_key: evidence.source_key,
                    title: evidence.title,
                    source_type: evidence.source_type,
                    temporal_state: evidence.temporal_state,
                    occurred_at: evidence.occurred_at,
                    passages: vec![passage],
                });
            }
        }
        GroupedSearchResult {
            groups,
            truncated: self.truncated,
            temporal_filter: self.temporal_filter,
            scope: self.scope,
        }
    }
}
