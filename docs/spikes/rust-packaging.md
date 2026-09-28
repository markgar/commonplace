# Rust packaging spike

## P2 local integrated preflight (2026-09-28 UTC)

**INFERENCE gate: PASS on macOS arm64 only. GRAPH gate: BLOCKED.** The current
harness replaces Ladybug with Grafeo 0.5.43; it does not change production code,
store formats, providers, or normative requirements. The graph blockers and
publication evidence are detailed in the [graph report](grafeo-evaluation.md).
Local inference can unblock P3 once P1 is integrated; this is not release
acceptance. No Actions, remote runners, workflow changes, or other machines were
used. Required baseline `84679a0702464561a3f3117699c92b735975957a` was verified
in branch ancestry before edits.

### Exact selections and artifacts

| Component | Current combined probe |
| --- | --- |
| Host | macOS 27.0 (26A428), arm64 |
| Toolchain | rustc 1.98.1 (48a229cea 2026-09-01), cargo 1.98.1 (797e8a9bc 2026-08-05) |
| Graph | Grafeo `=0.5.43`, defaults disabled, `lpg` |
| SQLite | rusqlite `=0.37.0`, bundled SQLite 3.50.2 / FTS5 |
| Vector | sqlite-vec `=0.1.6` |
| Models | fastembed `=7.1.0`, only `ort-download-binaries-rustls-tls` |
| Transitive runtime | `ort` and `ort-sys` 2.0.0-rc.13, locked in the spike Cargo.lock; native ONNX Runtime 1.28.0 distribution |
| Embedding | `Qdrant/all-MiniLM-L6-v2-onnx`, revision `8f518e882455312b086101e60691f5e6e2f05c3c`, mean pooling, max length 256, 384 dimensions |
| Reranker | `jinaai/jina-reranker-v1-turbo-en`, revision `b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2`, max length 512 |

Native ONNX Runtime's selected `ort-sys` distribution entry is
`https://cdn.pyke.io/0/pyke:ort-rs/ms@1.28.0/aarch64-apple-darwin+coreml.tar.lzma2`,
SHA-256 `6934874e2e953576d9c1db47ff1af39c62c4f4220dbe6f988e131f72879674c7`.
This entry identifies the native build distribution, not an additional inference
provider. This run used the available local build cache.

Final executable: `spikes/rust-packaging/target/release/rust-packaging`,
38,002,752 bytes, SHA-256
`a2fa9d073bc2b0ca6332fde878754a3adbcab01da213ed2e88f04ceb51f9fd36`.
The artifact hash is observed evidence, not a promise of byte-reproducible builds
across worktree paths/toolchains. `otool -L` showed only:

```text
/usr/lib/libc++.1.dylib
/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation
/System/Library/Frameworks/CoreML.framework/Versions/A/CoreML
/usr/lib/libiconv.2.dylib
/usr/lib/libSystem.B.dylib
/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation
/usr/lib/libobjc.A.dylib
```

No Ladybug or Homebrew OpenSSL remains in this executable. P2 did not run the
updated optional archive helpers or a fresh-machine package test.

### Immutable acquisition and offline inference

No usable cache was present at the spike/default worktree location. Explicit
`prepare-models.py` acquired the selected artifacts to `target/pinned-models`.
Every URL has this form, with literal revisions and filenames from the committed
[`models.json`](../../spikes/rust-packaging/models.json):

```text
https://huggingface.co/Qdrant/all-MiniLM-L6-v2-onnx/resolve/8f518e882455312b086101e60691f5e6e2f05c3c/<filename>
https://huggingface.co/jinaai/jina-reranker-v1-turbo-en/resolve/b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2/<filename>
```

Required files per model: `config.json`, `tokenizer.json`,
`tokenizer_config.json`, `special_tokens_map.json`, and respectively `model.onnx`
or `onnx/model.onnx`. ONNX SHA-256:

