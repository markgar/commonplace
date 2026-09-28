# Rust packaging spike

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
