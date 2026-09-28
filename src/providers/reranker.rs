use std::path::PathBuf;

use fastembed::{
    RerankInitOptionsUserDefined, TextRerank, TokenizerFiles, UserDefinedRerankingModel,
};

use crate::domain::search::RERANK_LIMIT;
use crate::{CommonplaceError, Result};

pub const RERANKER_REPOSITORY: &str = "jinaai/jina-reranker-v1-turbo-en";
pub const RERANKER_REVISION: &str = "b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2";
pub const RERANK_BATCH_SIZE: usize = 8;

pub trait Reranker {
    /// Scores correspond to input order. Empty input still verifies model availability.
    fn rerank(&mut self, query: &str, texts: &[&str]) -> Result<Vec<f32>>;
}

pub struct LocalReranker {
    cache: Option<PathBuf>,
    session: Option<std::result::Result<TextRerank, String>>,
}

impl LocalReranker {
    pub fn new(cache: Option<PathBuf>) -> Self {
        Self {
            cache,
            session: None,
        }
    }

    fn load(&self) -> Result<TextRerank> {
        let artifact = |name, hash, sha256| {
            super::artifacts::artifact(
                self.cache.as_deref(),
                RERANKER_REPOSITORY,
                RERANKER_REVISION,
                name,
                hash,
                sha256,
            )
        };
        let tokenizer = TokenizerFiles {
            tokenizer_file: artifact(
                "tokenizer.json",
                "61287c716716abd7a3096ff2f74a1de6e20d589b",
                false,
            )?,
            config_file: artifact(
                "config.json",
                "1febe623f0d5566bf1dd91fb0641a29e122ee6e3",
                false,
            )?,
            special_tokens_map_file: artifact(
                "special_tokens_map.json",
                "d5698132694f4f1bcff08fa7d937b1701812598e",
                false,
            )?,
            tokenizer_config_file: artifact(
                "tokenizer_config.json",
                "742b5d2b49f1a9222693ca4295f3226716f2f2bc",
                false,
            )?,
        };
        let weights = artifact(
            "onnx/model.onnx",
            "c1296c66c119de645fa9cdee536d8637740efe85224cfa270281e50f213aa565",
            true,
        )?;
        TextRerank::try_new_from_user_defined(
            UserDefinedRerankingModel::new(weights, tokenizer),
            RerankInitOptionsUserDefined::new()
                .with_max_length(512)
                .with_intra_threads(2),
        )
        .map_err(|error| {
            CommonplaceError::ModelUnavailable(format!(
                "cannot load pinned local reranker: {error}"
            ))
        })
    }
}

impl Reranker for LocalReranker {
    fn rerank(&mut self, query: &str, texts: &[&str]) -> Result<Vec<f32>> {
        if texts.len() > RERANK_LIMIT {
            return Err(CommonplaceError::LimitExceeded(format!(
                "reranking exceeds {RERANK_LIMIT} passages"
            )));
        }
        if self.session.is_none() {
            self.session = Some(self.load().map_err(|error| error.to_string()));
        }
        let model = match self.session.as_mut() {
            Some(Ok(model)) => model,
            Some(Err(message)) => return Err(CommonplaceError::ModelUnavailable(message.clone())),
            None => {
                return Err(CommonplaceError::ModelUnavailable(
                    "reranker session was not initialized".into(),
                ));
            }
        };
        let mut scores = Vec::with_capacity(texts.len());
        for batch in texts.chunks(RERANK_BATCH_SIZE) {
            let ranked = model
                .rerank(query, batch, false, Some(RERANK_BATCH_SIZE))
                .map_err(|error| {
                    CommonplaceError::ModelUnavailable(format!("local reranking failed: {error}"))
                })?;
            scores.extend(ordered_scores(
                batch.len(),
                ranked
                    .into_iter()
                    .map(|result| (result.index, result.score)),
            )?);
        }
        Ok(scores)
    }
}

fn ordered_scores(
    count: usize,
    ranked: impl IntoIterator<Item = (usize, f32)>,
) -> Result<Vec<f32>> {
    let mut ordered = vec![None; count];
    for (index, score) in ranked {
        let slot = ordered.get_mut(index).ok_or_else(invalid_scores)?;
        if !score.is_finite() || slot.replace(score).is_some() {
            return Err(invalid_scores());
        }
    }
    ordered
        .into_iter()
        .map(|score| score.ok_or_else(invalid_scores))
        .collect()
}

fn invalid_scores() -> CommonplaceError {
    CommonplaceError::ModelUnavailable(
        "reranker must return exactly one finite score per candidate".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_outputs_must_cover_each_input_exactly_once() {
        assert_eq!(
            ordered_scores(2, [(1, 0.5), (0, -0.5)]).unwrap(),
            [-0.5, 0.5]
        );
        for ranked in [
            vec![(0, 1.0)],
            vec![(0, 1.0), (0, 2.0)],
            vec![(0, 1.0), (2, 2.0)],
            vec![(0, f32::NAN), (1, 2.0)],
            vec![(0, 1.0), (1, f32::INFINITY)],
        ] {
            assert_eq!(
                ordered_scores(2, ranked).unwrap_err().code(),
                "model_unavailable"
            );
        }
    }

    #[test]
    fn bounds_lazy_loading_and_retained_empty_input_failure() {
        let cache = tempfile::tempdir().unwrap();
        let mut model = LocalReranker::new(Some(cache.path().into()));
        assert!(model.session.is_none());
        assert_eq!(
            model
                .rerank("query", &vec!["x"; RERANK_LIMIT + 1])
                .unwrap_err()
                .code(),
            "limit_exceeded"
        );
        assert!(model.session.is_none());
        let first = model.rerank("query", &[]).unwrap_err().to_string();
        assert!(first.contains(RERANKER_REVISION));
        model.cache = Some("another-missing-directory".into());
        assert_eq!(
            model.rerank("query", &["x"]).unwrap_err().to_string(),
            first
        );
    }
}