| Model | SHA-256 |
| --- | --- |
| Embedding | `bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5` |
| Reranker | `c1296c66c119de645fa9cdee536d8637740efe85224cfa270281e50f213aa565` |

Tokenizer/config identities use Git blob SHA-1
(`SHA1("blob " + byte_length + NUL + bytes)`), exactly matching the immutable
revision API's blob IDs recorded in `models.json`. Both preparation and runtime
validate all ten artifacts; no `refs/main` or remote custom Python code is used.
Model redirects used `us.aws.cdn.hf.co` and `cas-bridge.xethub-eu.hf.co`. Downloads
succeeded without changing endpoints or bypassing network policy. If those hosts
are blocked elsewhere, stop and report the exact missing artifact.

The cache is approximately 244 MiB on disk. Its shape is
`<cache>/<revision>/<repository-relative-filename>`. Preparation reuses matching
files and rejects incompatible files rather than replacing them. Runtime has no
Hugging Face client feature and feeds verified local bytes to
`try_new_from_user_defined`. ONNX executes locally using two intra-op threads.
Only synthetic fixed strings enter inference; no source documents leave the host.

### Reproduction and observed result

Commands are from the repository root:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
python3 spikes/rust-packaging/prepare-models.py \
  "$PWD/spikes/rust-packaging/target/pinned-models"
cargo build --manifest-path spikes/rust-packaging/Cargo.toml --release --locked --offline
export COMMONPLACE_MODEL_CACHE="$PWD/spikes/rust-packaging/target/pinned-models"
# Use a new directory on subsequent runs; never overwrite an earlier probe store.
COMMONPLACE_SPIKE_DATA_DIR="$PWD/spikes/rust-packaging/target/p2-final" \
  sandbox-exec -p '(version 1)(allow default)(deny network*)' \
  spikes/rust-packaging/target/release/rust-packaging all \
  > spikes/rust-packaging/target/p2-final.json
```

The same OS sandbox denied a control `curl https://huggingface.co/` request
(exit 6, unable to resolve host). Thus this is network-denied execution, not
merely a claim based on `HF_HUB_OFFLINE` or a proxy variable.

Durable excerpt of the successful combined execution:

```json
{
  "grafeo": {
    "gate": "blocked",
    "bounded_results": {"allowed": 3, "consumed": 3, "materialized_before_iteration": 10000},
    "publication": {"status": "pass", "reopened_name": "Candidate"},
    "deadline": {"elapsed_ms": 204}
  },
  "inference": {
    "status": "pass",
    "dimensions": 384,
    "embedding_batches": [2, 1],
    "norms": [0.9999997019767761, 0.9999998807907104, 1.0000003576278687],
    "reranker_batch": 2,
    "reranker_top_index": 0,
    "reranker_top_score": -0.25560659170150757,
    "artifact_hashes_verified": true,
    "repeat_calls_same_sessions": true,
    "sqlite": {
      "dimensions": 384,
      "nearest_passage_id": 1,
      "fts_passage_id": 2,
      "dimension_rejection": true,
      "transaction_rollback_and_commit": true
    }
  }
```

This proves real embedding-to-vector binding, not just an unrelated four-float
fixture. Inference calls receive chunks of at most two texts; the same loaded
sessions serve repeat embedding and reranking calls. Embeddings have finite unit
norms and stable repeat output; reranking order/scores repeat exactly. SQLite
rejects a one-dimensional vector, and passage/FTS/vector deletes roll back or
commit together. Synthetic standalone `sqlite` remains useful for model-free
checks. `sqlite` and `grafeo` both pass with a nonexistent model cache, proving
that these paths do not eagerly load inference.

### P3/P4 real-model test convention

Use a dedicated ignored `tests/real_models.rs`, explicitly invoked against
`COMMONPLACE_MODEL_CACHE`. Ordinary tests neither download nor run models.
Explicit runs fail on missing, corrupt, or incompatible artifacts; `#[ignore]`
is an invocation choice, not an internal skip-on-error branch. P3 owns the first
production invocation; P4 extends that target, rather than adding another cache
scheme or public model-preparation command.

