use std::path::Path;

use crate::domain::search::{RERANK_LIMIT, SearchItem, SearchRequest, SearchResult, fuse};
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
    Ok(SearchResult { items, truncated })
}
