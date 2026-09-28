use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use fastembed::{
    InitOptionsUserDefined, Pooling, RerankInitOptionsUserDefined, TextEmbedding, TextRerank,
    TokenizerFiles, UserDefinedEmbeddingModel, UserDefinedRerankingModel,
};
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::{Digest, Sha256};

const EMBEDDING: &str = "8f518e882455312b086101e60691f5e6e2f05c3c";
const RERANKER: &str = "b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2";
const BATCH: usize = 2;

fn verified(cache: &Path, revision: &str, name: &str) -> Result<Vec<u8>> {
    let manifest: Value = serde_json::from_str(include_str!("../models.json"))?;
    let entry = manifest
        .as_array()
        .context("invalid model manifest")?
        .iter()
        .find(|entry| entry["revision"] == revision)
        .context("unknown model revision")?;
    let file = entry["files"]
        .as_array()
        .context("invalid artifact manifest")?
        .iter()
        .find(|file| file[0] == name)
        .context("unknown artifact")?;
    let path = cache.join(revision).join(name);
    let bytes = std::fs::read(&path)
        .with_context(|| format!("missing model artifact: {}", path.display()))?;
    let actual = match file[1].as_str() {
        Some("sha256") => format!("{:x}", Sha256::digest(&bytes)),
        Some("git-blob-sha1") => {
            let mut hash = Sha1::new();
            hash.update(format!("blob {}\0", bytes.len()));
            hash.update(&bytes);
            format!("{:x}", hash.finalize())
        }
        _ => bail!("unsupported artifact hash"),
    };
    ensure!(
        Some(actual.as_str()) == file[2].as_str(),
        "incompatible model artifact: {}",
        path.display()
    );
    Ok(bytes)
}

fn tokenizer(cache: &Path, revision: &str) -> Result<TokenizerFiles> {
    Ok(TokenizerFiles {
        tokenizer_file: verified(cache, revision, "tokenizer.json")?,
        config_file: verified(cache, revision, "config.json")?,
        special_tokens_map_file: verified(cache, revision, "special_tokens_map.json")?,
        tokenizer_config_file: verified(cache, revision, "tokenizer_config.json")?,
    })
}

pub fn probe(root: &Path) -> Result<Value> {
    let cache = std::env::var_os("COMMONPLACE_MODEL_CACHE")
        .map(PathBuf::from)
        .context(
            "set COMMONPLACE_MODEL_CACHE to a prepared immutable cache; inference never downloads",
        )?;
    let mut embedding = TextEmbedding::try_new_from_user_defined(
        UserDefinedEmbeddingModel::new(
            verified(&cache, EMBEDDING, "model.onnx")?,
            tokenizer(&cache, EMBEDDING)?,
        )
        .with_pooling(Pooling::Mean),
        InitOptionsUserDefined::new()
            .with_max_length(256)
            .with_intra_threads(2),
    )
    .context("initialize pinned local embedding")?;
    let texts = [
        "release planning notes",
        "incident response timeline",
        "project dependency",
    ];
    let mut vectors = Vec::new();
    let mut batch_lengths = Vec::new();
    for batch in texts.chunks(BATCH) {
        batch_lengths.push(batch.len());
        vectors.extend(embedding.embed(batch, Some(BATCH))?);
    }
    ensure!(
        vectors.len() == 3 && vectors.iter().all(|v| v.len() == 384),
        "embedding shape mismatch"
    );
    let norms: Vec<f32> = vectors
        .iter()
        .map(|v| v.iter().map(|x| x * x).sum::<f32>().sqrt())
        .collect();
    ensure!(
        norms
            .iter()
            .all(|n| n.is_finite() && (n - 1.0).abs() < 0.001),
        "normalization failure"
    );
    let repeated = embedding.embed(vec![texts[0]], Some(BATCH))?;
    ensure!(
        vectors[0]
            .iter()
            .zip(&repeated[0])
            .all(|(a, b)| (a - b).abs() < 0.00001),
        "embedding changed between calls"
    );
    let mut reranker = TextRerank::try_new_from_user_defined(
        UserDefinedRerankingModel::new(
            verified(&cache, RERANKER, "onnx/model.onnx")?,
            tokenizer(&cache, RERANKER)?,
        ),
        RerankInitOptionsUserDefined::new()
            .with_max_length(512)
            .with_intra_threads(2),
    )
    .context("initialize pinned local reranker")?;
    let candidates = vec![
        "The release depends on the Atlas project.",
        "Lunch is scheduled for noon.",
    ];
    let ranked = reranker.rerank(
        "what depends on the project?",
        candidates.clone(),
        true,
        Some(BATCH),
    )?;
    let repeated_rank = reranker.rerank(
        "what depends on the project?",
        candidates,
        true,
        Some(BATCH),
    )?;
    ensure!(
        ranked.len() == 2 && ranked[0].index == 0,
        "reranking relevance failed"
    );
    ensure!(
        ranked == repeated_rank && ranked.iter().all(|r| r.score.is_finite()),
        "reranking is not deterministic"
    );
    let sqlite = crate::sqlite::probe(root, Some(&vectors))?;
    Ok(
        json!({"status":"pass", "embedding_revision":EMBEDDING, "reranker_revision":RERANKER,
        "dimensions":384, "norms":norms, "embedding_batches":batch_lengths,
        "reranker_batch":2, "reranker_top_index":ranked[0].index, "reranker_top_score":ranked[0].score,
        "repeat_calls_same_sessions":true, "artifact_hashes_verified":true, "sqlite":sqlite,
        "network_client_compiled":false, "cache":cache}),
    )
}
