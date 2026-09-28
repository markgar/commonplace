# Commonplace: Software Architecture Proposal

Status: Working revision after SQLite schema review  
Revision: 0.46  
Previously approved: revision 0.44 on 2026-09-27  
Review: Revision 0.45 requires joint rereview with the SQLite persistence design.  
Companion: `design.md`

Rust is the selected implementation language, contingent on the packaging spike
defined below. Python-like declarations in this document remain concise
pseudocode for responsibilities and dependencies; the implementation uses Rust
structs, enums, traits, modules, and functions where those constructs carry
useful ownership or type information.

The CLI JSON interface is the only stable public programmatic contract. All
components described here are internal architecture and may evolve without
compatibility guarantees.

## 1. Architecture goal

Build the v2 product with the fewest durable abstractions that allow one part to
change without forcing unrelated rewrites.

The architecture must make these changes local:

- add a new document input adapter without changing ingestion;
- change passage generation without changing source storage;
- replace an embedding model without changing search orchestration;
- change fusion or reranking without changing indexing;
- evolve the user-defined knowledge schema without database DDL generation;
- change the SQLite table or vector-extension layout without changing CLI
  commands;
- change the Ladybug projection shape without changing canonical authoring; and
- add CLI conveniences without putting business rules in the CLI.

This is a greenfield architecture. It must not copy earlier service boundaries,
request models, compatibility layers, package structure, or class hierarchy merely
because they already exist.

## 2. Primary rule: organize around changes

Code that changes for the same reason belongs together.

The system has four meaningful areas:

1. **Sources and evidence**: document identity, revisions, passages, and citations.
2. **Retrieval**: embeddings, lexical search, fusion, reranking, and result hydration.
3. **Knowledge**: schema, entities, types, facts, evidence bindings, and withdrawals.
4. **Graph**: complete projection and read-only Cypher execution.

The CLI is not a fifth domain. It is an adapter over application workflows.
SQLite is not the domain. It is the persistence mechanism for the domain.

## 3. Dependency direction

```text
CLI / source adapters
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
                         embedding / reranker / Ladybug
```

Dependencies point inward:

- Domain code imports no CLI, SQLite, model-runtime, or Ladybug package.
- Application workflows import domain types and small ports.
- Infrastructure implements those ports.
- CLI code imports application workflows and serialization models.
- Infrastructure never calls the CLI.

There must be one composition root that constructs the concrete application.
No module should create hidden database connections, model providers, or Ladybug
instances.

## 4. Keep the class count low

Do not create:

- one repository class per table;
- one service class per command;
- manager, coordinator, handler, processor, and facade classes for the same flow;
- abstract base classes with only one implementation;
- data classes that merely duplicate another layer's data;
- factories for objects that can be constructed directly;
- dependency injection containers;
- event buses;
- generic command dispatch frameworks; or
- interfaces around deterministic functions.

Use classes or stateful components only when an object owns one of:

- mutable lifecycle;
- an external resource;
- a transaction;
- a replaceable runtime dependency; or
- a cohesive application workflow with several collaborators.

Use frozen data classes or similarly simple value types for data. Use ordinary
functions for deterministic transformations.

Because the CLI is the only public interface, application workflows do not need
independent class identities merely to expose methods. Prefer workflow functions
or modules receiving one `AppContext`. Promote a workflow to a stateful class only
when it acquires a real lifecycle of its own.

### 4.1 Code-weight test

Before adding a component, abstraction, state, or error branch, answer:

1. Which required ingest or query behavior does it enable?
2. Which realistic personal-use failure does it prevent?
3. Which expected change becomes local because this boundary exists?
4. Could the same outcome be achieved by rerunning a synchronous command?
5. Will removing it make an agent perform additional calls or lose evidence?

If the first three answers are "none," rerunning is acceptable, and no agent
capability is lost, do not add the code.

Error handling should be proportional:

- validate external input;
- protect SQLite writes with transactions;
- reject incomplete search or graph builds;
- surface model, SQLite, and Ladybug failures clearly;
- clean up temporary graph output; and
- leave the knowledge base reopenable.

Do not model every theoretical interleaving, retry history, resource-accounting
state, or diagnostic observation. A clear failure plus a safe rerun is the normal
recovery mechanism.

### 4.2 Decision seams

The architecture should localize decisions that are reasonably likely to change,
but it should not implement unused alternatives.

Use the lightest seam that fits:

- a pure function for passage policy, fusion, ordering, and validation rules;
- a narrow internal trait for external stateful runtimes such as embedding,
  reranking, and Ladybug;
- one concrete module for SQLite vector-extension SQL;
- one projector boundary for the Ladybug graph shape;
- source adapters that yield the same `DocumentInput`; and
- CLI parsing and serialization outside application workflows.

A seam is successful when replacing one selected implementation changes one
module and focused tests, not when users can choose among implementations at
runtime. Do not add registries, dynamic plugins, generic factories, configuration
switches, parallel storage paths, or compatibility adapters until a second real
implementation is approved.

Not every decision needs a seam. Canonical SQLite tables, CLI JSON contracts, and
domain meanings should be direct and explicit. During early v2 development,
breaking changes use a fresh store and updated callers rather than migration or
compatibility scaffolding.

