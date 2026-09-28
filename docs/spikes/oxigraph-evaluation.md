# P2 stock Oxigraph evaluation

## Recommendation and gates

**PASS on the local macOS arm64 host for the evaluated stock-Oxigraph graph
capabilities and the combined inference/native runtime. Recommend replacing
Grafeo with unmodified Oxigraph, subject to the explicit contract and layout
decisions below.** No engine patch is needed for this demonstrated route.
This is a replacement evaluation, not production adoption or a claim that
Oxigraph implements the currently specified Cypher/property-graph contract.
The original [Grafeo blockers](grafeo-evaluation.md) and
[patched prototypes](grafeo-resolution.md) remain unchanged.

The user authorized evaluating SPARQL and does not want to maintain engine
patches. This spike uses published
[Oxigraph 0.5.11](https://github.com/oxigraph/oxigraph/releases/tag/v0.5.11),
released **2026-09-02 19:18:29 UTC**. Its isolated Cargo project has no
`[patch]`, source fork, alternate provider, or production wiring.

| Gate | Executed local evidence | Outcome |
| --- | --- | --- |
| Canonical projection | Real SQLite transaction to complete persistent RDF candidate; duplicate facts, memberships, exact old-revision citations, typed literals, withdrawal, unused vocabulary | PASS |
| Native read-only/local queries | Read-only RocksDB open, query/update parser separation, rejected direct insert/clear/update, no HTTP-client dependency | PASS |
| Bounds/cancellation | Lazy stock SELECT iterator, exactly three instrumented evaluations for three consumed rows, correct bounded prefixes, native cancellation during expensive/no-result work, SIGINT | PASS, cooperative overshoot recorded |
| Durable version/publication | Named-graph metadata, reopen including empty graph, two reader processes, native writer lock, restore after real SQLite COMMIT failure, crash rejection/rebuild, corrupt candidate | PASS for tested windows |
| Combined runtime | Stock Oxigraph + SQLite/FTS5/sqlite-vec + pinned embedding/reranking in a relocated release executable, empty HOME, no build-tool PATH, OS network denial | PASS |

**Remaining decisions are product/architecture choices, not a discovered engine
blocker:** SPARQL/RDF mapping and result JSON, operation-scoped cancellation timer,
visible metadata graph, and native directory publication/compatibility. Bare
minimum-i64 expression behavior is a documented engine-language limitation,
not data loss in the canonical projection. No automatic query rewriting is used.

## Specification ownership and proposed departures

The investigation reread `docs/specification/README.md` and its hierarchy:
product owns behavior, architecture owns boundaries, persistence owns durable
representation, implementation owns sequencing/acceptance. Relevant sources:
product graph behavior at lines 416-440; architecture sections 5.3/7.2/8;
persistence sections 5, 8, 9, 13, and 14; implementation section 3.5 P2 and its
separate named gates/local-only policy. The root README's formatting, Clippy,
and test checks were applied to this independent manifest, not the unmodified
production project.

Adoption would require explicit coordinated changes, **none made here**:

1. Replace Cypher/Grafeo-specific input and native role wording with SPARQL
   queries on a native `Store::open_read_only` handle; updates are a different
   native parser/API and remain forbidden to the public query path.
2. Approve the reified RDF mapping below and a tagged RDF-term result contract,
   rather than LPG nodes/edges/path JSON. Preserve ordered SELECT columns/rows;
   decide whether to expose the demonstrated ASK and graph-result shapes.
3. Replace native built-in deadline wording with an operation-scoped timer that
   cancels the engine's native token. It is cooperative, can overshoot, and
   is joined on completion; it is not a persistent job/service.
4. Approve `urn:commonplace:metadata` as discoverable graph metadata and the
   `knowledge_version` triple. This is a reserved application namespace, not
   a hidden store API or independent sidecar.
5. Replace the initialized single-file `graph/current.grafeo` assumption with
   native RocksDB directories (`current`, `candidate`, temporary `previous`)
   and the same separate publication lock. Incompatible old layouts must fail
   explicitly; no migration or silent replacement is implied. A process crash
   between the two renames can leave `current` absent, not just version-stale;
   reads must reject it and explicit rebuild must cover it.

The invariants remain: authoritative SQLite, explicit authored knowledge,
canonical immutable citations, complete derived graph, fail-closed versions,
shared reader/exclusive publication leases, rollback on reported failure, and
no remote source-text inference.

## Reproduce and identify the artifacts

Harness: [`spikes/oxigraph-evaluation/`](../../spikes/oxigraph-evaluation/README.md).
Checked-in [structured results](oxigraph-results.json) preserve the final
baseline output and separate optional triple-term excerpt; only absolute
worktree prefixes were normalized to `${WORKTREE}`. No `target/` artifact is
required to read the handoff evidence.

Executed locally:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo fmt --manifest-path spikes/oxigraph-evaluation/Cargo.toml --check
cargo test --manifest-path spikes/oxigraph-evaluation/Cargo.toml \
  --release --locked --offline -j 2
cargo clippy --manifest-path spikes/oxigraph-evaluation/Cargo.toml \
  --release --locked --offline -j 2 --all-targets -- -D warnings
COMMONPLACE_MODEL_CACHE="$PWD/spikes/rust-packaging/target/pinned-models" \
  cargo test --manifest-path spikes/oxigraph-evaluation/Cargo.toml \
  --release --locked --offline -j 2 real_combined_models -- --ignored --exact
bash spikes/oxigraph-evaluation/verify-local.sh \
  "$PWD/spikes/rust-packaging/target/pinned-models"
```

Baseline: **one metadata unit test + two ordinary integration tests passed**;
the ordinary run reports the real-model test ignored with its explicit cache
reason. The explicit invocation then ran that real-model test and passed.
Optional `--features rdf12-probe` tests and all-feature Clippy also passed;
the final bundle was rebuilt with **baseline features only**.

Host: macOS 27.0 build 26A428, arm64; rustc 1.98.1
(`48a229cea 2026-09-01`), Cargo 1.98.1. Native compilation used two Cargo jobs
and the local Xcode C++/libclang toolchain. Initial offline resolution reported
missing Oxigraph; ordinary Cargo acquired dependencies after the manifest edit.
Subsequent commands were locked/offline. No blocked host or bypass occurred;
no new model acquisition occurred.

Final baseline executable: **45,616,032 bytes**, SHA-256
`39aebca7ecb5ef334576109c96b26f9f9d01fc1a01b00c64ad6bb297eb3f4e02`.

| Component | Exact version / identity |
| --- | --- |
| Oxigraph | 0.5.11; crate SHA-256 `7f6632dd080bd07676f06e70254ff5862c9943e9483d700fa7258df73a3f9a6a` |
| oxrocksdb-sys | 0.5.11; crate SHA-256 `01fcf5ae1e1295a612bef72fc64cd81e1ac3d1bd3243ef84d330f2962a8335e1` |
| Bundled RocksDB | 11.0.4, from its `include/rocksdb/version.h`; bundled LZ4 |
| RDF/query stack | oxrdf 0.3.4, spareval 0.2.7, spargebra 0.4.7, oxrdfio 0.2.6, sparesults 0.3.4 |
| SQLite | rusqlite 0.37.0, libsqlite3-sys 0.35.0, runtime SQLite 3.50.2 / FTS5 |
| Vector extension | sqlite-vec 0.1.6, runtime v0.1.6 |
| Inference | FastEmbed 7.1.0, ort/ort-sys 2.0.0-rc.13, native ONNX Runtime 1.28.0 |

All transitive packages/checksums are in the isolated Cargo.lock. Baseline
Oxigraph features are only `rocksdb` (and its `oxrocksdb-sys` dependency);
`oxhttp` is absent from the lock. MIT/Apache-2.0 Oxigraph is used unchanged.

## Canonical RDF mapping and actual evidence traversal

`fixture.sql` is a compact synthetic schema with real SQLite constraints,
not the production schema/migration. The builder is required to run inside
the caller's SQLite transaction. It executes separate ordered reads of active
knowledge, referenced entities/vocabulary, supporting passages/revisions/docs,
and evidence pairs. It validates the fixture's kind/object shape and exact
UTF-8 passage slice before emitting quads. Batches to `Store::extend` have at
most 128 quads, avoiding a single graph-sized import transaction. Referenced
passage/revision data is emitted separately from knowledge objects.

The proposed canonical ID mapping is deterministic and reversible:

| Canonical object | RDF IRI |
| --- | --- |
| `doc:1` | `urn:commonplace:doc:1` |
| revision ID 1 | `urn:commonplace:revision:1` |
| `passage:1` | `urn:commonplace:passage:1` |
| `entity:1` | `urn:commonplace:entity:1` |
| `knowledge:3` | `urn:commonplace:knowledge:3` |
| type/predicate IDs | `urn:commonplace:type:1`, `urn:commonplace:predicate:1` |

Each authored item is a **distinct subject IRI**, not just an RDF relationship
triple. With `c:` = `urn:commonplace:property:`, facts have `c:kind "fact"`,
`c:subject`, `c:predicate`, `c:object`, and zero or more `c:evidence` links.
Memberships instead have `c:kind "type_membership"`, `c:subject`,
`c:entity_type`, and evidence. Names belong to referenced entity/vocabulary
IRIs. No convenience direct relationship triple replaces the authored item.
Thus equal facts `knowledge:3` and `knowledge:4` survive RDF set deduplication.
Multiple citations add links without duplicating a semantic item.

Objects are entity IRIs or native typed literals: signed i64 as `xsd:integer`,
boolean as `xsd:boolean`, string as `xsd:string`, normalized UTC timestamp as
`xsd:dateTime`. Production would reuse its canonical timestamp validation;
the spike uses fixed valid UTC fixtures. Passages carry revision links, exact
text, and signed integer start/end byte offsets; revisions carry document
links, revision numbers and digests. SQLite remains the exact source authority.
No blank nodes are used for canonical knowledge or citation identities.

Executed join/type/citation query:

```sparql
PREFIX c: <urn:commonplace:property:>
SELECT ?k ?subject ?object ?passage ?revision ?document ?start ?end ?quote
WHERE {
  ?k c:kind "fact"; c:predicate <urn:commonplace:predicate:1>;
     c:subject ?subject; c:object ?object; c:evidence ?passage .
  ?membership c:subject ?subject; c:entity_type <urn:commonplace:type:1> .
  ?passage c:revision ?revision; c:start_byte ?start;
           c:end_byte ?end; c:text ?quote .
  ?revision c:document ?document .
}
ORDER BY ?k ?passage
```

Observed three rows: knowledge 3 / passage 1 / `"café"` at `[0,5)`;
knowledge 3 / passage 2 / `"supports Atlas"` at `[6,20)`; knowledge 4 /
passage 1 / `"café"`. All resolve to **revision 1**, even though revision 2
exists in SQLite. The probe rehydrates passage IDs from SQLite and compares
exact text. Withdrawn knowledge 10, its exclusively cited new-revision passage,
and unused entity/type/predicate terms are absent. A successful later
withdrawal removes knowledge 4 from the next graph.

The initial build sees uncommitted version 2 while a separate SQLite connection
still sees committed version 1, then publishes/commits the same version. This
proves candidate construction uses the transaction snapshot rather than
reopening committed SQLite behind the caller.

## Stock query semantics, serialization, and bounds

`QueryResults` distinguishes SELECT solutions, ASK boolean, and
CONSTRUCT/DESCRIBE triples. The proposed result shape is:
SELECT `{kind, columns, rows, truncated}` with columns in native projected order;
ASK `{kind:"ask", boolean}`; graph results `{kind:"graph", triples, truncated}`.
`consumed` is spike instrumentation, not a proposed required production field.
Unbound variables use JSON null. RDF terms are tagged URI, blank node, or
literal objects; literal lexical value, datatype IRI, and language are retained.
Integers remain **typed lexical strings**, preserving exact i64 values for
JSON consumers that cannot represent large numbers. Blank-node identifiers have
result/store scope, not canonical Commonplace identity.

No custom map heuristics, LPG tags, or query rewrite is needed. Optional
`rdf12-probe` enables native triple terms and recursively encodes their
subject/predicate/object, including directional-literal metadata. It passed
separately but is unnecessary for ordinary reified RDF 1.1 knowledge; it is
not enabled in the baseline recommendation.

Stock source evidence:
[spareval QuerySolutionIter](https://github.com/oxigraph/oxigraph/blob/v0.5.11/lib/spareval/src/model.rs)
owns a boxed iterator (lines 79-82) and `next()` directly delegates
(133-139), rather than iterating a precollected result Vec.
[PreparedSparqlQuery::on_store](https://github.com/oxigraph/oxigraph/blob/v0.5.11/lib/oxigraph/src/sparql/mod.rs)
binds a storage snapshot (584-587).

Executable proof uses a supported custom function **only as instrumentation**:
over 2,000 real stored triples, its counter is zero after execute and exactly
three after consuming three solutions. Dropping the iterator performs no
further evaluations. Normal evaluation has no registered custom functions.
The adapter collects at most `row_limit + 1`, drops the engine iterator,
then removes only the extra sentinel row. This is not truncation after
exhaustive engine collection. Cap=2 returns two rows, consumes three, and
reports truncation.

Bounded prefixes match full small-fixture results for plain SELECT, descending
ORDER BY, DISTINCT, SUM aggregate, UNION, OFFSET and explicit user LIMIT.
Stock substitution supports repeated 7/19 parameters and both i64 extrema.
ASK and bounded CONSTRUCT pass. Graph-result iteration order is not guaranteed,
and truncation produces a partial triple stream, not a complete graph.
**The bound is final result retention, not sorting/join/aggregate working
memory, triple-pattern reads, or bytes inside an individual result value.**
Queries without ORDER BY have no stable cross-run order guarantee.

One stock limitation is preserved explicitly:
`SELECT (-9223372036854775808 AS ?minimum) WHERE {}` returns unbound/null.
The signed minimum succeeds as a stored `Literal::from(i64::MIN)`, as a native
substituted term, and as
`"-9223372036854775808"^^<http://www.w3.org/2001/XMLSchema#integer>`.
No user query is rewritten to substitute this syntax. Arbitrary SPARQL
expression errors retain SPARQL's unbound-result semantics; the documented
canonical storage path does not lose the value.

## Read-only, SERVICE, cancellation, and process lifetime

The public-query candidate accepts only `SparqlEvaluator::parse_query`, not
`parse_update`, and runs on `Store::open_read_only`. Native direct insert/clear
and parsed INSERT/DELETE/CLEAR/LOAD attempts all return
`Transaction are only possible on read-write instances`. Query parsing alone
is not the read-only proof; native mutation attempts on the actual disk handle
are tested.

Oxigraph's HTTP SERVICE handler exists only when `http-client` is enabled
([source](https://github.com/oxigraph/oxigraph/blob/v0.5.11/lib/oxigraph/src/sparql/mod.rs),
lines 50-52, 174-179). Baseline disables default features and enables RocksDB
only; the dependency graph contains no oxhttp. A SERVICE request returns
`The service <http://127.0.0.1:9/sparql> is not supported`.
`SERVICE SILENT` follows standard SPARQL semantics and yields an empty binding
instead of propagating that error; it does **not** enable HTTP or indicate
successful remote data retrieval. No remote handlers are registered.

`SparqlEvaluator::with_cancellation_token` is the stock cancellation mechanism,
not a built-in wall-clock timeout. One operation-scoped thread waits on a
channel with a duration; expiry cancels the native token. Early completion
wakes and joins that thread, including error/early-drop paths.
The token spans execute **and** iterator consumption. This does not make
parsing or every expensive operator preemptible.

With a requested **20 ms** budget, the recorded expensive three-way aggregate
returned typed `QueryEvaluationError::Cancelled` after **299.19 ms**; a
no-result three-way SELECT returned it after **283.38 ms**. Both meet the local
probe's 3-second upper bound, not a promised 20 ms hard deadline. A fast ASK
with a 30-second timer completed and joined in **0.118 ms**, not 30 seconds.
SIGINT terminated the query child after its store/lease readiness signal and
released its publication lock; readiness precedes query execution, so this is
process/lease lifecycle evidence rather than an operator-level interrupt probe.
Strict hard real-time limits would need a different, explicitly approved
process-boundary policy; they are not claimed here.

## Durable metadata and native-directory publication

Metadata uses an ordinary native named graph:

```sparql
GRAPH <urn:commonplace:metadata> {
  <urn:commonplace:store>
    <urn:commonplace:property:knowledge_version> 2 .
}
```

It persists in the **same native RocksDB directory** as the projection.
It is not in the default graph but is visible through `GRAPH ?g` or explicit
metadata queries. This namespace/discoverability is an adoption decision,
not an authorization boundary. The reader requires exactly one nonnegative,
typed i64 version and equality with authoritative SQLite. Missing, duplicate,
string-typed, and negative metadata fail. An otherwise empty graph contains
only its metadata triple and default-graph ASK returns false.

Native [Store documentation](https://github.com/oxigraph/oxigraph/blob/v0.5.11/lib/oxigraph/src/store.rs)
warns that another process writing the **same** database while it is opened
read-only is undefined (204-208). The spike never does that. Writers build
a separate candidate in bounded batches, explicitly flush it, and drop it
before opening for verification or renaming. Two child reader processes hold
shared publication leases plus open results; the publisher cannot acquire
an exclusive lease until both close. A separate test verifies that the native
RocksDB LOCK rejects a second write process, then permits reopening after close.
Results retain storage snapshots even after dropping `Store`; the probe
demonstrates this and drops **both result and handle** before releasing a lease.

Under the exclusive lease, activation renames `current` to temporary `previous`,
then `candidate` to `current`, and syncs the parent directory. SQLite COMMIT
follows; successful commit discards previous. A deliberately violated deferred
SQLite foreign key makes COMMIT return a real error; SQLite rolls back and the
previous graph is restored before releasing the lease.

Local child-process exits without destructors exercise:

| Crash window | Observed read behavior |
| --- | --- |
| Before activation | SQLite rolls back; old complete graph remains readable |
| After activation, before SQLite commit | SQLite version 1 versus graph version 2; read rejected |
| Between directory renames | `current` absent; read rejected rather than creating a new store |

Each case is followed by an explicitly invoked fixture rebuild from committed
SQLite and verifies parity. The fixture explicitly removes its known disposable
paths during recovery; this is **not** adoption of an automatic orphan/generation
selection or cleanup protocol. A corrupted candidate with truncated RocksDB
`CURRENT` is rejected; the valid current directory remains readable. These
checks cover process failures, not full power-loss recovery, all filesystem
faults, every corruption, or every possible commit/rename failure.

## Combined offline runtime and packaging evidence

The executable reuses existing pinned byte-only model loaders and SQLite vector
checks directly, without importing Grafeo. All graph modes run with no model
cache environment variable; inference is initialized only in `all`.
The explicit real-model convention fails on missing/incompatible cache rather
than skipping/downloading. The relocation script copies an already-prepared
cache and binary, renames the bundle, and runs `all` under:

```sh
sandbox-exec -p '(version 1)(allow default)(deny network*)' /usr/bin/env -i \
  PATH=/usr/bin:/bin HOME="$run/empty-home" \
  COMMONPLACE_MODEL_CACHE="$run/relocated/models" \
  COMMONPLACE_SPIKE_DATA_DIR="$run/data" \
  "$run/relocated/bin/oxigraph-evaluation" all
```

Network-denial control: curl to `https://huggingface.co/` under the same sandbox
fails exit 6 (`Could not resolve host`). Inference still passes. Existing cache:
Qdrant/all-MiniLM-L6-v2-onnx at
`8f518e882455312b086101e60691f5e6e2f05c3c`, ONNX SHA-256
`bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5`;
jinaai/jina-reranker-v1-turbo-en at
`b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2`, ONNX SHA-256
`c1296c66c119de645fa9cdee536d8637740efe85224cfa270281e50f213aa565`.
All tokenizer/config identities, immutable URLs, ORT distribution hash, and
original preparation behavior remain in the [packaging report](rust-packaging.md)
and `spikes/rust-packaging/models.json`.

Recorded excerpt: 384 dimensions; embedding batches `[2,1]`; norms
`[0.999999702,0.999999881,1.000000358]`; reranker batch 2/top index 0/score
`-0.2556065917`; nearest vector passage 1, FTS passage 2; wrong dimension
rejected; commit/rollback verified; repeated inference in the same sessions
agrees. Artifact hashes are verified before model construction.

`otool -L` on the relocated executable lists only macOS system dependencies:
libc++, libiconv, libSystem, libobjc, Foundation, CoreML, CoreFoundation.
There are no Homebrew/build-directory dylib paths. RocksDB/C++ compilation is
the additional native build cost; bundled SQLite, sqlite-vec, ONNX Runtime and
RocksDB/LZ4 code are linked into this executable. RocksDB has normal native
background flush/compaction machinery; the application does not introduce
persistent background jobs and explicitly flushes candidates.

## Boundaries and target evidence

| Target | Graph replacement suite | Combined relocated inference/native proof |
| --- | --- | --- |
| This macOS arm64 host | PASS | PASS |
| macOS x64 | UNVERIFIED | UNVERIFIED |
| Linux x64 / arm64 | UNVERIFIED | UNVERIFIED |
| Windows x64 / arm64 | UNVERIFIED | UNVERIFIED |

The finite fixture is not SPARQL conformance, large-store capacity, hard-deadline,
all-corruption, or all-platform certification. Production must still wire the
full canonical snapshot/schema, input/value-size limits, command error codes,
writer acquisition policy, rollback fault handling, and incompatible-layout
rejection. No production schema, public command, provider dependency,
specification, or workflow was changed. No Actions, other machines, engine
patches, external upstream submissions, merges, or release operations ran.
The evidence supports an adoption decision now; those implementation details
belong to the next approved slice, not more speculative spike infrastructure.
