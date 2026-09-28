use std::path::{Path, PathBuf};

use fastembed::{
    InitOptionsUserDefined, Pooling, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel,
};
use hf_hub::{Cache, Repo, RepoType, api::sync::ApiBuilder};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::{CommonplaceError, Result};

pub const EMBEDDING_DIMENSIONS: usize = 384;
pub const EMBEDDING_REVISION: &str = "8f518e882455312b086101e60691f5e6e2f05c3c";
pub const EMBEDDING_REPOSITORY: &str = "Qdrant/all-MiniLM-L6-v2-onnx";
pub const EMBEDDING_IDENTITY: &str =
    "Qdrant/all-MiniLM-L6-v2-onnx@8f518e882455312b086101e60691f5e6e2f05c3c:mean:256:384:unit";

pub trait EmbeddingModel {
    fn identity(&self) -> &'static str;
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}

pub struct LocalEmbeddingModel {
    cache: Option<PathBuf>,
    maximum_batch_size: usize,
    session: Option<std::result::Result<TextEmbedding, String>>,
}

impl LocalEmbeddingModel {
    pub fn new(cache: Option<PathBuf>, maximum_batch_size: usize) -> Self {
        Self {
            cache,
            maximum_batch_size,
            session: None,
        }
    }

    fn load(&self) -> Result<TextEmbedding> {
        let hub_cache = self.cache.is_none().then(Cache::from_env);
        let artifact = |name, hash, sha256| {
            let path = match (&self.cache, &hub_cache) {
                (Some(cache), _) => cache.join(EMBEDDING_REVISION).join(name),
                (_, Some(cache)) => hub_artifact(cache, name)?,
                _ => {
                    return Err(CommonplaceError::ModelUnavailable(
                        "model cache is unavailable".into(),
                    ));
                }
            };
            verified(&path, hash, sha256)
        };
        let tokenizer = TokenizerFiles {
            tokenizer_file: artifact(
                "tokenizer.json",
                "20cb6e457867e03c181180a296809275d3e40a7d",
                false,
            )?,
            config_file: artifact(
                "config.json",
                "56c8c186de9040d4fea8daac2ca110f9d412bf04",
                false,
            )?,
            special_tokens_map_file: artifact(
                "special_tokens_map.json",
                "9bbecc17cabbcbd3112c14d6982b51403b264bfa",
                false,
            )?,
            tokenizer_config_file: artifact(
                "tokenizer_config.json",
                "954add25cc0792a2226b11d8ed5c9c39578084f0",
                false,
            )?,
        };
        let weights = artifact(
            "model.onnx",
            "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5",
            true,
        )?;
        TextEmbedding::try_new_from_user_defined(
            UserDefinedEmbeddingModel::new(weights, tokenizer).with_pooling(Pooling::Mean),
            InitOptionsUserDefined::new()
                .with_max_length(256)
                .with_intra_threads(2),
        )
        .map_err(|error| {
            CommonplaceError::ModelUnavailable(format!(
                "cannot load pinned local embedding: {error}"
            ))
        })
    }
}

impl EmbeddingModel for LocalEmbeddingModel {
    fn identity(&self) -> &'static str {
        EMBEDDING_IDENTITY
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if self.maximum_batch_size == 0 || texts.len() > self.maximum_batch_size {
            return Err(CommonplaceError::LimitExceeded(format!(
                "embedding batch exceeds the {}-passage limit",
                self.maximum_batch_size
            )));
        }
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        if self.session.is_none() {
            self.session = Some(self.load().map_err(|error| error.to_string()));
        }
        let model = match self.session.as_mut() {
            Some(Ok(model)) => model,
            Some(Err(message)) => return Err(CommonplaceError::ModelUnavailable(message.clone())),
            None => {
                return Err(CommonplaceError::ModelUnavailable(
                    "embedding session was not initialized".into(),
                ));
            }
        };
        let vectors = model
            .embed(texts, Some(self.maximum_batch_size))
            .map_err(|error| {
                CommonplaceError::ModelUnavailable(format!("local embedding failed: {error}"))
            })?;
        validate_vectors(&vectors, texts.len())?;
        Ok(vectors)
    }
}