Apply that rule concretely:

| Decision | Rust seam | Initial implementation |
| --- | --- | --- |
| Embedding model/runtime | Narrow internal `EmbeddingModel` trait owning one loaded session | One pinned local model |
| Reranker | Narrow internal `Reranker` trait | One pinned local reranker |
| Ladybug execution | Narrow internal `GraphEngine` trait | One Ladybug adapter |
| Passage generation | Pure function plus explicit policy value | One paragraph-aware algorithm |
| Fusion and ranking | Pure functions plus explicit constants | One fusion formula |
| SQLite vector search | Isolated `storage::search` SQL module | One selected vector extension |
| Graph representation | One `build_graph_snapshot` mapping function | One stable generic graph shape |
| Input formats | Small adapters yielding `DocumentInput` | File, directory, stdin, JSONL, manifest |
| CLI | Parsing and JSON serialization around workflow functions | One public CLI contract |

Rust implementation guidance:

- use traits only for external stateful engines that require test doubles or have
  a credible replacement boundary;
- keep those traits internal and narrow;
- avoid generic type parameters spreading through domain and workflow signatures;
- prefer a trait object at the actual runtime boundary when static generics would
  infect the application;
- use ordinary modules and functions for deterministic policy;
- keep `SqliteDatabase`, `AppContext`, and the selected storage model concrete;
- do not add a factory when the composition root can construct the selected
  implementation directly; and
- do not add a second implementation merely to demonstrate that a seam works.

Review each proposed seam with one question:

> If this decision changes, can one module and its focused tests change without
> rewriting the workflow or CLI?

If yes, the seam is sufficient. If satisfying the question requires registries,
dynamic loading, parallel implementations, duplicated configuration, or generic
framework code, the proposed seam is too expensive for v2.

## 5. Proposed package structure

```text
src/
    composition-root
    errors

    domain/
        documents
        evidence
        schema
        knowledge
        search

    application/
        ingest
        search
        knowledge
        graph

    storage/
        sqlite
        documents
        knowledge
        search
        schema.sql

    engines/
        embeddings
        reranking
        ladybug

    adapters/
        sources
        json
        cli

    executable-entrypoint
```

This is a guide, not a demand for empty modules. Start with fewer files and split
only when a file owns more than one reason to change.

## 6. Domain layer

The domain layer contains values, invariants, and deterministic algorithms. It has
no I/O.

### 6.1 Values

Use immutable values for identifiers and inputs:

```python
@dataclass(frozen=True)
class DocumentInput:
    source_key: str
    text: str
    title: str | None
    source_type: str
    occurred_at: datetime | None
    metadata: Mapping[str, JsonValue]


@dataclass(frozen=True)
class Passage:
    passage_id: str
    document_id: str
    revision_id: str
    ordinal: int
    start: int
    end: int
    text: str


@dataclass(frozen=True)
class EvidenceRef:
    passage_id: str


@dataclass(frozen=True)
class EntityRef:
    entity_id: str
```

Use plain strings internally only when they do not represent different concepts.
Small ID wrappers are worthwhile when they prevent mixing document, revision,
passage, entity, and knowledge-item IDs.

Do not version every internal value. Version only persisted public input/output
formats when there is a demonstrated need.

### 6.2 Document rules

`domain.documents` owns deterministic decisions such as:

- validating a source key;
- hashing exact input;
- determining `added`, `updated`, or `unchanged`;
- constructing revision identity;
- normalizing metadata serialization; and
- validating source-size limits.

It does not open files or write SQL.

The normal source model is immutable event capture. A later meeting, message, or
summary is a new source. Revision creation exists for exceptional corrections and
genuinely editable source kinds, not for changing the knowledge inferred from a
source.

User-visible source metadata is revision content. A change to title, source type,
occurred-at timestamp, or custom metadata creates a revision even when text is
unchanged. Only internal operational timestamps may update in place.

### 6.3 Passage generation

Passage generation is a deterministic function:

```python
def build_passages(
    revision: DocumentRevision,
    policy: PassagePolicy,
) -> list[PassageDraft]:
    ...
```

Do not introduce a passage-builder interface until there are two real strategies
that must be selected at runtime. A policy value and a pure function are enough
for the first version.

The initial function accumulates whole paragraphs to a target size and splits an
oversized paragraph into fixed nonoverlapping windows. It accepts no
adapter-supplied boundaries.

### 6.4 Schema rules

`domain.schema` owns:

- schema input values;
- term-name validation;
- additive-change validation;
- predicate endpoint validation;
- literal kind validation.

The stored schema is additive. Each term records the monotonic version in which
it was introduced; the database does not retain complete repeated schema
snapshots. Existing terms cannot be redefined or removed.

It does not know how schema rows are stored.

### 6.5 Knowledge rules

`domain.knowledge` owns:

- entity declarations;
- multi-type membership;
- add-only aliases and identifiers;
- entity and literal assertions;
- optional passage evidence for memberships and assertions;
- add/withdraw lifecycle;
- predicate endpoint checks; and
- ambiguity and duplicate rules.

