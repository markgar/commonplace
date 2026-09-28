# Commonplace implementation and acceptance

## 1. Delivery objective

Implementation proceeds through vertical slices that each produce usable CLI
behavior. The first release is complete when a person can ingest local sources,
search exact evidence, author cited knowledge, query it through SPARQL SELECT, withdraw
knowledge, and reproduce the same results after reopening the application.

## 2. Technology gate

Before release packaging, a disposable Rust spike must prove that the selected
components can ship together as a self-contained distribution:

- stock Oxigraph with default features disabled and only `rocksdb` enabled;
- SQLite with FTS5;
- one SQLite vector extension;
- one local embedding model and runtime;
- one local reranker and runtime;
- native read-only storage and SELECT-only query enforcement;
- scoped native cooperative cancellation and process-level termination; and
- release execution without an installed interpreter, language runtime, source
  checkout, or build tool.

The spike records:

- exact component and model versions;
- build and packaging commands;
- target platform and architecture;
- produced artifacts and dynamic dependencies;
- model acquisition and cache behavior;
- vector DDL and Rust binding behavior;
- graph cancellation and mutation-rejection results; and
- pass or fail for clean-machine execution.

The selected vector extension must demonstrate:

1. fixed-dimension vector declaration;
2. Rust value binding;
3. integer `passage_id` lookup;
4. transactional insert and delete alongside ordinary tables and FTS5; and
5. reliable loading from the packaged release.

Failure of a required component reopens that component or the implementation
language choice. It does not introduce parallel backends.

The local graph and integrated runtime gates passed with stock Oxigraph 0.5.11
on macOS arm64. Evidence is committed in P2 commit
`5d3e0554ed5076d530a9b9a0907c2eca14b5d181`, in
`docs/spikes/oxigraph-evaluation.md` on the P2 branch.
That commit also contains structured results and the reproduction harness.
It proves the combined Oxigraph/RocksDB, SQLite/FTS5/sqlite-vec, and unchanged
pinned embedding/reranking runtime in a relocated, network-denied local run.
It is not independent clean-machine or other-platform execution evidence.

This specification adopts that stock-engine route, without engine patches.
Earlier Ladybug/Grafeo reports remain historical evidence, not proof for the
selected Oxigraph runtime. No further feasibility spike is required before local
implementation. Production integration and release packaging remain distinct:
P5a replaces the production engine, and P10 closes clean-target evidence.
The evaluated version is not a substitute for production Cargo pins.

The first release supports:

- macOS arm64;
- Windows x64 and arm64; and
- Linux x64 and arm64.

macOS x64 is not a release target.

## 3. Execution plan

### 3.1 Starting point and planning boundary

The planning baseline is commit `772f7424f6cd`. At that baseline the application
exposes only `init`. It includes the SQLite table definitions, FTS5 and sqlite-vec
initialization, an empty persistent Grafeo store, tagged IDs, a canonical file
source-key helper, and initialization tests. Those are starting assets, not
evidence that ingestion, knowledge publication, or release acceptance is done.
In particular, graph version metadata, publication locks, operational writer
locking, inference, and the remaining public workflows still need integration.

The initial plan changed delivery order; this revision also records the approved
Oxigraph/SPARQL replacement and version-2 store adoption.
[Product](product.md), [architecture](architecture.md), and
[persistence](persistence.md) retain their respective ownership under the
[specification hierarchy](README.md). Sections 5-7 below remain the release
acceptance contract. Spike reports supply implementation evidence, not substitute
requirements.

Optimize elapsed delivery time by keeping two implementation lanes moving after
a small shared starting slice. Do not divide work into independent CLI, domain,
database, and testing projects. Each packet owns its user path and its tests.
Knowledge writes and their graph publication ship together; a SQLite-only
authoring command is not an intermediate deliverable.

### 3.2 Dependency map and scheduling

The linked packet issues below own current progress, session ownership, blockers,
PRs, and integration evidence. Do not maintain a second status table in this file.
A dependency means the prerequisite packet or named gate has been integrated,
not merely started or committed. P2 has separate graph and inference gates so
an unrelated runtime question does not unnecessarily block both lanes. Its
recorded local proof is in section 2; production integration is tracked separately.

Execution policy: run development checks and spikes locally. Automatic GitHub
Actions CI is disabled to conserve Actions usage. Do not enable, dispatch, or
expand Actions workflows without fresh explicit approval. This changes where
validation runs, not the supported release targets or required release evidence.

