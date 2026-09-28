use std::path::{Path, PathBuf};

use hf_hub::{Cache, Repo, RepoType, api::sync::ApiBuilder};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::{CommonplaceError, Result};

pub(super) fn artifact(
    offline_cache: Option<&Path>,
    repository: &str,
    revision: &str,
    name: &str,
    hash: &str,
    sha256: bool,
) -> Result<Vec<u8>> {
    let path = match offline_cache {
        Some(cache) => cache.join(revision).join(name),
        None => hub_artifact(&Cache::from_env(), repository, revision, name)?,
    };
    verified(&path, hash, sha256)
}

pub(super) fn hub_artifact(
    cache: &Cache,
    repository: &str,
    revision: &str,
    name: &str,
) -> Result<PathBuf> {
    let repo = Repo::with_revision(repository.into(), RepoType::Model, revision.into());
    if let Some(path) = cache.repo(repo.clone()).get(name) {
        return Ok(path);
    }
    // Public immutable artifacts need no credentials. Cache hits never make a request.
    ApiBuilder::from_cache(cache.clone()).with_token(None).with_progress(false).build()
        .and_then(|api| api.repo(repo).get(name))
        .map_err(|error| CommonplaceError::ModelUnavailable(format!(
            "cannot acquire {repository}@{revision}/{name} from https://huggingface.co: {error}; check network access or use a prepared COMMONPLACE_MODEL_CACHE"
        )))
}

pub(super) fn verified(path: &Path, expected: &str, sha256: bool) -> Result<Vec<u8>> {
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