Authored entities and facts carry database-generated `created_at` and optional
informational `created_by`. These values never participate in permissions,
identity, deduplication, or lifecycle decisions.

Knowledge state changes explicitly. New proposals, changed outcomes, and later
decisions are new facts. Facts leave the current graph only through explicit
withdrawal; source revision or deactivation does not implicitly rewrite knowledge.

First-class mention records are not part of the initial domain. Textual mentions
are found through hybrid search; explicit graph facts carry their own optional
passage support.

Facts should share one core representation where possible:

```python
@dataclass(frozen=True)
class FactDraft:
    local_id: str
    subject: EntitySelector
    predicate: str
    object: EntitySelector | LiteralValue
    evidence: tuple[EvidenceRef, ...] = ()
```

Do not introduce a separate seed-support type. The same fact representation
supports both cited and uncited knowledge. Query outputs always expose the support
collection, including when it is empty.

Type membership uses a dedicated canonical table because its object is a schema
type rather than an entity or literal. It shares the same authored metadata,
evidence association, and withdrawal behavior as facts; it is not a separate
service or lifecycle subsystem.

Both type memberships and facts receive a `KnowledgeItemId`. One shared
knowledge-item row owns `created_at`, optional `created_by`, and withdrawal
state; one shared evidence table associates that ID with zero or more passages.
Concrete membership and fact tables retain direct typed columns and reference
the shared ID.

Aliases and identifiers are add-only entity metadata in v2. They use natural
entity/value keys and do not receive public row IDs, `KnowledgeItemId`, passage
evidence, withdrawal, or graph item projection.

Decisions, actions, blockers, outcomes, and statuses do not have implementation
types or workflow classes. They are schema-defined predicates represented by the
same entity or literal fact structures.

### 6.6 Retrieval rules

Deterministic retrieval operations should be functions:

```python
def reciprocal_rank_fusion(
    lexical: Sequence[RankedCandidate],
    dense: Sequence[RankedCandidate],
    config: FusionConfig,
) -> list[FusedCandidate]:
    ...
```

Candidate deduplication, tie-breaking, truncation, and score validation belong
here. Provider invocation and SQL belong elsewhere.

## 7. Infrastructure ports

Define a port only at a boundary that is expected to vary independently.

Recommended ports:

```python
class EmbeddingModel(Protocol):
    @property
    def identity(self) -> ModelIdentity: ...

    def embed_documents(self, texts: Sequence[str]) -> Sequence[Vector]: ...

    def embed_query(self, text: str) -> Vector: ...


class Reranker(Protocol):
    @property
    def identity(self) -> ModelIdentity: ...

    def rerank(
        self,
        query: str,
        passages: Sequence[PassageText],
    ) -> Sequence[float]: ...


class GraphEngine(Protocol):
    def rebuild(
        self,
        snapshot: GraphSnapshot,
        destination: Path,
        knowledge_version: int,
    ) -> None: ...

    def query(
        self,
        database: Path,
        cypher: str,
        *,
        row_limit: int,
        timeout: timedelta,
    ) -> GraphResult: ...
```

The concrete providers are lightweight handles at construction. Each uses one
private lazy-initialization cell to load model weights on its first inference
call, then reuses that session for the remainder of the process. The traits do
not expose loading, factories, profiles, or lifecycle control.

These protocols isolate actual replaceable runtimes.

Graph file lifecycle is owned by one concrete `GraphRuntime` boundary:

```python
class GraphRuntime:
    def build_candidate(
        self,
        snapshot: GraphSnapshot,
        knowledge_version: int,
    ) -> GraphCandidate: ...

    def activate_candidate(self, candidate: GraphCandidate) -> None: ...

    def open_current(self, expected_knowledge_version: int) -> GraphHandle: ...

    def discard_candidate(self, candidate: GraphCandidate) -> None: ...

    def query_current(
        self,
        expected_knowledge_version: int,
        cypher: str,
        *,
        row_limit: int,
        timeout: timedelta,
    ) -> GraphResult: ...
```

The initial implementation uses one current graph path plus one temporary
candidate. `open_current` verifies that the graph's version equals SQLite's
current `knowledge_version` and fails explicitly on mismatch. A later
implementation may use immutable graph generations behind this same boundary,
but v2 does not implement generation selection or cleanup now.

Do not define protocols for every repository or deterministic helper. SQLite is a
chosen product dependency, not an implementation detail that must be abstracted
away behind dozens of interfaces.

## 8. SQLite ownership

### 8.1 One database owner

Use one concrete `SqliteDatabase` class:

```python
class SqliteDatabase:
    def __init__(self, path: Path) -> None: ...

    def initialize(self) -> None: ...

    @contextmanager
    def read(self) -> Iterator[SqliteReadSession]: ...

    @contextmanager
    def write(self) -> Iterator[SqliteWriteSession]: ...
```

It owns:

- connection creation;
- pragmas;
- schema initialization and exact store-format checks, including the pinned
  passage, embedding, vector, and lexical representation;
- transaction boundaries;
- row factories;
- common SQL error translation; and
- connection shutdown.

No other class creates a SQLite connection.