Verified spike commands:

```sh
cargo fmt --manifest-path spikes/rust-packaging/Cargo.toml -- --check
cargo fmt --manifest-path spikes/grafeo-evaluation/Cargo.toml -- --check
cargo clippy --manifest-path spikes/rust-packaging/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo clippy --manifest-path spikes/grafeo-evaluation/Cargo.toml --locked --offline --all-targets -- -D warnings
cargo test --manifest-path spikes/rust-packaging/Cargo.toml --release --locked --offline --all-targets
COMMONPLACE_MODEL_CACHE="$PWD/spikes/rust-packaging/target/pinned-models" \
  sandbox-exec -p '(version 1)(allow default)(deny network*)' \
  cargo test --manifest-path spikes/rust-packaging/Cargo.toml --release --locked --offline \
  --test real_models -- --ignored --exact combined_cached_models
```

Results: three ordinary integration tests pass (lazy native commands,
missing-model failure, incompatible-model failure); one model test is ignored
ordinarily and passes when explicitly selected under network denial.

### Target evidence matrix

| Target | Prior standalone Grafeo | Selected combined inference/native stack | P10 clean-target release |
| --- | --- | --- | --- |
| macOS arm64 | Pass | **PASS locally** | UNVERIFIED |
| Windows x64 | Pass, historical independent machine | UNVERIFIED | UNVERIFIED |
| Windows arm64 | Pass, historical runner | UNVERIFIED | UNVERIFIED |
| Linux x64 | Pass, historical runner | UNVERIFIED | UNVERIFIED |
| Linux arm64 | Pass, historical runner | UNVERIFIED | UNVERIFIED |

Historical standalone execution is not combined inference evidence. The graph
API blockers also remain independently unresolved on all targets. macOS x64 is
not a release target. Missing target execution blocks release, not proven-host
feature work.

## Historical Ladybug baseline (superseded)

This report records the original Ladybug-based full-stack packaging baseline.
Ladybug was later replaced by Grafeo; the current graph decision and
cross-platform evidence are in
[the Grafeo evaluation spike](grafeo-evaluation.md).

## Outcome

**Pass for macOS arm64.** The selected Rust, SQLite, vector, graph, embedding,
and reranking components build and run together as a relocatable release
bundle. The extracted bundle executes all probes with an empty environment,
an intentionally unusable network proxy, and a `PATH` containing no Rust,
Cargo, CMake, or Homebrew tools.

The test was performed on a clean temporary extraction on the build host, not
on a second physical machine. A fresh-host smoke test remains required for
each release target, as specified by the release acceptance criteria.

## Selected components

| Component | Pinned selection | Result |
|---|---|---|
| Rust | 1.98.1, edition 2024 | Pass |
| Ladybug Rust crate and native release | `lbug` 0.20.4 / Ladybug 0.20.4 | Pass |
| SQLite binding | `rusqlite` 0.37.0 with bundled SQLite 3.50.2 | Pass |
| Vector extension | `sqlite-vec` 0.1.6 | Pass |
| Inference runtime | `fastembed` 7.1.0 with ONNX Runtime | Pass |
| Embedding model | `Qdrant/all-MiniLM-L6-v2-onnx` revision `8f518e882455312b086101e60691f5e6e2f05c3c`, 384 dimensions | Pass |
| Reranker model | `jinaai/jina-reranker-v1-turbo-en` revision `b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2` | Pass |

`sqlite-vec` 0.1.10-alpha.4 was evaluated first and rejected because its
published crate omitted `sqlite-vec-diskann.c`, causing native compilation to
fail. Version 0.1.6 is the working pinned choice.

