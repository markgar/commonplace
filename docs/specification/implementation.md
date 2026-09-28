# Commonplace implementation and acceptance

## 1. Delivery objective

Implementation proceeds through vertical slices that each produce usable CLI
behavior. The first release is complete when a person can ingest local sources,
search exact evidence, author cited knowledge, query it through Cypher, withdraw
knowledge, and reproduce the same results after reopening the application.

## 2. Technology gate

Before production implementation, a disposable Rust spike must prove that the
selected components can ship together as a self-contained distribution:

- official Ladybug integration;
- SQLite with FTS5;
- one SQLite vector extension;
- one local embedding model and runtime;
- one local reranker and runtime;
- native read-only Cypher enforcement;
- real graph-query cancellation; and
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

The completed macOS arm64 technology-gate evidence and pinned selections are
recorded in [the Rust packaging spike](../spikes/rust-packaging.md). The
cross-platform graph-engine evaluation, including an independent Windows x64
package test, is recorded in
[the Grafeo evaluation spike](../spikes/grafeo-evaluation.md).

## 3. Implementation sequence

1. **Packaging spike**

   Prove the complete native dependency chain and distribution model.

2. **Store and source slice**

   Implement `kg init`, store-format validation, source identity, immutable
   revisions, and one-document and batch transactions.

3. **Ingestion slice**

   Add passage generation, FTS rows, vector rows, bounded embedding batches, and
   complete `kg ingest` results.

4. **Retrieval slice**

   Add lexical and vector candidate retrieval, fusion, deduplication, reranking,
   filters, and exact citation hydration.

5. **Schema and entity slice**

   Add vocabulary application, identifiers, aliases, exact entity resolution,
   and multiple active entity types.

6. **Knowledge slice**

   Add atomic relationship and literal-fact authoring, evidence validation,
   history reads, and withdrawal.

7. **Graph slice**

   Add deterministic snapshot construction, complete Ladybug replacement,
   version checks, citation projection, read-only Cypher, and graph schema.

8. **Removal slice**

   Add explicit source deletion, evidence detachment, graph rebuild, and
   reingestion with a new identity.

9. **Streaming adapters**

   Add JSON Lines, manifest, and connector-facing ingestion after the canonical
   pipeline is stable.

Each slice includes CLI tests and the lowest-level tests needed to prove its
invariants. Domain layers and repositories are not built ahead of a workflow
that uses them.

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
- At least one bounded embedding call contains passages from multiple documents.
- A failed document does not roll back successful documents in the same batch.
- FTS and vector results correlate exactly by `passage_id`.
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
- Entities may hold multiple active types.
- Identifier uniqueness and ambiguous name resolution are enforced.
- Evidence quote and offset mismatches reject the whole authoring request.
- Validation or graph-build failure rolls back the whole request.
- Duplicate successful knowledge writes receive distinct IDs.
- Aliases and identifiers remain add-only.
- Withdrawal is atomic and retained in history.

### Graph

- The projection contains all active knowledge and no withdrawn knowledge.
- Only schema terms used by active knowledge are projected.
- Ordinary document ingestion does not rebuild the graph.
- Evidence can be traversed directly from knowledge to passage and source.
- Graph and SQLite versions must match before query execution.
- Cypher mutations are rejected natively.
- Row limits are enforced.
- An over-budget query is actually cancelled.
- A graph activation failure leaves SQLite unchanged.
- A reported SQLite commit failure restores the previous graph.

### Persistence and packaging

- Foreign keys and `STRICT` tables are active on every connection.
- The process-level writer lock works on every supported platform.
- Incompatible store formats are rejected without mutation.
- The packaged executable loads SQLite extensions and Ladybug on a clean target.
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