There is no per-document derived-search fingerprint or in-place reindex workflow.
An incompatible search-producing change increments the exact store format;
opening the prior store fails explicitly and recovery is a fresh store plus
reingestion.

### 8.2 Transaction sessions

`SqliteReadSession` and `SqliteWriteSession` own repositories bound to one
connection and transaction:

```python
class SqliteReadSession:
    documents: DocumentQueries
    evidence: EvidenceQueries
    knowledge: KnowledgeQueries
    search: SearchQueries


class SqliteWriteSession(SqliteReadSession):
    documents: DocumentStore
    knowledge: KnowledgeStore
    search: SearchIndexStore
```

These are transaction-scoped collaborators, not globally cached service objects.
They make transaction ownership obvious and prevent one workflow from
accidentally mixing connections.

### 8.3 Repository granularity

Group SQL by domain capability, not by table:

- `DocumentStore`: source identity, revisions, current document, passages.
- `EvidenceQueries`: exact passage and historical source resolution.
- `SearchIndexStore` / `SearchQueries`: lexical rows, vector-extension writes and
  candidate queries, search reads.
- `KnowledgeStore` / `KnowledgeQueries`: schema, entities, facts, evidence,
  withdrawals.

Four cohesive SQL components are preferable to twenty table repositories.

Repository methods should return domain values or compact persistence records.
They should not return raw SQLite rows outside `storage/`.

### 8.4 SQL is allowed to be SQL

Keep complex set operations in explicit SQL when SQL is the clearest expression.
Do not hide every query behind generic filter builders or ORM models.

Use:

- checked-in schema SQL;
- parameterized statements;
- explicit indexes;
- small row-decoding functions; and
- integration tests against real SQLite.

Avoid an ORM unless actual use demonstrates that it removes more code than it
adds. The expected schema and query patterns favor direct SQLite.

## 9. Application workflows

Application functions orchestrate complete user intents. They contain no CLI
parsing and very little domain logic. Python-like signatures below are
illustrative; Rust should use module functions and small structs only where state
or owned resources require them.

### 9.1 `ingest_documents`

```python
def ingest_documents(
    app: AppContext,
    documents: Iterable[DocumentInput],
) -> IngestBatchResult:
    ...
```

Responsibilities:

1. validate the batch envelope and duplicate source keys;
2. initialize one embedding-model session for the command;
3. process each document through the same change-detection and passage logic;
4. send passages from changed documents through that session in bounded inference
   batches, optionally spanning document boundaries;
5. map vectors back to their document and passage IDs;
6. atomically publish each document revision, passages, lexical rows, and vectors;
7. return one outcome per document; and
8. return an aggregate result.

Each document publishes in its own SQLite transaction. `IngestBatchResult`
reports `complete`, `partial`, or `failed`: partial success is expected batch
behavior, not a recovery subsystem. Rerunning the same source keys converges
through source identity and content comparison.

`EmbeddingModel` is a stateful resource owned once by `AppContext`, not a factory
called for every document. One batch command may make several bounded
`embed_documents` calls, but all calls use the same loaded model session. This
avoids repeated model startup without requiring the complete source collection
or all generated passages to reside in memory simultaneously.

There is no separate `AddDocument`, `UpdateDocument`, `IndexDocument`, or
`PrepareSearch` application service. Those are stages of one intent.

There is also no public index status, pending, cleanup, attempt-history, or
rebuild workflow. Reprocessing uses `ingest_documents`.

Implementation detail:

```text
read current source
    -> compute revision/passages
    -> embed outside write transaction
    -> open write transaction
    -> commit complete searchable document
```

The mutating command holds the application's single writer lock for this
workflow. The system does not defend against another program editing the SQLite
file behind the application.

### 9.2 `remove_document`

```python
def remove_document(
    app: AppContext,
    source_key: str,
) -> RemoveDocumentResult:
    ...
```

This workflow opens one SQLite transaction, deletes one document and all of its
revisions, passages, lexical rows, vectors, and evidence links, builds the
resulting candidate graph, then activates and commits through the normal
`knowledge_version` finalization window. It does not withdraw authored knowledge;
items whose last citation is removed become uncited. The result reports detached
evidence counts and affected `KnowledgeItemId` values. Reingesting the source key
later creates a new document identity.

Do not add batch removal, restore, tombstones, cascading withdrawal, or
directory-absence synchronization.

### 9.3 `search_documents`

```python
def search_documents(
    app: AppContext,
    query: str,
    *,
    since: datetime | None = None,
    source_types: tuple[str, ...] = (),
    limit: int,
) -> SearchResult:
    ...
```

Responsibilities:

1. validate the query;
2. embed the query;
3. apply the same time and source-type filters to lexical and vector candidates
   in one SQLite read session;
4. fuse and deduplicate with domain functions;
5. rerank the bounded candidate set;
6. hydrate exact evidence in the same read session; and
7. return one complete cited result.

The CLI should not call lexical, vector, reranking, and evidence services
separately.

### 9.4 `apply_knowledge_schema`

```python
def apply_knowledge_schema(
    app: AppContext,
    definition: SchemaDefinition,
    *,
    check_only: bool = False,
) -> SchemaApplyResult:
    ...
```

