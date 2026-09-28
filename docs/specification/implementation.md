# Commonplace implementation and acceptance

## 1. Delivery objective

Implementation proceeds through vertical slices that each produce usable CLI
behavior. The first release is complete when a person can ingest local sources,
search exact evidence, author cited knowledge, query it through Cypher, withdraw
knowledge, and reproduce the same results after reopening the application.

## 2. Technology gate

Before release packaging, a disposable Rust spike must prove that the selected
components can ship together as a self-contained distribution:

- Grafeo 0.5.43 with the `lpg` feature profile;
- SQLite with FTS5;
- one SQLite vector extension;
- one local embedding model and runtime;
- one local reranker and runtime;
- native role-scoped read-only Cypher enforcement;
- native query deadlines and process-level cancellation; and
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

Component feasibility is established but the integrated gate remains open. The
[Rust packaging spike](../spikes/rust-packaging.md) proves SQLite, sqlite-vec,
inference, reranking, and the superseded Ladybug engine together on macOS
arm64. The [Grafeo evaluation spike](../spikes/grafeo-evaluation.md) proves the
selected graph engine separately on every intended OS and architecture. The
packaging slice must replace Ladybug with Grafeo and rerun the complete bundle
before release.

The first release supports:

- macOS arm64;
- Windows x64 and arm64; and
- Linux x64 and arm64.

macOS x64 is not a release target.

## 3. Execution plan

### 3.1 Starting point and planning boundary

The planning baseline is commit `772f7424f6cd`. The application currently exposes
only `init`. It includes the SQLite table definitions, FTS5 and sqlite-vec
initialization, an empty persistent Grafeo store, tagged IDs, a canonical file
source-key helper, and initialization tests. Those are starting assets, not
evidence that ingestion, knowledge publication, or release acceptance is done.
In particular, graph version metadata, publication locks, operational writer
locking, inference, and the remaining public workflows still need integration.

This plan changes delivery order, not product scope or durable semantics.
[Product](product.md), [architecture](architecture.md), and
[persistence](persistence.md) retain their ownership under the
[specification hierarchy](README.md). Sections 5-7 below remain the release
acceptance contract. Spike reports supply implementation evidence, not substitute
requirements.

Optimize elapsed delivery time by keeping two implementation lanes moving after
a small shared starting slice. Do not divide work into independent CLI, domain,
database, and testing projects. Each packet owns its user path and its tests.
Knowledge writes and their graph publication ship together; a SQLite-only
authoring command is not an intermediate deliverable.

### 3.2 Dependency map and scheduling

All packets below are planned, not completed. Update a packet with its PR or
commit and acceptance evidence when it is delivered. A dependency means the
prerequisite packet or named gate has been integrated, not merely that somebody
has started it. P2 has separate graph and inference gates so an unrelated
runtime question does not unnecessarily block both lanes.

| Packet | User-visible outcome or gate | Depends on | Primary ownership |
| --- | --- | --- | --- |
| P0 | Agree the first execution boundaries and unresolved decisions | Baseline inspection | Integrator |
| P1 | Apply and inspect vocabulary in a safely opened store | P0 | Shared start, then knowledge lane |
| P2 | Prove the selected inference and graph components work together | P0 | Bounded runtime work |
| P3 | Ingest files and read exact, revisioned evidence | P1, P2 inference gate | Source lane |
| P4 | Search current evidence through the complete hybrid pipeline | P3 | Source lane |
| P5a | Query, inspect, and rebuild the graph safely | P1, P2 graph gate | Knowledge lane |
| P5b | Record entities and cited types with coordinated graph publication | P3, P5a | Knowledge lane |
| P6 | Record cited relationships and literal facts atomically | P5b | Knowledge lane |
| P7 | Withdraw knowledge without leaving invalid active facts | P6 | Knowledge lane |
| P8 | Remove sources while preserving authored knowledge | P5b | Integration handoff |
| P9 | Ingest stdin, JSON Lines, and manifest inputs | P3 | Source lane |
| P10 | Demonstrate the complete release on supported targets | P4, P7, P8, P9 | Integrator and packet owners |

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
adapters. P2 does not block vocabulary work. Required-target inference
feasibility is checked early in P2, before committing to its persisted
representation. Final application packaging and clean-target acceptance remain
in P10; do not repeat the entire release exercise for every packet.

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
P2 probe scope are agreed. P2 names graph storage/lock paths and compatibility
implications before P5a changes the initialized layout; no layout change is
presumed necessary.
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