| Packet | User-visible outcome or gate | Depends on | Primary ownership |
| --- | --- | --- | --- |
| [P0 (#11)](https://github.com/markgar/commonplace/issues/11) | Agree the first execution boundaries and unresolved decisions | Baseline inspection | Integrator |
| [P1 (#12)](https://github.com/markgar/commonplace/issues/12) | Apply and inspect vocabulary in a safely opened store | P0 | Shared start, then knowledge lane |
| [P2 (#13)](https://github.com/markgar/commonplace/issues/13) | Prove the selected inference and graph components work together | P0 | Bounded runtime work |
| [P3 (#15)](https://github.com/markgar/commonplace/issues/15) | Ingest files and read exact, revisioned evidence | P1, P2 inference gate | Source lane |
| [P4 (#19)](https://github.com/markgar/commonplace/issues/19) | Search current evidence through the complete hybrid pipeline | P3 | Source lane |
| [P5a (#16)](https://github.com/markgar/commonplace/issues/16) | Query, inspect, and rebuild the graph safely | P1, P2 graph gate | Knowledge lane |
| [P5b (#14)](https://github.com/markgar/commonplace/issues/14) | Record entities and cited types with coordinated graph publication | P3, P5a | Knowledge lane |
| [P6 (#17)](https://github.com/markgar/commonplace/issues/17) | Record cited relationships and literal facts atomically | P5b | Knowledge lane |
| [P7 (#20)](https://github.com/markgar/commonplace/issues/20) | Withdraw knowledge without leaving invalid active facts | P6 | Knowledge lane |
| [P8 (#21)](https://github.com/markgar/commonplace/issues/21) | Remove sources while preserving authored knowledge | P5b | Integration handoff |
| [P9 (#18)](https://github.com/markgar/commonplace/issues/18) | Ingest stdin, JSON Lines, and manifest inputs | P3 | Source lane |
| [P10 (#22)](https://github.com/markgar/commonplace/issues/22) | Demonstrate the complete release on supported targets | P4, P7, P8, P9 | Integrator and packet owners |

Recommended schedule:

1. Complete P0; run P1 and the bounded P2 preflight concurrently.
2. Run P3 ingestion and P5a graph commands concurrently once their respective
   prerequisites pass.
3. After P3, run P4 then P9 in the source lane. After both P3 and P5a, run P5b
   then P6 then P7 in the knowledge lane.
4. Start P8 after P5b as soon as an owner is available; it does not depend on P6
   or P7. The later of P6 and P8 owns their combined fact/removal acceptance.
   Coordinate edits to the graph publication boundary rather than assigning two
   owners to it.
5. Finish P10 against the integrated release candidate.

P9 may precede P4 if streamed input is more urgent; neither depends on the other.
P5a does not need ingestion; P5b needs evidence IDs but not search or streaming
adapters. P2 does not block vocabulary work. Local inference feasibility is
checked early in P2; unverified target portability is an explicit risk, not
evidence of success. Development can proceed on the proven host, but required
target evidence still gates P10. Final application packaging and clean-target
acceptance remain in P10; do not repeat the release exercise for every packet.

### 3.3 P0: execution kickoff

**Outcome:** the first implementers can start without inventing conflicting
contracts. This is a decision checkpoint, not an infrastructure coding project.

- Confirm ownership of shared files: `Cargo.toml`, `Cargo.lock`,
  `src/cli/mod.rs`, `src/cli/output.rs`, `src/error.rs`,
  `src/storage/database.rs`, and `src/storage/schema.sql`.
- Agree the P1 command input/output examples and typed errors. Preserve the
  shipped `init` response unless an explicit contract change is approved.
- Agree the shared output convention, including the specified `--json` flag
  alongside today's JSON-by-default behavior. Select the validator-backed input
  description mechanism and JSON Schema dialect for P1 to implement and later
  commands to reuse. Do not leave these shared decisions to each packet.
- Assign store opening, transaction entry, writer locking, and operation
  timestamps to P1, where schema application first consumes them.
- Assign document values, canonical JSON/digests, and evidence hydration to P3;
  inference integration to P2/P3/P4; graph runtime and rebuild publication to
  P5a; coordinated knowledge-write publication and evidence projection to P5b.
  Reuse the existing tagged IDs rather than creating another identifier model.
- Record remaining choices with the packet that resolves them, as specified in
  section 4. Do not finalize every future Rust type or command schema now.

**Exit:** owners, prerequisites, shared CLI conventions, P1 examples, and the
P2 probe scope are agreed. The Oxigraph adoption now fixes graph storage/lock
paths and version-2 compatibility in persistence section 13. P5a implements that
replacement rather than reopening the engine/layout decision.
Obtain approval before implementing new public contracts, changing a durable
format, or running platform packaging. Approval of this plan is not approval to
introduce unlisted commands, providers, migrations, or broad platform CI.

### 3.4 P1: vocabulary and the first shared write path

**Outcome:** initialize a store, check a vocabulary without mutation, apply it,
and read it back through `schema show`.

**Work:** implement `schema apply`, `--check`, generated `--describe --json`,
and `schema show`. Validate additive terms, identifier schemes, predicate kinds,
and compatible endpoints. Add only the store-opening, read/write-session,
bounded process-level writer-lock, timestamp, and CLI serialization support
these commands consume. Verify format and required extensions before mutation;
do not migrate an incompatible initialized store.

Generalize the internal response payload beyond `InitResult` while preserving
the shipped `init` JSON shape. Establish the shared binary-invocation tests in
`tests/`, using `CARGO_BIN_EXE_commonplace`, temporary stores, and child-process
helpers needed by the lock tests. Later packets extend these fixtures rather
than creating parallel harnesses. Implement the shared output/description
conventions agreed in P0 through these first consumers.

**Primary surfaces:** `app/schema`, `domain/schema`, `storage/database`,
`storage` vocabulary queries, and CLI input/output. Module names are ownership
guidance, not a requirement to create every file.

**Acceptance:** through the executable, apply a schema, reopen, and read the
same vocabulary. A no-op preserves `schema_version`; an endpoint-only addition
increments it exactly once. Invalid redefinitions and mixed valid/invalid
requests leave SQLite unchanged. `--check` and description commands do not
mutate state or load inference. Real SQLite tests cover constraints and
operation timestamps; process tests cover writer contention and release on
termination. Keep initialization behavior covered.

### 3.5 P2: integrated runtime preflight

**Outcome:** establish that the chosen native components can support the next
slices together before production ingestion depends on them.

**Status: local PASS, committed; no repeat spike requested.** The evidence commit
in section 2 completes the local graph and inference gates. Integrate that
evidence with the adoption specifications before dependent implementation; do
not mistake spike code for a production adapter.

**Graph gate evidence:** native read-only mutation rejection, query/update
separation, disabled HTTP, genuinely lazy SELECT, typed RDF results, distinct
knowledge identities and exact citations, persisted named-graph version metadata,
close/reopen, scoped cancellation, multi-process locks, and native-directory
publication. Real SQLite commit failure restores the previous directory;
enumerated process-crash windows fail closed and explicit rebuild restores
parity. The production mapping includes all source fields required by product
and persistence, not just the spike's compact fixture.

The lazy iterator bounds consumed final solutions and retained output, not
internal joins/sorts/aggregates or individual value size. Native cancellation is
cooperative: the recorded 20 ms requests completed in approximately 283-299 ms.
These are observations, not a promised upper bound. Bare minimum-i64 expressions
may be unbound; canonical typed storage and explicit typed literals preserve
the exact value. Do not patch the engine or rewrite queries to hide this.

**Inference gate evidence:** the combined Oxigraph/SQLite/inference executable
passed embedding dimensions/normalization, vector binding and transactions,
reranking, and relocated offline cached execution using the pinned models.
Inference providers/revisions are unchanged. Retain the report's exact artifact
and cache identities; production Cargo files own integration pins.

| Target | Selected combined runtime evidence |
| --- | --- |
| macOS arm64 | Local PASS; independent clean-machine packaging unverified |
| Windows x64 | UNVERIFIED |
| Windows arm64 | UNVERIFIED |
| Linux x64 | UNVERIFIED |
| Linux arm64 | UNVERIFIED |

Use additional local machines only when available and authorized. Do not dispatch
Actions to fill gaps or count prior engine results as Oxigraph evidence.

A passing local inference gate permits P3 on that host. Missing target execution
remains explicitly unverified and blocks release, not local feature development.
A demonstrated required-target incompatibility reopens the component decision
before dependent representation work continues. Retain the portability risk until
P10 obtains real target evidence; do not claim cross-compilation proves execution.

Define the real-model test convention once: a dedicated ignored integration-test
target, explicitly invoked against a prepared pinned cache in local runs.
Record its exact command and cache setup when P3 introduces the
production tests. Normal README development checks use deterministic inference and do not
download model weights. Explicit real-model runs fail if weights are absent or
incompatible; they must not silently skip or count as passing. P2 owns the spike
evidence, P3 owns the first production invocation, and P4 extends it for retrieval.

**Primary surfaces:** existing `spikes/` harnesses and `docs/spikes/` reports.
Production providers are wired when P3 and P4 consume them; this packet does not
add unused provider registries or a public model-preparation command.

**Acceptance:** record each gate separately with commands, immutable component
and model revisions, artifacts, dynamic dependencies, and results. The inference
gate names the verified local host and includes target-by-target evidence;
missing target execution is marked unverified and remains a P10 release blocker.
Verify local inference with networking unavailable after
cache preparation and inspect the provider path to ensure source text is never
sent to a remote service. A blocked download is a reported blocker, not permission
to change providers or bypass network policy. A required API failure reopens the
affected decision before dependent code is written. Final application packaging
and clean-target release acceptance remain P10.

### 3.6 P3: file ingestion and authoritative evidence reads

**Outcome:** ingest one file or a directory, rerun safely, revise a source, and
resolve returned document, revision, and passage IDs through `get`.

**Work:** implement exact UTF-8 input, canonical metadata and revision digests,
the fixed paragraph-aware passage algorithm, and the pinned local embedding
provider. Complete publication of revisions, passages, FTS rows, and vectors
before reporting success. Compute embeddings before the write transaction,
then recheck the current revision under the writer lock. Preserve previous
revisions while replacing only the current search rows.

Support one or many files, directory recursion and include/exclude filters,
source identity parity, symlink policy, bounded input enumeration, and independent
document outcomes. Implement complete document/revision/passage `get` responses
in one read session. Add generated ingestion descriptions for supported inputs.

**Primary surfaces:** `adapters/sources`, `domain/documents`, `domain/passages`,
`storage/documents`, `storage/evidence`, search-index writes, `providers`,
`app/ingest`, `app/get`, and CLI wiring.

**Acceptance:** a CLI fixture ingests multiple files, reruns unchanged, updates
one source, and reads both revisions with exact byte slices. Metadata-only
changes create revisions. Verify CRLF, BOM, NUL, combining marks, non-BMP text,
empty content, oversized paragraphs, invalid UTF-8, and configured limits.
A failed item preserves successful items. Injected publication failures roll
back ordinary, FTS, and vector rows together; searchable IDs equal current
passage IDs. Assert one model load per command, bounded embedding batches, and
no graph rebuild. A real-model smoke test supplements deterministic inference
substitutes used by pipeline tests, using P2's explicit invocation/cache
convention. Exercise the production ingestion path offline with prepared weights
and verify that source text remains on the local inference path.

### 3.7 P4: complete hybrid retrieval

**Outcome:** search an ingested corpus and receive ordered, fully hydrated exact
citations without follow-up reads.

**Work:** add lexical and vector candidates in one SQLite read session,
identical time/source-type filtering on both paths, deterministic fusion and
deduplication, the pinned local reranker, result limits, and explicit truncation.
Reuse P3 evidence hydration. Select and pin the retrieval constants in section 4
using a small checked-in relevance fixture rather than adding a tuning platform.

**Primary surfaces:** `domain/search`, `storage/search`, `providers/reranker`,
`app/search`, and CLI wiring.

**Acceptance:** real FTS5/sqlite-vec tests demonstrate contribution from both
candidate paths, filter parity, duplicate elimination, stable ties, bounded
reranking, and exact citations. Cover empty results and limits at, below, and
above each boundary. Missing models and incompatible vectors fail explicitly,
never with lexical-only success. Verify the relevance fixture with the pinned
real models through P2/P3's explicit test invocation, as well as deterministic
pipeline tests; attach the real-model result to this packet's acceptance.

### 3.8 Graph commands and cited authoring

#### P5a: graph runtime, queries, and rebuild

**Outcome:** inspect the physical graph schema, query an initialized graph, and
explicitly rebuild it from committed SQLite state with version parity and safe
directory publication. This packet runs alongside ingestion and exposes no authoring
command.

**Work:** replace production Grafeo with stock Oxigraph, remove the old graph
dependency/adapter, and pin the evaluated baseline with only RocksDB enabled.
Update `init`, store format validation, SQLite's format marker/constraint, and
configuration together for `commonplace-store/2` and `commonplace-config/2`.
Reject version-1 stores without mutation; no migration or compatibility backend.
Preserve the `init` response field names while returning the new format value.

Implement `graph query`, `graph schema`, and `graph rebuild` against the adopted
product/persistence contracts. Add named-graph metadata initialization/validation,
SELECT-only parsing, native read-only handles, scoped native cancellation,
RDF-term serialization, and process termination. Rebuild owns deterministic snapshot reads,
candidate verification, publication locking, and safe activation from committed
state. Add only capabilities consumed by these commands; coordinated SQLite
commit/graph restoration first belongs to P5b, where knowledge writes need it.

**Primary surfaces:** `app/graph`, graph snapshot reads, `graph/`, initialization,
store/configuration validation, root Cargo manifests, and CLI wiring. Coordinate
shared-file changes with P3; do not replace its inference dependency work.

**Acceptance:** initialize, inspect, query, rebuild, and reopen an empty store
through the binary. Applying unused vocabulary leaves the default graph empty;
the metadata graph remains present. Use
real SQLite fixtures with active memberships to exercise nonempty rebuild and
canonical-ID parity without inventing a public seed command. A version mismatch
fails closed; missing/corrupt current and invalid metadata do too. Rebuild
restores parity without changing `knowledge_version`.
Build or activation failure preserves the previous graph. A reader in another
process prevents publication from replacing files it holds. Verify native
mutation rejection, unsupported query-form errors, cooperative budget cancellation,
prompt timer teardown, process termination, exact RDF-term encodings, and the
proven row-bound/truncation behavior. Version-1 opening, initialization, and
rebuild fail without modifying the old store. These checks accompany production
implementation; they are not another feasibility gate.

Do not prebuild future authoring forms or an unused general transaction
coordinator. P5b extends the snapshot with its real evidence path, and P6 extends
it with facts; each addition is complete before that write form becomes available.

#### P5b: entities, cited types, and coordinated publication

**Outcome:** record entities with multiple cited types, read their canonical
state, and query the same active knowledge through read-only SPARQL SELECT after reopen.

**Work:** implement entity resolution, alias/identifier corrections, type
membership authoring and evidence validation, plus entity/knowledge `get`.
Deliver these supported `record` forms with generated descriptions; reject
unsupported forms explicitly until P6 extends the validator. Include request-local
entity references and atomic multi-item writes, not a single-item shortcut.

Extend P5a's snapshot to include all supported active knowledge and its evidence,
read from the pending write transaction. Join candidate publication with the
SQLite commit, retaining and restoring the previous graph on reported commit
failure. These supported authoring forms must not report success without their
complete projection. Any required change to the existing store representation
needs explicit format approval.

**Primary surfaces:** `domain/knowledge`, `domain/evidence`,
`storage/knowledge`, `app/record`, `app/get`, `app/graph`, and `graph/`.

**Acceptance:** a CLI test ingests evidence, applies vocabulary, records a
multiply typed entity, traverses its citations, and repeats after reopen.
Ambiguous resolution, duplicate identifiers, invalid quotes/offsets, and invalid
request-local references reject the entire request. Bare entities stay out of
the graph until active; only entity types used by active knowledge are projected.
Metadata-only corrections do not rebuild it.

Use real Oxigraph and SQLite to verify build/activation failure rollback and
restoration after reported SQLite commit failure. Simulate the activation/commit
crash window and prove queries fail closed until explicit rebuild. Reuse P5a's
reader/publication tests against authoring and add end-to-end citation traversal.
Test seams may inject failures at infrastructure boundaries but must not replace
the databases being tested.

This is the highest integration-risk packet. Work through recording, snapshot
construction, publication failure paths, and CLI queries as internal checkpoints;
do not declare it complete or expose successful authoring before all are joined.

### 3.9 P6: relationships and literal facts

**Outcome:** one atomic request can create or resolve entities, add types, and
record cited relationships and literal assertions.

**Work:** extend the P5b validator, write plan, snapshot, and graph projection for
directed facts and supported literal kinds. Validate endpoints against the
resulting active state, canonicalize literals, retain schema-version provenance,
and support request-local references throughout the batch. Complete `record`
JSON and JSON Lines inputs, preserving whole-request atomicity rather than
copying ingestion's per-item semantics. Extend descriptions and authoritative
reads with the same executable validators.

**Primary surfaces:** P5a/P5b knowledge/graph modules and record input decoding.

**Acceptance:** the CLI records a cited relationship and literal decision in one
request and retrieves matching graph and SQLite IDs. Cover all literal kinds,
timestamp normalization, integer bounds, float rejection, invalid endpoints,
and one invalid late item rolling back the whole request. Repeated successful
writes get distinct IDs. Multiple evidence passages do not duplicate semantic
edges; evidence from an older source revision remains resolvable. Every knowledge
result includes support, even when empty. Repeat P5b publication-failure checks
with facts and verify only actively used vocabulary is projected. If P8 has
already landed, extend its removal tests to relationships and literal facts here;
otherwise P8 owns that combined acceptance.

### 3.10 P7: withdrawal and retained history

**Outcome:** withdraw one or several knowledge items while retaining exact
history and a valid remaining active graph.

**Work:** implement complete-batch validation against the resulting active state,
withdrawal timestamps and labels, and graph republication through P5b's existing
publication path. Do not introduce a second transaction coordinator.

**Primary surfaces:** `app/withdraw`, knowledge validation/storage, and CLI.

**Acceptance:** withdraw a fact and observe its absence from SPARQL results and its
retained subtype, evidence, and withdrawal state through `get`. Removing the
last permitted endpoint type fails unless dependent facts are also withdrawn
in that request; another permitted active type allows withdrawal. An invalid or
already withdrawn ID rejects the whole batch. Publication failures preserve
both the previous active state and graph.

### 3.11 P8: explicit source removal

**Outcome:** remove a source and all its evidence without silently withdrawing
authored knowledge, then reingest its key with a new document identity.

**Work:** implement `remove --source-key` using the existing writer and graph
publication path. Collect affected knowledge and passage IDs, delete virtual
index rows explicitly, delete the canonical document tree and evidence links,
increment the knowledge version, and rebuild the complete remaining projection.
Return detached-evidence counts and affected knowledge IDs.

**Primary surfaces:** `app/remove`, source/index/evidence storage, and CLI.

**Acceptance:** through the CLI, remove a multiply revised, cited source; verify
no source, revision, passage, FTS, vector, or evidence rows remain for it.
Affected knowledge remains active, possibly with empty support, in SQLite and
Oxigraph. Unrelated evidence survives. Failure rolls back deletion and publication;
reopen and reingestion preserve the specified identity semantics. Directory
absence remains unrelated to removal.

P8 can land after P5b using cited type memberships; it does not need to wait for
facts or withdrawal. The later of P6 and P8 must repeat removal acceptance with
relationships and literal facts, including retained active knowledge with empty
support. P10 requires both packets and this combined evidence.

### 3.12 P9: remaining source adapters

**Outcome:** external tools can submit exact text and stable source keys without
a new indexing path.

**Work:** add stdin, JSON Lines, and manifest adapters to P3's canonical pipeline.
Complete their generated descriptions, metadata handling, input limits, duplicate
key handling, and aggregate results. These are connector-facing inputs, not
built-in email, PDF, web, or service connectors.

**Primary surfaces:** source adapters, ingestion input validators, and CLI.

**Acceptance:** stream a non-file key, rerun unchanged, then change only metadata.
A malformed JSON Lines item or duplicate streamed key fails that item without
undoing prior publications. Invalid command options or manifest envelopes fail
before item processing. Verify bounded reading and document/request limits,
exact text preservation, and the same revision/index behavior as file ingestion.

### 3.13 P10: release closure

**Outcome:** a packaged executable completes the entire section 5 scenario on
each supported target, with release-gating evidence for section 6.

**Work:** extend the early integrated package proof to the actual application,
using available, authorized local target machines. Verify bundled native dependencies,
pinned model acquisition/cache behavior, offline execution after acquisition,
writer and publication locks, and clean-target execution without build tools or
an interpreter. Do not use Actions or paid remote runners without fresh approval.
Unavailable target machines leave an explicit release blocker; local development
checks are not a substitute for clean-target evidence.

**Acceptance:** execute the full scenario through the public binary from an
empty store, then reopen and reproduce results. Account for every section 6
check with an automated test or explicit manual evidence; no untested gate is
marked passed. Run the README development checks. Update shipped-behavior
documentation and record exact artifacts, component/model revisions, target
results, and any remaining blockers. Missing target evidence blocks release,
not completion of an otherwise accepted feature packet.

### 3.14 Packet delivery and integration rules

- Use one accountable owner per packet. Start with two implementation lanes;
  assign additional agents only bounded independent work with a concrete
  acceptance result. Do not keep speculative implementation branches running
  ahead of unresolved dependencies.
- Before coding a packet, obtain an independent `rubber-duck` agent review of its
  plan against the existing code and owning specifications. Resolve blocking
  findings and record dispositions using the repo-local
  [`commonplace-progress` skill](../../.github/skills/commonplace-progress/SKILL.md).
  The coordinator then approves the concrete public input/output examples and
  any durable-format implications. Intermediate command descriptions advertise
  only implemented forms; no successful placeholder responses or fallback paths.
- Implement domain rules, persistence, CLI wiring, and focused tests together.
  Add abstractions only when that packet consumes them. A packet may require
  more than one PR, but internal checkpoints are not completed user capabilities.
- The integrator coordinates shared-file changes. P1 establishes the first
  reusable CLI and store boundaries; later packets extend them surgically.
  P5a owns graph runtime/rebuild publication and P5b joins knowledge commits;
  P6-P8 reuse that path. Changes to shared types require
  dependent callers and tests to be updated in the same integration.
- Every feature packet carries a real CLI acceptance test using temporary stores and
  the relevant section 6 failure/invariant checks. Use real SQLite and Oxigraph;
  substitute only expensive inference in routine pipeline tests and retain
  separate real-model checks. Grow the section 5 scenario incrementally instead
  of delegating all testing to a final lane.
- Run focused checks while developing and the README checks before declaring a
  code packet ready. Check the combined result after integrating concurrent
  work, not only each isolated branch. Document currently shipped behavior in
  the root README as each capability lands.
- Reuse fixtures and failure-injection seams already added by prior packets.
  Split a packet further only at a demonstrated dependency boundary, not by
  architectural layer. Stop expanding it when its acceptance passes.
- Implementation, PR creation, merge, and release are separate actions. This
  document does not authorize automatic merging or publication.

### 3.15 Progress tracking

The packet issue's `Progress` table is the live record: state, owner/session,
dependencies, blocker, PR, evidence, next action, and update date. Use
`Not started`, `Building`, `Ready for review`, `Blocked`, or `Integrated`.
`Blocked` requires a concrete cause and unblock action; a queued packet waiting
its ordinary dependencies can remain `Not started`.

Update the issue when work starts, becomes blocked, is ready for review, or
integrates. Preserve decisions and handoffs in short issue comments; do not log
every command. A commit, passing isolated checks, idle session, or closed issue
alone does not prove integration. Mark `Integrated` and close only after the
accepted work is merged into the integration base and combined acceptance is
confirmed; record the PR/merge commit and evidence. Local P2 proof and P10
release readiness remain separate.

The repo-local
[`commonplace-progress` skill](../../.github/skills/commonplace-progress/SKILL.md)
defines the start/update/handoff procedure for implementers and the coordinator.
Progress summaries report newly integrated work, active work, blockers or needed
approvals, and next eligible packets from freshly read issues/session state.
Do not estimate percentage complete or add a parallel dashboard/status document.

## 4. Bounded implementation decisions

The architecture fixes one implementation of each item below rather than
supporting runtime alternatives. Their concrete values are selected and
verified during the packaging and retrieval slices. Durable representation
choices are pinned by the store format; P4 completes the initial query policy
without changing the version-2 persisted representation:

- embedding model and runtime;
- embedding dimensions and normalization;
- vector extension and encoding;
- reranker model and runtime;
- passage target size;
- lexical tokenizer options;
- candidate limits;
- fusion formula and constants; and
- rerank limit.

P4 pins 64 candidates per path, equal-weight reciprocal-rank fusion with constant
60 and 1-based ranks, and at most 64 reranked passages in batches of eight.
FTS BM25 and native SQLite `vec_distance_L2` order ascending with passage-ID ties;
fusion orders descending with passage-ID ties; final reranker scores order
descending with fusion-order ties. One extra candidate per path proves omission.
The reranker is FastEmbed 7.1.0 with
`jinaai/jina-reranker-v1-turbo-en@b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2`,
512-token pairs and two intra-operation threads, verified against the P2 hashes.
These query-policy constants add no durable state and reinterpret no source,
embedding, lexical, vector, or graph bytes. Representation changes still require
the explicit treatment in persistence section 15.

The following operational limits are explicit configuration:

- maximum source bytes;
- maximum documents per request;
- maximum passages per document;
- embedding batch size;
- search result limit;
- graph result-row limit;
- graph query duration;
- writer-lock timeout; and
- maximum JSON input size.

P3's positive operational defaults are 10 MiB source bytes, 1,000 documents,
10,000 passages per document, embedding batches of 32, 1 MiB metadata JSON, and a
2,000 ms writer-lock timeout. The file command exposes these as
`--max-source-bytes`, `--max-documents`, `--max-passages`,
`--embedding-batch-size`, `--max-json-bytes`, and `--writer-lock-timeout-ms`.
They bound work without eagerly allocating from a caller-supplied limit.
The fixed 1024-byte passage representation is defined in persistence section 5,
not a per-request tuning option.

CLI input and result JSON schemas are finalized alongside the workflow that
first uses them. They are generated from the executable validators through the
`--describe --json` commands rather than duplicated as hand-maintained schema
documents.

Decision ownership and deadlines:

| Decision | Resolve in | Required evidence |
| --- | --- | --- |
| Compatible combined runtimes, immutable model revisions, embedding normalization, vector dimensions/encoding | P2 local inference gate, before P3 publication | Local combined-stack proof, real-model/vector checks, and explicit remaining target risks |
| Passage size and boundaries, canonical JSON/digest implementation, lexical tokenizer | P3, before writing source revisions | Golden representation tests and approved store-format treatment |
| Candidate limits, fusion constants, rerank limit, ordering | P4, before search completion | Deterministic tests and the pinned-model relevance fixture |
| Source, batch, passage, embedding, and JSON-input limits | First consuming packet: P1/P3, extended in P6/P9 | Boundary tests and explicit error examples |
| Writer timeout and graph publication behavior | P1, P5a rebuild, P5b knowledge commits | Real multi-process contention and failure tests |
| RDF mapping, metadata, directory/lock paths, and version-2 compatibility | Adopted in product/persistence; implement in P5a/P5b/P6 | Completed P2 local proof; production projection, publication, and format-rejection checks |
| SELECT/RDF result contract, lazy row bound, and cooperative cancellation mechanism | Adopted in product/architecture; implement in P5a | Completed P2 local proof; exact production output, cancellation, and truncation checks |
| Numerical graph row and duration budgets | P5a, before query completion | Boundary checks using the adopted mechanisms; no hard-timeout claim |
| Shared output flags and validator-backed descriptions | P0 agreement, P1 first implementation | Approved shared conventions, schema dialect, and unchanged init JSON shape |
| Binary test fixtures and real-model test execution | P1 harness; P2 convention, P3 production model tests | Shared process fixtures and an explicit pinned-cache invocation with no silent skips |
| Command-specific inputs, outputs, and errors | Each command's owning packet, reusing P1 conventions | Approved examples, executable validators, and CLI tests |

P5a's approved operational defaults are 1000 retained rows (`--row-limit`, with
zero allowed) and 5000 ms (`--timeout-ms`, positive) for cooperative evaluation
cancellation. Product section 8 owns their CLI behavior; they do not add fields
to the version-2 configuration. Acceptance covers zero/at/above row bounds,
overflow rejection, native lazy consumption, execution/iteration cancellation,
and prompt timer teardown.

The adopted `commonplace-store/2` marker is not proof that every representation
choice above has been finalized. P5a implements the approved replacement of
version 1 described in persistence; it does not silently repurpose that marker.
Before persisting any other newly selected representation, explicitly determine
whether it is compatible with initialized
stores. Obtain approval for any format change, update the owning specification,
and reject incompatible stores without mutation. Do not silently repurpose a
format identifier or add migration machinery to make development fixtures pass.

## 5. End-to-end acceptance scenario

A release candidate must pass this sequence from an empty store:

1. Initialize a local knowledge base.
2. Batch-ingest a directory of Markdown documents.
3. Stream one document with a stable non-file source key.
4. Rerun ingestion and report unchanged documents without duplicates.
5. Change one source and retain its previous immutable revision.
6. Run hybrid search and receive ordered exact citations.
7. Apply a user-defined schema.
8. Record an entity with multiple active types.
9. Record an evidence-backed entity relationship.
10. Record a cited literal decision.
11. Query the graph with read-only SPARQL SELECT and return canonical evidence
    references.
12. Withdraw one fact and confirm that it disappears from active graph queries
    while remaining readable from history.
13. Remove one source and confirm complete source deletion, evidence detachment,
    and graph reconstruction.
14. Reingest the removed source key and receive a new document identity.
15. Close and reopen the application and reproduce search and graph results.

## 6. Release-gating checks

### Ingestion and evidence

- One embedding-model load serves a multi-document command.
- Embedding batches remain within the configured bound.
- A failed document does not roll back successful documents in the same batch.
- A malformed JSON Lines record or repeated streamed source key fails that item
  without rolling back earlier documents.
- Direct and directory ingestion of the same file use the same canonical source
  key.
- FTS and vector results correlate exactly by `passage_id`.
- FTS and vector row IDs equal the current searchable passage IDs.
- Every citation selects the exact UTF-8 byte range from its revision.
- Changed metadata creates a new revision even when source text is unchanged.
- Missing files during directory ingestion do not delete stored sources.

### Search

- Hybrid search uses lexical and vector candidates.
- Fusion is deterministic and removes duplicate passages.
- Reranking operates on a bounded candidate set.
- Returned evidence is complete without one follow-up read per result.
- Missing models or incompatible representations produce explicit errors.
- Result and filter limits are enforced.

### Knowledge

- Schema application is additive and atomic.
- An endpoint-only schema change increments `schema_version` once.
- Entities may hold multiple active types.
- Identifier uniqueness and ambiguous name resolution are enforced.
- Evidence quote and offset mismatches reject the whole authoring request.
- Validation or graph-build failure rolls back the whole request.
- Duplicate successful knowledge writes receive distinct IDs.
- Alias and identifier corrections are atomic and require a canonical entity ID.
- Withdrawal is atomic and retained in history.

### Graph

- The projection contains all active knowledge and no withdrawn knowledge.
- Only schema terms used by active knowledge are projected; reserved version
  metadata is separate from user vocabulary and remains present for an empty graph.
- Ordinary document ingestion does not rebuild the graph.
- Evidence traverses knowledge to passage, exact cited revision, and document.
- Equal facts retain distinct knowledge IRIs and their own evidence links.
- Graph and SQLite versions must match before query execution.
- Invalid/missing version metadata and missing/corrupt current graphs fail closed.
- Compatible derived-state failures are recoverable with `commonplace graph rebuild`.
- Graph publication cannot replace files held by an active graph reader.
- Native read-only handles reject mutations; the public parser accepts SELECT only.
- HTTP/remote SERVICE execution remains disabled.
- Lazy iteration consumes at most `row_limit + 1` solutions, with no injected LIMIT.
- RDF-term JSON preserves column order, unbound nulls, and exact typed literals.
- Budget expiry requests native cancellation through execution and iteration;
  observed cancellation returns `limit_exceeded` rather than partial success.
- Timer teardown is prompt on completion, truncation, and errors; no hard
  wall-clock or internal working-memory bound is claimed.
- Terminating the synchronous CLI process stops an executing graph query.
- A graph activation failure leaves SQLite unchanged.
- A reported SQLite commit failure restores the previous graph.

### Persistence and packaging

- Foreign keys and `STRICT` tables are active on every connection.
- The process-level writer lock works on every supported platform.
- Incompatible store formats are rejected without mutation.
- Rows written by one operation share one application-supplied timestamp.
- The packaged executable loads SQLite extensions and Oxigraph on a clean target.
- Commands that do not use inference do not load model weights.
- Source text never leaves the local inference path.

## 7. Completion criteria

The first release is ready when:

- the end-to-end scenario passes through the public CLI;
- all release-gating checks are automated where practical;
- exact dependency and model versions are pinned;
- a clean-machine package test passes for every supported target;
- CLI JSON schemas are documented and tested; and
- the implementation contains no second storage, vector, model, graph, or
  compatibility path.