It validates and applies one additive schema change in one SQLite transaction.
There are no proposal, approval, or administrator services. Schema application
does not rebuild Ladybug: SQLite owns the complete vocabulary, while the graph
contains only terms required by active knowledge.

### 9.5 `record_knowledge`

```python
def record_knowledge(
    app: AppContext,
    document: KnowledgeDocument,
) -> KnowledgeWriteResult:
    ...
```

Responsibilities:

1. validate the complete input document;
2. resolve IDs, exact identifiers, and unique names inside SQLite;
3. validate types, predicates, endpoints, and evidence;
4. stage the complete knowledge change in one SQLite transaction;
5. read the complete candidate projection through that same transaction;
6. assign the same candidate `knowledge_version` to SQLite and the graph;
7. build a replacement Ladybug graph separately while retaining the old graph;
8. activate the graph and commit SQLite in one short finalization window; and
9. return one success or one explicit failure.

Do not create separate application services for entity creation, type assignment,
alias creation, relationship creation, and decision creation. They are variants
within one explicit authoring operation.

Knowledge authoring is append-only and may create duplicate facts. It is not
idempotent after success. A failed command, however, must not retain its newly
authored knowledge items. The previous complete graph remains available, so
the original input can be rerun without duplicating knowledge from the failed
attempt. A crash during final activation may leave a version mismatch; graph
queries detect and refuse that state. Do not add a public graph-rebuild command,
retry ledger, or generalized transaction coordinator for this case.

If the caller cannot determine whether a successful knowledge response was
received, it inspects canonical item IDs before retrying. `record_knowledge`
remains one command boundary so persistent caller request IDs can later wrap the
workflow without changing domain items, graph projection, or CLI operations.

### 9.6 `withdraw_knowledge`

```python
def withdraw_knowledge(
    app: AppContext,
    item_ids: Sequence[KnowledgeItemId],
) -> WithdrawalResult:
    ...
```

Facts and entity type memberships use this one workflow. The complete submitted
ID set validates, withdraws in one SQLite transaction, and produces one candidate
graph; any invalid ID or build failure rolls back the whole request. It does not
dispatch to specialized withdrawal services. Alias and identifier IDs are
not accepted because withdrawal takes only `KnowledgeItemId` values.

Validation includes the complete post-withdrawal active state. Reject the request
if any fact that remains active would have no permitted active type on a required
endpoint, unless that dependent fact is withdrawn in the same request.

### 9.7 `query_graph`

```python
def query_graph(app: AppContext, cypher: str) -> GraphResult:
    ...
```

Responsibilities:

- execute through a native read-only Ladybug transaction or session;
- apply native interruption, cancellation, or a hard execution timeout;
- stop after `row_limit + 1` rows to detect truncation;
- serialize supported graph values deterministically; and
- report truncation and execution errors.

It does not parse natural language, translate a custom query plan, inspect
Cypher strings for safety, or implement a watchdog. The packaging spike must
prove native write rejection and actual cancellation. If the Ladybug integration
cannot provide both, the graph-query architecture or runtime choice is reopened
before implementation; the required graph capability does not ship behind an
unsafe approximation.

### 9.8 `get_record`

Exact follow-up inspection uses one internal workflow rather than one service per
noun:

```python
def get_record(app: AppContext, target: RecordId) -> RecordView:
    ...
```

`RecordId` is a tagged document, revision, passage, entity, or knowledge-item ID.
The workflow dispatches to the appropriate SQLite query component and returns one
stable tagged result. A knowledge-item result identifies whether it is a type
membership or fact and includes evidence and withdrawal state. It does not
implement generic history, centered context, or revision comparison.

## 10. Graph projection

### 10.1 Snapshot construction and runtime ownership

`build_graph_snapshot(session)` is a deterministic SQLite mapping function. It
reads current active knowledge through the caller's transaction and returns one
complete `GraphSnapshot`.

`GraphRuntime` is the only stateful graph owner. It receives the snapshot, builds
and verifies a candidate Ladybug database, activates the candidate only after
complete success, opens the current version for queries, and discards failed
temporary output.

This is intentionally a complete rebuild first. Do not introduce incremental
projection interfaces until actual graph size makes rebuilds unacceptable.

Every successful knowledge authoring or withdrawal command invokes this complete
rebuild. Historical and withdrawn facts remain in SQLite and are not represented
in Ladybug.

### 10.2 `GraphSnapshot`

`GraphSnapshot` is a simple iterable data transfer value:

```python
@dataclass(frozen=True)
class GraphSnapshot:
    schema_version: int
    entities: Sequence[GraphEntity]
    types: Sequence[GraphType]
    memberships: Sequence[GraphMembership]
    facts: Sequence[GraphFact]
    knowledge_items: Sequence[GraphKnowledgeItem]
    passages: Sequence[GraphPassage]
    documents: Sequence[GraphDocument]
```

`GraphDocument` carries stable `document_id` and `source_key`.
`GraphPassage` carries its passage/revision IDs, exact quote, offsets, ordinal,
title, source type, source timestamp, and other revision-specific display
metadata. This prevents evidence from different revisions of one document from
sharing incorrect current metadata and avoids per-row SQLite hydration.