**Work:** reuse the existing spike harnesses, replacing the superseded Ladybug
component in the combined probe with Grafeo. Start from the model revisions and
runtime evidence in the packaging report, but pin production dependencies in
Cargo rather than treating the report as a dependency manifest. Verify embedding
dimensions, normalization, vector binding, reranking, offline cached execution,
and lazy model loading in the same executable as Grafeo and SQLite.

**Graph gate:** exercise the APIs needed by P5a: native role-scoped mutation rejection,
version metadata persistence, close/reopen, deadline errors, result iteration
and serialization, and candidate-file replacement after handles close. Deliver
the demonstrated version-metadata carrier, current/candidate/lock path layout,
and any compatibility implications. Do not assume that an extra file requires
changing `config.json`, or choose a sidecar/reserved node without checking the
specified projection and publication semantics.

Demonstrate the actual mechanism for bounded graph results; an iterator over
already materialized rows does not prove bounded engine retrieval. Post-hoc
truncation must not silently weaken the product's read bound, and rewriting
Cypher with a `LIMIT` requires evidence that query semantics are preserved.
If the engine cannot satisfy the specified behavior, report the mismatch for
an explicit component or specification decision rather than designing a fallback.

**Inference gate:** verify the combined inference/native stack on every required
target, including immutable model acquisition, embedding and reranking execution,
and offline cached operation. The existing reports prove Grafeo across targets
but the combined inference stack only on macOS arm64. Reuse that evidence and
the existing manual workflow scaffolding, targeting the missing combined-stack
proof rather than repeating standalone Grafeo evaluation. The current workflow
covers Linux x64/arm64 and Windows arm64; it is not already a five-target matrix.
Obtain approval for the exact manual platform runs and workflow changes first.
An unresolved required-target failure blocks the inference gate and P3's
representation commitment, not P1 or a separately proven graph gate.

Define the real-model test convention once: a dedicated ignored integration-test
target, explicitly invoked against a prepared pinned cache in local or authorized
manual runs. Record its exact command and cache setup when P3 introduces the
production tests. Normal README/CI checks use deterministic inference and do not
download model weights. Explicit real-model runs fail if weights are absent or
incompatible; they must not silently skip or count as passing. P2 owns the spike
evidence, P3 owns the first production invocation, and P4 extends it for retrieval.

**Primary surfaces:** existing `spikes/` harnesses and `docs/spikes/` reports.
Production providers are wired when P3 and P4 consume them; this packet does not
add unused provider registries or a public model-preparation command.

**Acceptance:** record each gate separately with commands, immutable component
and model revisions, artifacts, dynamic dependencies, and results. The inference
gate includes target-by-target evidence; missing execution is a blocker, not a
pass deferred to P10. Verify local inference with networking unavailable after
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
file publication. This packet runs alongside ingestion and exposes no authoring
command.

**Work:** implement `graph query`, `graph schema`, and `graph rebuild`, consuming
P2's proven metadata, path-layout, and bounded-result decisions. Add graph version
initialization/validation, native read-only roles, deadlines, deterministic value
serialization, and process termination. Rebuild owns deterministic snapshot reads,
candidate verification, publication locking, and safe activation from committed
state. Add only capabilities consumed by these commands; coordinated SQLite
commit/graph restoration first belongs to P5b, where knowledge writes need it.

**Primary surfaces:** `app/graph`, graph snapshot reads, `graph/`, initialization,
and CLI wiring.

**Acceptance:** initialize, inspect, query, rebuild, and reopen an empty store
through the binary. Applying unused vocabulary leaves the graph empty. Use
real SQLite fixtures with active memberships to exercise nonempty rebuild and
canonical-ID parity without inventing a public seed command. A version mismatch
fails closed; rebuild restores parity without changing `knowledge_version`.
Build or activation failure preserves the previous graph. A reader in another
process prevents publication from replacing files it holds. Verify native
mutation rejection, deadlines, process cancellation, specified value encodings,
unsupported-value errors, and the proven row-bound/truncation behavior.

Do not prebuild future authoring forms or an unused general transaction
coordinator. P5b extends the snapshot with its real evidence path, and P6 extends
it with facts; each addition is complete before that write form becomes available.