pub fn validate_vectors(vectors: &[Vec<f32>], expected: usize) -> Result<()> {
    if vectors.len() != expected
        || vectors.iter().any(|vector| {
            vector.len() != EMBEDDING_DIMENSIONS
                || vector.iter().any(|value| !value.is_finite())
                || (vector
                    .iter()
                    .map(|value| f64::from(*value).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    - 1.0)
                    .abs()
                    > 0.001
        })
    {
        return Err(CommonplaceError::ModelUnavailable(
            "embedding output must contain one finite normalized 384-dimensional vector per passage".into(),
        ));
    }
    Ok(())
}

fn hub_artifact(cache: &Cache, name: &str) -> Result<PathBuf> {
    let repo = Repo::with_revision(
        EMBEDDING_REPOSITORY.into(),
        RepoType::Model,
        EMBEDDING_REVISION.into(),
    );
    if let Some(path) = cache.repo(repo.clone()).get(name) {
        return Ok(path);
    }
    // Public immutable artifacts need no credentials. Cache hits never make a request.
    ApiBuilder::from_cache(cache.clone()).with_token(None).with_progress(false).build()
        .and_then(|api| api.repo(repo).get(name))
        .map_err(|error| CommonplaceError::ModelUnavailable(format!(
            "cannot acquire {EMBEDDING_REPOSITORY}@{EMBEDDING_REVISION}/{name} from https://huggingface.co: {error}; check network access or use a prepared COMMONPLACE_MODEL_CACHE"
        )))
}

fn verified(path: &Path, expected: &str, sha256: bool) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path).map_err(|error| {
        CommonplaceError::ModelUnavailable(format!(
            "cannot read pinned model artifact {}: {error}; prepare the exact pinned cache",
            path.display()
        ))
    })?;
    let actual = if sha256 {
        format!("{:x}", Sha256::digest(&bytes))
    } else {
        let mut hash = Sha1::new();
        hash.update(format!("blob {}\0", bytes.len()));
        hash.update(&bytes);
        format!("{:x}", hash.finalize())
    };
    if actual != expected {
        return Err(CommonplaceError::ModelUnavailable(format!(
            "incompatible pinned model artifact {}; expected {expected}, found {actual}",
            path.display()
        )));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_failure_is_retained_and_bounds_do_not_load() {
        let cache = tempfile::tempdir().unwrap();
        let mut model = LocalEmbeddingModel::new(Some(cache.path().into()), 2);
        assert!(model.embed(&[]).unwrap().is_empty());
        assert!(model.session.is_none());
        assert_eq!(
            model.embed(&["a", "b", "c"]).unwrap_err().code(),
            "limit_exceeded"
        );
        assert!(model.session.is_none());
        let first = model.embed(&["a"]).unwrap_err().to_string();
        assert!(matches!(model.session, Some(Err(_))));
        model.cache = Some(PathBuf::from("different-missing-cache"));
        assert_eq!(model.embed(&["b"]).unwrap_err().to_string(), first);
    }

    #[test]
    fn stock_cache_hit_is_offline_and_still_verified() {
        let temp = tempfile::tempdir().unwrap();
        let cache = Cache::new(temp.path().into());
        let repo = cache.repo(Repo::with_revision(
            EMBEDDING_REPOSITORY.into(),
            RepoType::Model,
            EMBEDDING_REVISION.into(),
        ));
        repo.create_ref(EMBEDDING_REVISION).unwrap();
        let directory = repo.pointer_path(EMBEDDING_REVISION);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("tokenizer.json");
        std::fs::write(&path, b"corrupt").unwrap();
        assert_eq!(hub_artifact(&cache, "tokenizer.json").unwrap(), path);
        assert!(
            verified(&path, "20cb6e457867e03c181180a296809275d3e40a7d", false)
                .unwrap_err()
                .to_string()
                .contains("incompatible")
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
    }

    #[test]
    fn corrupt_cache_and_malformed_vectors_fail() {
        let cache = tempfile::tempdir().unwrap();
        std::fs::create_dir(cache.path().join(EMBEDDING_REVISION)).unwrap();
        std::fs::write(
            cache.path().join(EMBEDDING_REVISION).join("tokenizer.json"),
            b"{}",
        )
        .unwrap();
        let mut model = LocalEmbeddingModel::new(Some(cache.path().into()), 1);
        assert!(
            model
                .embed(&["a"])
                .unwrap_err()
                .to_string()
                .contains("incompatible")
        );
        let mut unit = vec![0.0; EMBEDDING_DIMENSIONS];
        unit[0] = 1.0;
        validate_vectors(&[unit.clone()], 1).unwrap();
        assert!(validate_vectors(&[], 1).is_err());
        assert!(validate_vectors(&[vec![1.0]], 1).is_err());
        assert!(validate_vectors(&[vec![0.0; EMBEDDING_DIMENSIONS]], 1).is_err());
        unit[1] = f32::NAN;
        assert!(validate_vectors(&[unit], 1).is_err());
    }
}