All passage ranges are zero-based, half-open UTF-8 byte offsets into the exact
stored revision text. Passage construction and evidence validation assert valid
UTF-8 boundaries and exact equality between the byte slice and stored quote.

`types` and predicate identifiers are limited to those referenced by active
memberships and facts in the same snapshot. Unused vocabulary remains solely in
SQLite and is returned by `kg schema show`.

`passages` contains only passages referenced by active `GraphKnowledgeItem`
support, and `documents` contains only their source documents. Uncited corpus
passages remain in SQLite hybrid search. Document ingestion does not rebuild
Ladybug; explicit document removal does because it deletes support edges and
projected evidence.

For larger data later, replace sequences with streaming iterators without changing
the domain or CLI contracts.

The snapshot is a projection input, not another authored model. It may flatten
SQLite's normalized rows into the shape Ladybug needs.

Membership and fact edges carry `knowledge_item_id`. `GraphKnowledgeItem` nodes
use the same IDs and own `SUPPORTED_BY` edges to passages. Only entities
participating in an active membership or fact are included; bare SQLite entities
are not graph-active.

### 10.3 Stable projected shape

The graph engine owns Ladybug DDL and projection details. No knowledge application
workflow issues Ladybug statements directly.

Changing node labels, edge tables, property names, or bulk-loading strategy should
normally affect only:

- `GraphSnapshot` construction;
- `LadybugGraphEngine.rebuild`; and
- the documented output of `kg graph schema`.

It should not change SQLite authoring rules.

## 11. Input adapters

### 11.1 `DocumentSource`

All input forms implement one iterator protocol:

```python
class DocumentSource(Protocol):
    def __iter__(self) -> Iterator[DocumentInput]: ...
```

Concrete adapters:

```text
FileSource
DirectorySource
StdinDocumentSource
JsonLinesSource
ManifestSource
```

`DirectorySource` derives the default source key from the normalized path relative
to its scan root. `ManifestSource` and `JsonLinesSource` may supply a stable key
independent of path. Adapters do not detect renames or infer deletion from missing
files.

These classes only:

- read their input medium;
- derive or require stable source keys;
- decode text;
- map metadata; and
- yield `DocumentInput`.

They do not connect to SQLite or invoke models.

If an external Teams, email, web, or PDF connector is later written in another
process or language, it can emit the same JSON Lines format without linking to KG
internals.

### 11.2 Batch streaming

`ingest_documents()` accepts an iterable, not a list, so a connector can stream
large input without placing every source text in memory.

Batch-level duplicate source-key validation may retain only keys and small
metadata. If complete prevalidation would require buffering every full document,
prefer bounded per-item processing plus explicit duplicate failure over a complex
spill-to-disk framework.

## 12. CLI adapter

The CLI performs:

- argument parsing;
- adapter selection;
- JSON input decoding;
- calling exactly one application workflow;
- rendering text or JSON output; and
- mapping known errors to exit codes.

Complex JSON input commands expose descriptions generated from the same boundary
validators used during execution. The CLI adapter serializes those descriptions;
it does not maintain a parallel schema model. `kg record --describe` composes the
static input contract with the current user vocabulary from SQLite.

The application workflows expose these descriptions directly:

- `describe_ingest(app)`;
- `describe_schema_apply(app)`; and
- `describe_record(app)`.

The first two return their input schema and a minimal example. The record
description additionally reads the current vocabulary from SQLite.

The CLI must not:

- open SQLite directly;
- execute SQL;
- call embedding or Ladybug APIs;
- implement entity resolution;
- split passages;
- fuse search rankings;
- decide retries;
- rebuild graphs itself; or
- contain alternate one-item business logic.

Example:

```python
def ingest_command(args: IngestArgs, app: AppContext) -> int:
    source = build_document_source(args)
    result = ingest_documents(app, source)
    return render(result, json=args.json)
```

## 13. Composition root

The composition root constructs the application once. In Python-like pseudocode:

```python
@dataclass(frozen=True)
class AppContext:
    database: SqliteDatabase
    embeddings: EmbeddingModel
    reranker: Reranker
    graph_runtime: GraphRuntime
    config: AppConfig


def build_application(config: AppConfig) -> AppContext:
    database = SqliteDatabase(config.sqlite_path)
    embeddings = LocalEmbeddingModel(config.embedding)
    reranker = LocalReranker(config.reranker)
    graph_engine = LadybugGraphEngine()
    graph_runtime = GraphRuntime(
        graph_engine,
        config.graph_path,
    )

    return AppContext(
        database=database,
        embeddings=embeddings,
        reranker=reranker,
        graph_runtime=graph_runtime,
        config=config,
    )
```

`build_application` creates each lightweight model-provider handle once per CLI
process. Model weights are not loaded here. `ingest_documents` triggers at most
one lazy embedding-model load and reuses that session for the entire document
batch; search loads only the model or models it actually invokes. Adapters and
per-document loops never construct model runtimes.

This explicit construction is the dependency injection mechanism. Do not add a
container framework.

