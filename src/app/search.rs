use std::path::Path;

use crate::domain::search::{
    RERANK_LIMIT, SearchItem, SearchRequest, SearchResult, TemporalDiagnostic, TemporalFilter, fuse,
};
use crate::providers::embeddings::{EMBEDDING_IDENTITY, EmbeddingModel, validate_vectors};
use crate::providers::reranker::Reranker;
use crate::storage::{database::SqliteDatabase, evidence, search};
use crate::{CommonplaceError, Result};

pub fn search(
    root: &Path,
    request: &SearchRequest,
    embedding: &mut impl EmbeddingModel,
    reranker: &mut impl Reranker,
) -> Result<SearchResult> {
    let filters = request.validate()?;
    let session = SqliteDatabase::read(root)?;
    let connection = session.connection();
    search::validate_representation(connection)?;
    let temporal_filter = (filters.since.is_some() || filters.until.is_some()).then(|| {
        let coverage = search::temporal_coverage(connection, &filters)?;
        let diagnostics = if coverage.excluded_unknown == 0 {
            Vec::new()
        } else {
            vec![TemporalDiagnostic {
                code: "unknown_dates_excluded",
                message: "Sources with unknown event dates are excluded by the date window; results do not cover those sources.",
            }]
        };
        Ok::<_, CommonplaceError>(TemporalFilter {
            since: filters.since.clone(),
            until: filters.until.clone(),
            includes_timeless: true,
            coverage,
            diagnostics,
        })
    }).transpose()?;
    if embedding.identity() != EMBEDDING_IDENTITY {
        return Err(CommonplaceError::ModelUnavailable(
            "search requires the store's pinned embedding identity".into(),
        ));
    }
    let vectors = embedding.embed(&[&request.query])?;
    validate_vectors(&vectors, 1)?;
    let lexical = search::lexical(connection, &request.lexical_query(), &filters)?;
    let vector = search::vector(connection, &vectors[0], &filters)?;
    let mut candidates = fuse(&lexical.ids, &vector.ids);
    let scope = request.scope.as_ref().map(|selection| {
        let mut coverage = search::scope_coverage(connection, &filters)?;
        coverage.supplied_ids = selection.document_ids.len();
        coverage.duplicate_ids = coverage.supplied_ids - coverage.selected_sources;
        coverage.selection_truncated = selection.truncated;
        coverage.lexical_truncated = lexical.truncated;
        coverage.vector_truncated = vector.truncated;
        coverage.fusion_truncated = candidates.len() > RERANK_LIMIT;
        coverage.result_truncated = candidates.len().min(RERANK_LIMIT) > request.limit;
        coverage.diagnostics.push(TemporalDiagnostic {
            code: "selected_scope_only",
            message: "Results cover only the selected documents, not all corpus evidence; retrieval is bounded, not exhaustive.",
        });
        if selection.truncated {
            coverage.diagnostics.push(TemporalDiagnostic {
                code: "selection_truncated",
                message: "The graph selection was truncated; omitted documents were not searched.",
            });
        }
        if !coverage.missing_document_ids.is_empty() {
            coverage.diagnostics.push(TemporalDiagnostic {
                code: "missing_documents",
                message: "Selected document IDs are missing or removed; they were excluded, not replaced.",
            });
        }
        if coverage.selected_sources == 0 {
            coverage.diagnostics.push(TemporalDiagnostic {
                code: "empty_scope",
                message: "The explicit document selection is empty; no corpus-wide search was performed.",
            });
        } else if coverage.eligible_passages == 0 {
            coverage.diagnostics.push(TemporalDiagnostic {
                code: "no_eligible_passages",
                message: "No current passages in this scope satisfy the filters; this is not corpus-wide absence.",
            });
        } else if candidates.is_empty() {
            coverage.diagnostics.push(TemporalDiagnostic {
                code: "no_candidates_in_scope",
                message: "No candidates were found in this scope; this is not corpus-wide absence.",
            });
        }
        Ok::<_, CommonplaceError>(coverage)
    }).transpose()?;
    let truncated = lexical.truncated
        || vector.truncated
        || candidates.len() > RERANK_LIMIT
        || candidates.len() > request.limit;
    candidates.truncate(RERANK_LIMIT);
    let passages = candidates
        .into_iter()
        .map(|id| evidence::passage(connection, id))
        .collect::<Result<Vec<_>>>()?;
    let texts: Vec<_> = passages
        .iter()
        .map(|passage| passage.text.as_str())
        .collect();
    let scores = reranker.rerank(&request.query, &texts)?;
    if scores.len() != passages.len() || scores.iter().any(|score| !score.is_finite()) {
        return Err(CommonplaceError::ModelUnavailable(
            "reranker must return exactly one finite score per candidate".into(),
        ));
    }
    let mut ranked: Vec<_> = passages.into_iter().zip(scores).enumerate().collect();
    ranked.sort_by(|(a_order, (_, a_score)), (b_order, (_, b_score))| {
        b_score.total_cmp(a_score).then(a_order.cmp(b_order))
    });
    let items = ranked
        .into_iter()
        .take(request.limit)
        .enumerate()
        .map(|(index, (_, (evidence, _)))| SearchItem {
            rank: index + 1,
            evidence,
        })
        .collect();
    Ok(SearchResult {
        items,
        truncated,
        temporal_filter,
        scope,
    })
}