The initial `BAAI/bge-reranker-base` probe passed, but its ONNX file was about
1.0 GB. The selected Jina reranker passed the same relevance check with an
approximately 160 MB ONNX file, reducing the complete model cache from about
1.2 GB to 259 MB.

## Functional evidence

### SQLite, FTS5, and vectors

The harness:

- creates a strict ordinary passage table and an FTS5 table;
- creates `vec0(passage_id INTEGER PRIMARY KEY, embedding FLOAT[4])`;
- binds `f32` vectors as bytes through Rust;
- retrieves the expected integer passage ID by nearest-neighbor search;
- retrieves the expected passage through FTS5; and
- inserts and deletes ordinary, FTS, and vector rows in shared transactions.

Observed versions were SQLite 3.50.2 and sqlite-vec `v0.1.6`.

### Ladybug

The harness creates two entity nodes and one relationship containing a
canonical `knowledge_item_id`, then reads the relationship back through
Cypher. It also proves:

- a prepared mutation is identified as non-read-only;
- reopening the database with native read-only configuration rejects mutation;
- `Connection::interrupt()` cancels an executing query in about 110 ms; and
- a native 50 ms query timeout cancels an executing query in about 110 ms.

The crate reports native library source
`release:LadybugDB/ladybug/v0.20.4` and storage version 47. The build must set
`LBUG_VERSION=0.20.4`; otherwise the crate defaults to downloading Ladybug's
`latest` release, which is not sufficiently reproducible.

### Local inference

FastEmbed produced three 384-dimensional embeddings in a bounded batch and
ranked the relevant passage first from a two-document candidate set. A second
run succeeded from the packaged cache with `HF_HUB_OFFLINE=1` and all HTTP
proxies directed to an unreachable local endpoint.

The selected model cache occupies approximately 259 MB. Model weights load
only when the `models` or `all` probe is selected; the `sqlite` and `ladybug`
commands do not initialize inference. The packaging script rejects caches
whose model revision hashes differ from the selections above; production
packaging should likewise acquire and verify those immutable revisions rather
than treating a repository's moving `main` reference as the model identity.

## Packaging evidence

The release executable is a 53,611,808-byte Mach-O arm64 binary. Ladybug is
statically linked, but its precompiled native archive introduces dynamic
OpenSSL dependencies. The packaging script includes and ad-hoc signs
`libssl.3.dylib` and `libcrypto.3.dylib`, rewriting their install names to
bundle-relative paths.

The resulting bundle is approximately 289 MB. The compressed archive from the
validated run is 174,046,481 bytes with SHA-256:

```text
92e8d66a60da148ee722ee9fc3617948c8d9ee65d53d1409a57e9b23e96af36b
```

Its remaining dynamic dependencies are the bundled OpenSSL libraries and
macOS system libraries/frameworks:

```text
@executable_path/lib/libssl.3.dylib
@executable_path/lib/libcrypto.3.dylib
/usr/lib/libc++.1.dylib
/usr/lib/libiconv.2.dylib
/usr/lib/libSystem.B.dylib
/usr/lib/libobjc.A.dylib
/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation
/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation
/System/Library/Frameworks/CoreML.framework/Versions/A/CoreML
```

The bundle passed `codesign --verify --deep --strict` and all functional probes
after extraction under `/tmp`.

## Reproduction

Host used for this spike:

```text
macOS 27.0 (26A428), arm64
rustc 1.98.1 (48a229cea 2026-09-01)
cargo 1.98.1 (797e8a9bc 2026-08-05)
cmake 4.4.3
Apple clang 21.0.0
```

Commands:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cd spikes/rust-packaging

export LBUG_VERSION=0.20.4
cargo build --release --locked
COMMONPLACE_SPIKE_DATA_DIR="$PWD/target/spike-data" \
  ./target/release/rust-packaging all

./package-macos.sh
./verify-macos.sh
```

The harness source and exact transitive dependency lockfile live in
`spikes/rust-packaging/`.