## 14. Errors

Use one small public error hierarchy:

```python
class KgError(Exception):
    code: ErrorCode
    message: str


class InvalidInput(KgError): ...
class NotFound(KgError): ...
class Ambiguous(KgError): ...
class Conflict(KgError): ...
class ModelUnavailable(KgError): ...
class GraphUnavailable(KgError): ...
class LimitExceeded(KgError): ...
class InternalError(KgError): ...
```

Infrastructure translates SQLite, model, and Ladybug failures once at its
boundary. Application workflows do not repeatedly catch and rewrap the same
error.

Batch results contain per-item errors. Whole-command structural errors raise
before item processing. Document ingestion may therefore return `partial`.
Schema, knowledge, and withdrawal workflows return one success or one failure
for the complete request and never expose per-item partial mutation.

Do not create a separate error model for every package.

## 15. Configuration

Use one application configuration:

```python
@dataclass(frozen=True)
class AppConfig:
    sqlite_path: Path
    graph_path: Path
    embedding: EmbeddingConfig
    reranker: RerankerConfig
    passages: PassagePolicy
    search: SearchConfig
    graph_limits: GraphQueryLimits
```

The first version should have one supported configuration for each model and
algorithm. Configuration values primarily make behavior explicit and testable;
they do not imply a plugin framework or multiple supported profiles.

The concrete embedding and reranking implementations use pinned local models and
the selected runtime's standard model cache. No component models approval,
profile attachment, cache-profile management, or remote inference.

## 16. Transaction boundaries

Transaction ownership belongs to application workflows.

Recommended boundaries:

- one SQLite transaction per document publication;
- one SQLite transaction per submitted knowledge command, held through candidate
  graph construction so a failed build can roll back the attempted change;
- one SQLite transaction for schema apply;
- one SQLite transaction and one candidate graph rebuild for the complete
  submitted withdrawal batch;
- one SQLite read transaction for one hybrid search;
- one SQLite read transaction for one graph snapshot.

No repository method starts or commits its own hidden transaction.

Embedding and reranking do not run while a write transaction is held.

For a knowledge mutation, `build_graph_snapshot` reads projection rows through
the open SQLite transaction and `GraphRuntime` builds a candidate graph without
replacing the current graph. A build failure rolls back SQLite and deletes the
candidate.

Final graph activation and SQLite commit cannot be one physical transaction. The
command retains the previous graph, activates the candidate, commits SQLite, and
then discards the previous graph. If commit returns an ordinary error, it restores
the previous graph and reports failure. The window uses the same monotonic
`knowledge_version`; a process crash can still interrupt it. Graph queries compare
the selected graph version with SQLite and refuse mismatches. The initial product
does not attempt automatic crash recovery; a fresh store and reingestion are the
supported recovery.

## 17. Concurrency model

The first version assumes one local writer at a time.

Use:

- SQLite WAL for ordinary read/write coexistence;
- a simple process-level write lock for mutating CLI commands;
- short SQLite write transactions;
- optimistic recheck after model work; and
- one graph-build lock.

Do not build:

- worker fences;
- distributed locks;
- leases;
- heartbeat protocols;
- transaction observers;
- cross-process retained snapshots; or
- generalized concurrency-control abstractions.

If two writers contend, one waits briefly or receives a clear busy/conflict error
and reruns.

## 18. Testing architecture

Test at the layer that owns the behavior.

### Domain tests

Fast pure tests for:

- source identity and revision decisions;
- passage boundaries;
- schema evolution;
- multi-type endpoint validation;
- fact add/withdraw and duplicate-preservation rules;
- fusion, deduplication, and tie-breaking.

### SQLite integration tests

Use real temporary SQLite databases for:

- schema initialization;
- document revision and citation preservation;
- atomic searchable publication;
- exact evidence resolution;
- knowledge authoring and ambiguity;
- fail-closed knowledge-item reads for missing, duplicate, or kind-mismatched
  detail rows;
- withdrawal visibility and history; and
- complete graph snapshot selection.

Do not mock SQLite repositories.

### Provider contract tests

Use tiny deterministic embedding and reranker implementations to verify:

- dimensions and normalization;
- pipeline ordering;
- candidate fusion;
- error propagation; and
- complete result hydration.

A multi-document ingestion test must additionally prove:

- exactly one embedding-model construction for the command;
- at least one bounded embedding call may contain passages from multiple
  documents;
- every vector is correlated to the correct document and passage; and
- one document's publication failure does not discard another document's
  successful publication.

Run a small number of explicit real-model acceptance tests separately.

### Ladybug tests

Use the real supported Ladybug runtime for:

- complete rebuild;
- canonical ID parity;
- multi-type entities;
- entity and literal facts;
- evidence edges;
- native rejection of mutation attempts;
- actual cancellation of an over-budget query;
- row-limit truncation;
- failed-build cleanup; and
- reopen/query behavior.

### Product acceptance tests

Promote the defining cross-component requirements into executable acceptance
tests:

- one lazy model load and bounded cross-document embedding;
- independent per-document publication and aggregate partial status;
- complete source deletion, citation detachment, graph rebuild, and new identity
  on later reingestion;
