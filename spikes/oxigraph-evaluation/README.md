# Stock Oxigraph P2 evaluation

Local-only disposable spike, **not a production backend or specification change**.
It evaluates unmodified published Oxigraph 0.5.11 as a possible replacement for
Grafeo. Baseline features: `default-features = false`, `rocksdb` only. The
optional `rdf12-probe` feature tests quoted triple terms but is not required by
the knowledge mapping. No engine patches, migrations, network service handlers,
or model downloads occur at runtime.

See the [report](../../docs/spikes/oxigraph-evaluation.md) for gate results,
proposed SPARQL/RDF contracts, precise limitations, and source references.
`fixture.sql` is a compact synthetic authoritative SQLite fixture, not a
replacement for Commonplace's normative production schema.

## Local checks

From the repository root, using the existing local Rust/C++ toolchain:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo build --manifest-path spikes/oxigraph-evaluation/Cargo.toml \
  --release --locked --offline -j 2
cargo fmt --manifest-path spikes/oxigraph-evaluation/Cargo.toml --check
cargo clippy --manifest-path spikes/oxigraph-evaluation/Cargo.toml \
  --release --locked --offline -j 2 --all-targets -- -D warnings
cargo test --manifest-path spikes/oxigraph-evaluation/Cargo.toml \
  --release --locked --offline -j 2
```

The first offline resolution reported missing Oxigraph. Dependency acquisition
then used ordinary Cargo after the manifest change; no blocked-host bypass.
Subsequent validation is locked/offline. Building RocksDB needs C++20 and
libclang/bindgen; the tested machine's Xcode toolchain supplied them.
Use two Cargo jobs to bound native compiler parallelism. Missing dependencies
are explicit errors; acquire only via approved sources.

Ordinary tests exercise real SQLite and persistent Oxigraph, native read-only
rejection, query bounds/cancellation, multiple processes, and deliberate process
exits. They do not initialize inference or need model weights. The executable
refuses an existing probe data directory:

```sh
run="$(mktemp -d "$PWD/spikes/oxigraph-evaluation/target/graph.XXXXXX")"
COMMONPLACE_SPIKE_DATA_DIR="$run/data" \
  spikes/oxigraph-evaluation/target/release/oxigraph-evaluation graph \
  > "$run/results.json"
```

Private `child` modes exist only for the process harness, not as new public
Commonplace commands. The tests intentionally terminate a child with SIGINT and
exit other children with status 73 to simulate crashes.

## Explicit real-model and relocated execution

This executable includes the existing
`spikes/rust-packaging/src/{models,sqlite}.rs` sources directly. The immutable
artifacts and byte-only loaders are exactly those in
[`../rust-packaging/models.json`](../rust-packaging/models.json).
Use the already-prepared cache from the original P2 run; no fresh download is
required. Absent or incompatible weights cause an error, never a skip/fallback.

```sh
COMMONPLACE_MODEL_CACHE="$PWD/spikes/rust-packaging/target/pinned-models" \
  cargo test --manifest-path spikes/oxigraph-evaluation/Cargo.toml \
  --release --locked --offline -j 2 real_combined_models -- --ignored --exact

# Rebuild the BASELINE binary if the optional feature was tested last.
cargo build --manifest-path spikes/oxigraph-evaluation/Cargo.toml \
  --release --locked --offline -j 2
bash spikes/oxigraph-evaluation/verify-local.sh \
  "$PWD/spikes/rust-packaging/target/pinned-models"
```

`verify-local.sh` copies the binary and verified cache to an assembly directory,
renames the entire bundle, and executes it under macOS network denial with empty
HOME and `PATH=/usr/bin:/bin`. Its `all` mode runs real graph publication,
inference, FTS5, and sqlite-vec in one release executable. It prints the binary
hash, dynamic dependencies, and result file path. No build tools are invoked by
the relocated executable. This is a host-only feasibility check, not signed
release packaging or a claim about other targets.

## Optional RDF 1.2 coverage

```sh
cargo test --manifest-path spikes/oxigraph-evaluation/Cargo.toml \
  --release --locked --offline -j 2 --features rdf12-probe
cargo clippy --manifest-path spikes/oxigraph-evaluation/Cargo.toml \
  --release --locked --offline -j 2 --all-targets --all-features -- -D warnings
```

Reification with canonical knowledge IDs works using RDF 1.1 alone. This feature
only demonstrates recursive serialization of native triple terms and retains
direction information on literals. It does not change the baseline mapping.
Experimental language coverage must not become an adoption prerequisite.
