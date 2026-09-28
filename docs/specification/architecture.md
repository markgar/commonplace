# Commonplace software architecture

## 1. Architecture goal

Commonplace is implemented in Rust as a small command-line application with
clear ownership of source data, retrieval, authored knowledge, and graph
projection.

The architecture must make these changes local:

- add a source adapter without changing ingestion;
- change passage generation without changing source storage;
- replace an embedding model or reranker without changing search orchestration;
- change vector SQL without changing CLI commands;
- evolve user vocabulary without database DDL generation;
- change the graph projection without changing canonical authoring; and
- add CLI conveniences without moving business rules into the CLI.

The CLI JSON interface is the stable external contract. Internal modules may
evolve as the implementation becomes clearer.

## 2. Dependency direction

```text
CLI and source adapters
        |
        v
Application workflows
        |
        v
Domain values and rules
        |
        +----------------------+
        |                      |
        v                      v
SQLite persistence       External engines
                         embeddings, reranking, Oxigraph
```

Dependencies point inward:

- domain code imports no CLI, SQLite, inference runtime, or Oxigraph package;
- workflows depend on domain values and narrow infrastructure capabilities;
- infrastructure implements storage and external-runtime capabilities;
- CLI code depends on workflows and serialization models; and
- one composition root constructs all concrete resources.

No module creates hidden database connections, model providers, or graph
instances.

## 3. Package structure

```text
src/
  main.rs
  cli/
    mod.rs
    input.rs
    output.rs
  app/
    mod.rs
    context.rs
    ingest.rs
    remove.rs
    search.rs
    schema.rs
    record.rs
    withdraw.rs
    get.rs
    graph.rs
  domain/
    mod.rs
    ids.rs
    documents.rs
    passages.rs
    search.rs
    schema.rs
    knowledge.rs
    evidence.rs
  storage/
    mod.rs
    database.rs
    documents.rs
    evidence.rs
    search.rs
    knowledge.rs
    schema.sql
  providers/
    mod.rs
    embeddings.rs
    reranker.rs
  graph/
    mod.rs
    snapshot.rs
    oxigraph.rs
  adapters/
    mod.rs
    sources.rs
tests/
```

Modules may be combined when the resulting file remains cohesive. The structure
is an ownership guide, not a requirement to create empty abstractions.

## 4. Domain layer

The domain layer contains immutable values and deterministic rules.

### 4.1 Identifiers

Each durable identity is a distinct Rust newtype over `i64`:

```text
DocumentId
RevisionId
PassageId
EntityId
KnowledgeItemId
```

Distinct types prevent accidental cross-domain ID use. CLI codecs convert these
to and from tagged strings.

### 4.2 Source values

Core source values include:

```text
DocumentInput
DocumentRevision
Passage
EvidenceRef
```

Pure functions validate inputs, canonicalize metadata, calculate revision
digests, produce passages, and verify byte ranges.

### 4.3 Retrieval values

Retrieval values include query filters, lexical and vector candidates, fused
candidates, reranked candidates, and hydrated search results.

Fusion, deduplication, ordering, limit enforcement, and score normalization are
pure functions with explicit constants.

### 4.4 Schema and knowledge values

Schema values represent entity types, identifier schemes, predicates, endpoint
constraints, and literal kinds.

Knowledge values represent entity references, entity changes, type memberships,
entity facts, literal facts, evidence references, and withdrawal requests.
Validation produces a complete write plan before persistence begins.

## 5. Stateful components

Stateful components exist only when they own a resource, transaction, or
replaceable runtime.

### 5.1 Database

`SqliteDatabase` owns connection creation, store-format validation, SQLite
extensions, pragmas, and transaction entry points.

`SqliteReadSession` provides a consistent read view.

`SqliteWriteSession` owns one immediate write transaction and exposes cohesive
storage capabilities:

- `DocumentStore`;
- `EvidenceQueries`;
- `SearchIndexStore`;
- `SearchQueries`;
- `KnowledgeStore`; and
- `KnowledgeQueries`.

Storage modules return domain values rather than raw SQLite rows.
No repository method begins or commits a hidden transaction. Complex set
operations remain explicit parameterized SQL, with checked-in schema SQL and
small row decoders. The design does not use an ORM or generic repository layer.

### 5.2 Inference providers

