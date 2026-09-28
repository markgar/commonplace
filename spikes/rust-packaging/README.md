# Rust runtime spike

Disposable local-only combined Grafeo, SQLite/FTS5, sqlite-vec, embedding and
reranking preflight. Not production infrastructure. See
[the report](../../docs/spikes/rust-packaging.md) for separate gate outcomes.
The graph diagnostic exits successfully when it reproduces the known blockers;
its JSON says `gate: blocked`. That is not a passing graph gate.

From the repository root, on macOS arm64:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo build --manifest-path spikes/rust-packaging/Cargo.toml --release --locked --offline

# Explicit acquisition only. Uses immutable URLs and verifies every artifact.
# Stop on network-policy failures; do not change endpoints or bypass controls.
python3 spikes/rust-packaging/prepare-models.py \
  "$PWD/spikes/rust-packaging/target/pinned-models"
export COMMONPLACE_MODEL_CACHE="$PWD/spikes/rust-packaging/target/pinned-models"

# Each probe needs a fresh disposable output directory; existing stores are rejected.
export COMMONPLACE_SPIKE_DATA_DIR="$(mktemp -d /tmp/commonplace-p2.XXXXXX)"
sandbox-exec -p '(version 1)(allow default)(deny network*)' \
  spikes/rust-packaging/target/release/rust-packaging all

# Ordinary tests: no model loading/downloading, including missing/corrupt-cache tests.
cargo test --manifest-path spikes/rust-packaging/Cargo.toml \
  --release --locked --offline --all-targets
cargo clippy --manifest-path spikes/rust-packaging/Cargo.toml \
  --locked --offline --all-targets -- -D warnings

# Explicit real-model target: absent/incompatible weights FAIL; never silently skip.
sandbox-exec -p '(version 1)(allow default)(deny network*)' \
  cargo test --manifest-path spikes/rust-packaging/Cargo.toml \
  --release --locked --offline --test real_models \
  -- --ignored --exact combined_cached_models --nocapture
```

`models.json` pins revisions and all artifact identities. Cache layout is
`COMMONPLACE_MODEL_CACHE/<revision>/<repository-relative-file>`, not a mutable
Hugging Face `refs/main`. The runtime verifies bytes and uses FastEmbed's
user-defined-model constructor with the selected ONNX models; Hugging Face
client support is not compiled. Cache preparation requires Python 3 and curl;
inference does not. Existing valid files are reused, incompatible files fail
without replacement, and failed downloads leave no partial artifact.

`sqlite` and `grafeo` do not even inspect the model cache. `models` additionally
binds the real 384-dimensional embeddings to SQLite; `all` adds the shared graph
preflight from `../grafeo-evaluation/src/preflight.rs`.

The same ignored `tests/real_models.rs`/`COMMONPLACE_MODEL_CACHE` convention is the
P3/P4 handoff; P3 owns its first production test and exact production command.
Do not run acquisition from tests or ordinary README checks.

The updated `package-macos.sh` and `verify-macos.sh` are optional **manual**
same-host bundle helpers. P2 did not execute them or claim clean-host packaging;
that remains P10. They contain no Actions dispatch or remote execution.