- exact UTF-8 citation slicing;
- whole-request knowledge rollback on invalid input or graph-build failure;
- whole-request withdrawal rollback;
- add-only alias and identifier behavior;
- projection of only schema terms used by active knowledge;
- distinct item IDs for repeated successful knowledge authoring;
- rejection of an incompatible store/search format;
- graph `knowledge_version` mismatch refusal;
- one-query graph evidence retrieval; and
- clean-target release execution without an installed language runtime.

### CLI tests

Test only command composition and stable public JSON:

- one file;
- directory batch;
- stdin;
- JSON Lines;
- search;
- schema apply;
- record;
- withdraw;
- graph schema; and
- graph query.

Do not duplicate all domain cases through the CLI.

## 19. Change-locality checks

The architecture is succeeding if these changes remain local:

| Change | Expected files |
| --- | --- |
| Add a PDF adapter | `adapters/sources.rs` and adapter tests |
| Change passage size | domain passage policy/function and its tests |
| Replace embedding model | one `EmbeddingModel` implementation and configuration |
| Change fusion weights | `domain/search.rs` and tests |
| Replace reranker | one `Reranker` implementation |
| Add a literal value kind | schema/knowledge domain values, SQLite codec, graph codec |
| Change SQLite indexing strategy | `storage/search.rs` and schema SQL |
| Replace the selected vector extension | `storage/search`, schema SQL, packaging |
| Change Ladybug graph shape | graph snapshot mapping, Ladybug engine, graph-schema output |
| Add a CLI alias or convenience | CLI adapter only |
| Add a new graph query | no code; issue different read-only Cypher |

If a routine change touches the CLI, application workflows, domain, SQLite, and
Ladybug simultaneously, inspect the boundaries before accepting it.

## 20. Proposed component inventory

The initial implementation should aim for approximately this concrete component
inventory. In Rust, a component may be a struct, enum, trait, module, or small set
of functions:

### Resource and provider classes

1. `SqliteDatabase`
2. `SqliteReadSession`
3. `SqliteWriteSession`
4. `LocalEmbeddingModel`
5. `LocalReranker`
6. `LadybugGraphEngine`

### SQL capability classes

7. `DocumentStore`
8. `EvidenceQueries`
9. `SearchIndexStore`
10. `SearchQueries`
11. `KnowledgeStore`
12. `KnowledgeQueries`

### Application state

13. `AppContext`
14. `GraphRuntime`

Ingest, search, schema apply, record, withdraw, get, graph query, and description
operations begin as workflow functions grouped by application module. They are
not separate classes unless later lifecycle or state justifies promotion.

### Input adapter classes

15. `FileSource`
16. `DirectorySource`
17. `StdinDocumentSource`
18. `JsonLinesSource`
19. `ManifestSource`

This is a ceiling to question, not a quota to fill. Some SQL capability classes
may begin as modules of functions bound to a session. Some source adapters may be
factory functions. Do not create an empty class merely to match this list.

Most of the remaining code should be:

- frozen domain values;
- validation functions;
- deterministic algorithms;
- SQL statements and row decoders;
- serializers; and
- tests.

## 21. Implementation gates and bounded decisions

Before implementation proceeds beyond the disposable spike, prove that the
official Ladybug integration, SQLite vector extension, local embedding and
reranking runtimes, and release packaging produce a self-contained Rust
distribution without substantial custom binding code, including native
read-only Cypher enforcement and real query cancellation.

The remaining input-shape and platform details are bounded implementation
decisions, not architecture approval questions:

- each JSON Lines record is self-contained; and
- the process-level writer lock must be tested on every initially supported
  platform.

## 22. Recommended implementation order

Implement vertical slices rather than finishing layers in isolation:

1. Disposable Rust packaging spike proving Ladybug, SQLite vector search, local
   embedding, reranking, native read-only/cancellable Cypher, and execution
   without an installed interpreter, language runtime, source checkout, or build
   tool. Record tested versions, commands, artifact contents, target platform,
   and pass/fail results.
2. `SqliteDatabase` plus one-document/batch source storage.
3. Passage generation, lexical rows, embeddings, and complete `kg ingest`.
4. Full hybrid `kg search` with exact evidence.
5. User schema and multi-type entities.
6. Uniform cited fact authoring and withdrawal.
7. Complete Ladybug rebuild.
8. Read-only Cypher and graph schema.
9. JSON Lines and external connector-facing ingestion.

Each slice must produce a usable CLI behavior. Do not build all domain models,
repositories, or provider abstractions before the first end-to-end ingestion and
search experiment works. The packaging spike is a gate, not a reusable framework:
discard its code unless it is the smallest clean foundation for the first slice.

## 23. Summary

The proposed architecture is deliberately conventional:

- pure domain values and functions;
- a small set of application workflows;
- direct SQLite persistence;
- protocols only for model and graph runtimes;
- one explicit composition root;
- thin CLI and source adapters;
- synchronous execution; and
- no compatibility or orchestration framework.

Its flexibility comes from clear ownership and dependency direction, not from
generalized abstractions. The code should remain easy to replace because each
external engine and each user intent has one obvious boundary.