`EmbeddingModel` and `Reranker` are narrow internal traits because they wrap
stateful external runtimes.

`LocalEmbeddingModel`:

- loads the pinned model lazily;
- retains one loaded session per process;
- accepts bounded text batches;
- returns fixed-dimension normalized vectors; and
- exposes its immutable representation identity.

`LocalReranker` follows the same lifecycle and returns deterministic ordering for
a fixed model and input.

There are no provider registries or runtime profile selectors.

### 5.3 Graph engine

`GraphRuntime` and its private Oxigraph projection/query modules form one direct
internal graph boundary. A separate engine trait is not required without a
current consumer; there is only one stock Oxigraph implementation. This boundary:

- builds a graph from a complete `GraphSnapshot`;
- verifies the candidate graph;
- activates a candidate while retaining the previous graph temporarily;
- restores the previous graph after an SQLite commit failure;
- executes read-only SPARQL SELECT with row and cooperative time budgets; and
- exposes graph schema information.

`GraphRuntime` owns the active Oxigraph resource for a CLI process.
It manages the current, candidate, and temporary previous directories defined in
[persistence section 13](persistence.md#133-layout-and-compatibility). Opening the
current graph occurs under the shared graph publication lock and verifies its
stored `knowledge_version` against SQLite before returning a query handle.
Candidate creation, activation, rollback, and cleanup remain private to this
boundary.

Use unmodified Oxigraph with default features disabled and only `rocksdb`
enabled. The evaluated baseline is 0.5.11; production versions are pinned in
Cargo manifests/lockfiles when integrated. Do not enable HTTP clients, remote
SERVICE handlers, RDF 1.2, or register application custom query functions.
The adapter uses native query parsing, accepts only SELECT, and never exposes
the update parser/API through the public query path. Queries open
`Store::open_read_only`; they never open/create a writable current database.

Candidate construction reads the caller's SQLite transaction rather than opening
a second connection that could miss uncommitted knowledge changes. Emit the
complete projection in bounded batches to a separate writable candidate, flush
it, and close all writer handles before read-only verification. Never write a
database directory while any process has it open read-only.

### 5.4 Application context

`AppContext` contains:

- database access;
- lazy embedding and reranking providers;
- graph runtime;
- immutable configuration; and
- clock or ID-independent utilities needed by workflows.

It is constructed once in the composition root and passed explicitly.

## 6. Application workflows

Workflows are functions grouped by user intent. They coordinate domain rules and
infrastructure without embedding SQL or CLI serialization.

### 6.1 Ingest documents

```text
ingest_documents(context, document_stream)
```

The workflow validates command-level input, streams documents, resolves current
revisions, skips unchanged documents, prepares passages, and publishes each
changed document in its own SQLite transaction. Embedding uses bounded batches;
combining passages from multiple documents is an optional optimization.

A malformed JSON Lines record or source key repeated earlier in the same stream
is a failed item. It does not roll back documents already published by that
command.

Within a document transaction it writes the document revision, passages, FTS
rows, and vector rows. When replacing the current revision, it explicitly
removes the prior revision's FTS and vector rows so only current passages remain
searchable. Any failure rolls back that document only.

### 6.2 Remove a document

```text
remove_document(context, source_key)
```

The workflow opens a write transaction, identifies affected knowledge items,
deletes the document tree and evidence links, increments `knowledge_version`,
builds and activates a candidate graph, commits SQLite, and then discards the
previous graph.

### 6.3 Search

```text
search_documents(context, query)
```

The workflow obtains lexical and vector candidates in one read session, fuses
and deduplicates them, reranks a bounded set, and hydrates exact evidence before
returning results.

### 6.4 Apply schema

```text
apply_knowledge_schema(context, request)
```

The workflow validates additive changes and endpoint compatibility, writes the
vocabulary, and increments `schema_version`. Schema terms unused by active
knowledge do not need graph projection.

### 6.5 Record knowledge

```text
record_knowledge(context, request)
```

The workflow resolves entities and evidence, validates the complete request,
writes entity metadata corrections and knowledge items, increments
`knowledge_version` when projected knowledge changes, builds and activates a
candidate graph when needed, and commits as one operation. Alias and identifier
removals require the canonical entity ID and do not rebuild the graph.

### 6.6 Withdraw knowledge

```text
withdraw_knowledge(context, request)
```

The workflow validates that the item exists and is active, marks it withdrawn,
increments `knowledge_version`, rebuilds the graph, and commits as one
coordinated operation with version-mismatch recovery.

### 6.7 Read records and query the graph

`get_record` reads canonical SQLite state, including withdrawn history and exact
evidence.

`query_graph` first verifies version parity, then executes a parsed SPARQL SELECT
through the native read-only store. It consumes at most `row_limit + 1` solutions
from the native lazy iterator, drops that iterator, removes any extra sentinel
row, and serializes RDF terms according to the product contract. Never collect
the full result before truncation or rewrite the query to inject LIMIT.
Serialization preserves native column order and lexical literal values; it does
not impose row order on a query without ORDER BY.

An operation-scoped timer starts immediately before evaluation and drives
Oxigraph's native cancellation token through execution and iteration. Normal
completion, row-limit termination, and error paths wake and join the timer
promptly instead of waiting for its budget to expire. This is scoped synchronous
operation support, not a background service. Native cancellation is cooperative;
the adapter does not promise a hard timeout or bounded engine working memory.
Native cancellation caused by budget expiry maps to `limit_exceeded`, with no
partial-success payload. Parse errors and unsupported query forms map to
`invalid_input`; store/version failures map to `graph_unavailable`. Other query
evaluation failures are explicit errors, not empty successful results; native
SPARQL unbound-expression and SERVICE SILENT semantics remain intact.

The synchronous CLI process terminates on
Ctrl+C; the architecture does not promise cancellation of one query while
keeping the process alive.

`rebuild_graph` acquires the writer lock, reads a complete snapshot from
committed SQLite state, builds and verifies a candidate graph, and publishes it
without changing `knowledge_version`.

## 7. Transaction and publication model

### 7.1 Source ingestion

Each changed document owns one SQLite transaction:

```text
begin
  -> write revision and passages
  -> write FTS and vector rows
  -> verify passage/index correlation
commit
```

Documents in the same request are independent. The embedding runtime is shared,
but uncommitted database state is not.

### 7.2 Knowledge changes

Removal, authoring, and withdrawal coordinate SQLite and Oxigraph:

```text
begin SQLite transaction
  -> apply canonical changes
  -> increment knowledge_version
  -> read complete graph snapshot
  -> build and verify candidate graph
  -> acquire exclusive graph publication lock
  -> close candidate verification handles and results
  -> rename current to previous, if current exists
  -> rename candidate to current and synchronize directory publication
  -> commit SQLite
  -> discard previous graph
  -> release graph publication lock
```

All renames remain on the same filesystem. Before activation every candidate
handle/result is closed, and the exclusive lock excludes current readers.
If graph construction fails, SQLite rolls back and current is untouched. If
activation fails after moving current, restore it before releasing the lock.
If SQLite commit reports failure, roll back SQLite and restore previous before
releasing the lock. Restoration failures are explicit and must not permit a
success response; subsequent graph reads still require a valid matching version.
Cleanup failure after successful SQLite commit is reported as post-commit cleanup
failure, not as if the authoritative transaction had rolled back.

The two directory renames and SQLite commit are not one atomic operation:

| Process crash window | Subsequent graph read |
| --- | --- |
| Before moving current | Existing complete current remains usable if versions match |
| Between directory renames | Missing current is rejected, never recreated by a read |
| After activation, before SQLite commit | New graph/old SQLite version mismatch is rejected |
| After SQLite commit, before cleanup | Matching current remains usable; previous is not a fallback |

`commonplace graph rebuild` restores derived state from committed SQLite without
incrementing its version, including when current is missing or corrupt. It holds
the writer and exclusive publication locks for explicit recovery, removes only
the known derived scratch paths as defined by persistence, builds/verifies a
replacement, and uses the same activation/restore steps without a knowledge
commit. Failure preserves the pre-operation current state, even if that state
was already unavailable. There is no automatic crash repair, generation
selection, general orphan scanner, or general transaction coordinator. The
local spike proves enumerated process-crash windows, not universal power-loss
or filesystem-failure recovery.

## 8. Concurrency

The application supports concurrent read sessions and one process-level writer.
Write commands acquire an explicit writer lock before opening a write
transaction. SQLite uses WAL for ordinary read/write coexistence.

The writer lock:

- covers source publication and knowledge-changing workflows;
- has bounded acquisition time;
- is released by process termination;
- is tested on every supported platform; and
- does not introduce leases, heartbeats, or a job scheduler.

When writers contend, one waits for the bounded lock interval or receives a
clear busy or conflict error and can rerun.

A separate graph publication lock protects graph-directory lifetime. A graph query
holds it in shared mode from opening the graph through closing both its result
and store handle; an iterator may retain a native storage snapshot after the
handle is dropped. Version comparison uses a consistent SQLite read snapshot
under that same lease before execution.
Activation, rollback, cleanup, and `commonplace graph rebuild` hold it exclusively.
Normal knowledge-write candidate construction occurs before acquiring the
exclusive lock. Explicit rebuild holds it throughout recovery. The native
RocksDB writer lock is additional protection, not a substitute for the
application writer/publication locks. Never rename or delete the publication
lock as part of directory replacement.

Embedding and reranking never run while a SQLite write transaction is held.
Ingestion computes passages and embeddings first, then opens a short transaction
and optimistically rechecks the current source revision before publication.

The filesystem and local operating-system account are the security boundary.
The application does not defend against another program directly editing its
SQLite or graph files.

## 9. CLI adapter

The CLI owns argument parsing, source-adapter selection, JSON decoding, invoking
exactly one workflow, output serialization, and mapping typed failures to exit
codes. It contains no storage or domain rules.

Complex JSON commands expose descriptions generated from their execution
validators:

```text
describe_ingest
describe_schema_apply
describe_record
```

The first two return their input JSON Schema and a minimal example.
`describe_record` also reads the current vocabulary so it can return legal entity
types, predicates, endpoint rules, literal kinds, and schema version. The CLI
serializes these descriptions rather than maintaining duplicate schemas.

`commonplace get ID` dispatches a tagged document, revision, passage, entity, or
knowledge-item ID to the corresponding indexed SQLite read.

## 10. Errors

Errors are typed by responsibility:

- `invalid_input`;
- `not_found`;
- `ambiguous`;
- `conflict`;
- `model_unavailable`;
- `graph_unavailable`;
- `limit_exceeded`; and
- `internal_error`.

Infrastructure translates SQLite, model, and Oxigraph failures once at its
boundary. The CLI maps typed errors to stable JSON codes, actionable messages,
and exit codes. Invalid command options and manifest envelopes fail before item
processing. JSON Lines parse errors, duplicate streamed source keys, and
document failures are per-item errors and may produce `partial`; schema,
knowledge, and withdrawal requests return one success or failure for the
complete request. Workflows do not catch broad errors or return success-shaped
fallbacks.

## 11. Configuration

Configuration is intentionally small:

- knowledge-base path;
- source, batch, and result limits;
- maximum passages per document;
- passage target size;
- pinned model and vector representation identity;
- search candidate and rerank limits;
- fusion constants;
- graph row and duration budgets; and
- writer-lock timeout.

Values that define stored representation are part of the store format and cannot
change while opening an existing store.

## 12. Testing strategy

### Domain tests

Cover passage boundaries, UTF-8 offsets, canonical JSON, digests, fusion,
deduplication, ordering, schema rules, literal validation, and entity resolution.

### SQLite integration tests

Use real SQLite, FTS5, and the selected vector extension. Verify constraints,
transactions, cascading deletion, immutable revisions, search correlation,
history, and deterministic graph snapshot reads.

Do not mock SQLite repositories.

### Provider contract tests

Verify lazy loading, bounded batches, dimensions, normalization, stable
representation identity, reranking order, and explicit runtime failure.
Use small deterministic providers for pipeline tests and a small separate suite
against the pinned real models.

### Oxigraph tests

Verify complete projection, version metadata, citation traversal, native
read-only enforcement, lazy row limits, scoped native cancellation,
canonical SQLite-ID parity, RDF-term results, and cleanup of failed candidates.
Cover duplicate fact identity, exact old-revision citations, invalid metadata,
directory publication/restore, missing-current crash rejection, and explicit
rebuild. These are production contract checks, not a new pre-implementation spike. CLI
integration tests verify that process termination stops an executing query.

### Product tests

Exercise the public CLI from an empty knowledge base through ingestion, search,
schema application, authoring, graph query, withdrawal, removal, and reopen.