#### P5b: entities, cited types, and coordinated publication

**Outcome:** record entities with multiple cited types, read their canonical
state, and query the same active knowledge through read-only Cypher after reopen.

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

Use real Grafeo and SQLite to verify build/activation failure rollback and
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

**Acceptance:** withdraw a fact and observe its absence from Cypher and its
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
Grafeo. Unrelated evidence survives. Failure rolls back deletion and publication;
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
using authorized manual platform runs. Verify bundled native dependencies,
pinned model acquisition/cache behavior, offline execution after acquisition,
writer and publication locks, and clean-target execution without build tools or
an interpreter. Preserve ordinary CI's focused scope.

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
- Before coding a packet, approve its concrete public input/output examples and
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
  the relevant section 6 failure/invariant checks. Use real SQLite and Grafeo;
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

## 4. Bounded implementation decisions

The architecture fixes one implementation of each item below rather than
supporting runtime alternatives. Their concrete values are selected and
verified during the packaging and retrieval slices, then pinned as part of the
store format:

- embedding model and runtime;
- embedding dimensions and normalization;
- vector extension and encoding;
- reranker model and runtime;
- passage target size;
- lexical tokenizer options;
- candidate limits;
- fusion formula and constants; and
- rerank limit.

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

CLI input and result JSON schemas are finalized alongside the workflow that
first uses them. They are generated from the executable validators through the
`--describe --json` commands rather than duplicated as hand-maintained schema
documents.

Decision ownership and deadlines:

| Decision | Resolve in | Required evidence |
| --- | --- | --- |
| Compatible combined runtimes, immutable model revisions, embedding normalization, vector dimensions/encoding | P2 inference gate, before P3 publication | Required-target combined-stack evidence and real-model/vector checks |
| Passage size and boundaries, canonical JSON/digest implementation, lexical tokenizer | P3, before writing source revisions | Golden representation tests and approved store-format treatment |
| Candidate limits, fusion constants, rerank limit, ordering | P4, before search completion | Deterministic tests and the pinned-model relevance fixture |
| Source, batch, passage, embedding, and JSON-input limits | First consuming packet: P1/P3, extended in P6/P9 | Boundary tests and explicit error examples |
| Writer timeout and graph publication behavior | P1, P5a rebuild, P5b knowledge commits | Real multi-process contention and failure tests |
| Graph metadata carrier, current/candidate/lock paths, and compatibility | P2 graph gate, before P5a implementation | Persist/reopen/publication proof and explicit format assessment |
| Graph row-bound mechanism, duration limits, and value encoding | P2 graph gate, P5a implementation | Semantics-preserving native API proof, exact output tests, timeout and truncation tests |
| Shared output flags and validator-backed descriptions | P0 agreement, P1 first implementation | Approved shared conventions, schema dialect, and unchanged init JSON shape |
| Binary test fixtures and real-model test execution | P1 harness; P2 convention, P3 production model tests | Shared process fixtures and an explicit pinned-cache invocation with no silent skips |
| Command-specific inputs, outputs, and errors | Each command's owning packet, reusing P1 conventions | Approved examples, executable validators, and CLI tests |

The existing `commonplace-store/1` marker is not proof that every representation
choice above has been finalized. Before persisting a newly selected
representation, explicitly determine whether it is compatible with initialized
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
11. Query the graph with read-only Cypher and return canonical evidence
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
- Only schema terms used by active knowledge are projected.
- Ordinary document ingestion does not rebuild the graph.
- Evidence can be traversed directly from knowledge to passage and source.
- Graph and SQLite versions must match before query execution.
- A version mismatch is recoverable with `commonplace graph rebuild`.
- Graph publication cannot replace files held by an active graph reader.
- Cypher mutations are rejected natively.
- Row limits are enforced.
- An over-budget query returns Grafeo's native timeout error.
- Terminating the synchronous CLI process stops an executing graph query.
- A graph activation failure leaves SQLite unchanged.
- A reported SQLite commit failure restores the previous graph.

### Persistence and packaging

- Foreign keys and `STRICT` tables are active on every connection.
- The process-level writer lock works on every supported platform.
- Incompatible store formats are rejected without mutation.
- Rows written by one operation share one application-supplied timestamp.
- The packaged executable loads SQLite extensions and Grafeo on a clean target.
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
