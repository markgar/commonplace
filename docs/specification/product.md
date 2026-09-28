# Commonplace product design

## 1. Purpose

Commonplace gives a person and their local agent a durable knowledge workspace
over local documents. It stores source material exactly, retrieves useful
passages efficiently, records explicit interpretations with citations, and
supports expressive graph queries without turning the product into a hosted
knowledge platform.

The product must let an agent:

1. ingest one document or a collection of documents;
2. retrieve relevant source passages through complete hybrid search;
3. author cited entities, types, relationships, and literal facts;
4. query the authored graph with read-only Cypher; and
5. resolve every cited result back to exact stored evidence.

Deterministic processing belongs in the executable. Semantic interpretation
belongs to the calling agent.

## 2. Product principles

### 2.1 Local and personal

A knowledge base is a local directory containing authoritative SQLite state and
a derived Grafeo graph. Anyone who can open the directory and run the CLI has
full access.

There are no tenants, accounts, principals, grants, namespaces, or network
service endpoints.

### 2.2 Exact evidence

Commonplace preserves the exact UTF-8 text used to produce every passage.
Citations contain canonical document, revision, and passage identifiers plus
zero-based, half-open UTF-8 byte offsets.

For a stored revision `text`, every passage must satisfy:

```text
text[start_byte..end_byte] == passage.text
```

Offsets must fall on UTF-8 character boundaries. Newlines and Unicode are not
normalized before offsets are computed.

### 2.3 Explicit knowledge

Search results are evidence, not automatically accepted facts. Entities, type
memberships, relationships, and literal assertions enter the knowledge graph
only through an explicit authoring command.

Authored knowledge remains active until explicitly withdrawn. Revising a source
does not silently invalidate a fact. Removing a source deletes its evidence
links but does not withdraw the knowledge item; an item may therefore remain
active without citations.

### 2.4 Minimal machinery

The product is synchronous and rerunnable. It does not expose jobs, processing
attempts, retained query handles, retry receipts, generalized audit logs, model
profile management, or background repair workflows.

Errors must remain clear and data must remain consistent, but safe reruns are the
normal recovery mechanism.

## 3. Source model

### 3.1 Normalized input

Every source adapter produces a `DocumentInput`:

| Field | Meaning |
| --- | --- |
| `source_key` | Stable identity assigned by the adapter or caller |
| `text` | Exact UTF-8 source text |
| `title` | Optional display title |
| `source_type` | Descriptive source category |
| `occurred_at` | Optional source timestamp |
| `metadata` | Small JSON object |

SQLite maps `source_key` to an internal document ID. Examples include an email
message ID, an editable message ID, a meeting-summary key, or a canonical file
URI.

### 3.2 Adapters

The CLI supports:

```sh
commonplace ingest document.md
commonplace ingest document-a.md document-b.md
commonplace ingest ./notes --recursive
commonplace ingest --stdin --source-key teams/message/123
commonplace ingest --jsonl documents.jsonl
commonplace ingest --manifest corpus.json
```

Direct file and directory inputs accept Markdown and plain UTF-8 text. Their
default source key is a `file:` URI derived from the normalized absolute path,
so direct and directory ingestion of the same file use the same identity and
different scan roots cannot conflate unrelated files. Moving a file creates a
new source unless a manifest or streamed input supplies a stable key.
Directory scans support `.md` and `.txt`, recurse only when requested, skip
symlinks by default, and preserve Markdown as text rather than extracting
domain-specific records. Repeated include and exclude globs may narrow a scan.
Every input path is subject to the configured maximum document size.

Inputs must decode as valid UTF-8. The system preserves valid text exactly,
including empty content, CRLF line endings, byte-order marks, NUL characters,
combining marks, and non-BMP characters. It rejects undecodable or binary input
rather than normalizing it.

Other systems can normalize email, chat, web, PDF, or connector content into
stdin or JSON Lines input. Format-specific acquisition does not create a
separate storage or indexing pipeline. Each JSON Lines record is self-contained.
JSON Lines and manifest records may provide stable source keys and explicit
metadata.

### 3.3 Revisions

Submitting a new source key creates a document and its first immutable revision.
Submitting an existing key with identical text and user-visible metadata returns
`unchanged`. A change creates a new immutable revision.

The current revision is the greatest revision number for a document. All prior
revisions remain readable so existing citations remain resolvable.

The revision digest covers exact text and canonical user-visible metadata:
title, source type, occurred-at time, and custom metadata.

### 3.4 Batch ingestion

One command accepts one or many documents. It:

1. validates command options and any manifest envelope;
2. enumerates inputs without retaining complete document bodies in memory;
3. reports a malformed JSON Lines record or repeated source key as a failed
   item while preserving earlier publications;
4. reuses one loaded embedding-model session across all changed documents;
5. embeds passages in bounded batches, which may span documents as an
   implementation optimization;
6. publishes each document independently; and
7. returns one outcome per document and a compact aggregate result.

Per-document outcomes are `added`, `updated`, `unchanged`, or `failed`.
Aggregate status is `complete`, `partial`, or `failed`.

A failed document cannot corrupt another document. Rerunning the same input
converges without duplicate documents.

Ingestion completes passage generation, embedding, lexical indexing, and vector
indexing before a document is reported as published. There is no separate public
index-processing, readiness, pending-work, cleanup, attempt-history, or rebuild
workflow, and no per-document derived-search fingerprint.

### 3.5 Removal

Removal is explicit:

```sh
commonplace remove --source-key notes/obsolete.md
```

The command permanently deletes one document, all of its revisions and
passages, lexical and vector rows, and evidence links. It reports detached
evidence counts and affected knowledge IDs, rebuilds the graph, and publishes
the SQLite and graph change as one coordinated operation. A process crash in
the publication window is detected by version mismatch and repaired with
`commonplace graph rebuild`.

Directory absence never implies deletion. Reingesting a removed source key
creates a new document identity. There is no batch removal, restore, tombstone,
or automatic withdrawal of affected knowledge.

## 4. Passage preparation

One deterministic paragraph-aware algorithm prepares passages:

1. accumulate whole paragraphs up to a fixed target size;
2. preserve exact source text and offsets;
3. split an oversized paragraph into fixed, nonoverlapping windows; and
4. assign stable ordinals within the revision.

Passages do not overlap. Adapters cannot provide custom passage boundaries.

Each passage stores:

- its canonical passage ID;
- document and revision IDs;
- ordinal and exact text;
- start and end byte offsets; and
- enough source metadata to hydrate a complete citation.

## 5. Hybrid search

`commonplace search` performs one complete retrieval workflow:

```text
query
    -> lexical candidates
    -> vector candidates
    -> deterministic fusion and deduplication
    -> local reranking
    -> evidence hydration
    -> ordered results
```

SQLite owns canonical passages, FTS rows, and vector rows. The embedding model
and reranker are pinned local models. They load lazily and remain loaded for the
CLI process. Commands that do not require inference do not load them.

Dense retrieval uses one SQLite vector extension. There is no application-level
exact vector scan or alternate vector backend.

Hybrid search fails clearly if a required model or vector representation is
unavailable. It never silently degrades to lexical-only search.

Every result includes:

- exact passage text;
- document, revision, and passage IDs;
- source title, type, time, and metadata;
- exact byte offsets;
- score or rank information; and
- explicit truncation information.

Supported filters are optional `--since` and repeatable `--source-type`. Both
apply identically to lexical and vector candidate selection. Structured entity
or graph-expansion filters are not part of hybrid search.

Ordinary results include final rank and may include a lightweight final score.
They do not expose candidate-stage membership, model identities, fusion
contributions, or retained search explanations. A narrow debug response may be
added only when retrieval evaluation requires it.

## 6. Knowledge schema

The user defines an additive vocabulary:

- entity types;
- identifier schemes;
- directed predicates;
- allowed subject entity types;
- allowed object entity types or literal value kinds.

Schema terms are lowercase ASCII identifiers. Applying a schema may add terms
and compatible predicate endpoints. Existing terms cannot be redefined or
removed.

Each term records the schema version in which it was introduced, and each
knowledge item records the schema version under which it was validated. A no-op
schema application leaves the version unchanged. The product does not retain
complete historical schema snapshots because the additive current schema can
interpret all stored knowledge.

Schema operations include validation without mutation:

```sh
commonplace schema apply schema.json --check --json
commonplace schema apply schema.json --json
commonplace schema show --json
```

Schema application changes SQLite only. It does not rebuild Grafeo because the
graph projects only vocabulary referenced by active knowledge.

The physical SQLite and Grafeo schemas remain generic. User vocabulary is data,
not generated database DDL.

## 7. Entities and authored knowledge

### 7.1 Entities

An entity has:

- a stable canonical ID;
- a display name;
- zero or more active types;
- zero or more aliases; and
- zero or more identifiers.

An identifier is unique within its declared scheme. Authoring resolves entities
by canonical ID or exact identifier. Ambiguous name or alias matches are errors;
the system does not silently merge entities.

A bare identified entity may exist in SQLite before its types or relationships
are known. It becomes graph-active only when it participates in an active type
membership or fact. `commonplace record` may add or remove aliases and identifiers
atomically. Removal identifies the entity by canonical ID and does not merge or
delete entities. Entity and metadata creation may carry an informational
`created_by` label, which has no authorization or lifecycle semantics. Metadata
correction is not retained as knowledge history and does not rebuild the graph
because aliases and identifiers are not projected.

### 7.2 Knowledge items

Type memberships and facts share a common knowledge-item lifecycle:

- stable knowledge ID;
- the schema version used to validate the item;
- creation time;
- optional short `created_by` label;
- optional withdrawal time and `withdrawn_by` label; and
- zero or more evidence passages.

A fact is either:

- a directed relationship from a subject entity to an object entity; or
- a directed assertion from a subject entity to a canonical JSON literal.

Literal kinds are string, integer, boolean, and normalized UTC RFC 3339
timestamp.

Repeated successful writes create distinct knowledge IDs. The product does not
perform semantic deduplication.

Every knowledge result includes a support array, including when it is empty.
Multiple cited passages record the evidence used when the item was authored;
they do not create a separate conjunctive eligibility lifecycle.

Decisions, actions, blockers, outcomes, and statuses are ordinary user-defined
predicates and facts. They have no dedicated tables, commands, or lifecycle
rules.

### 7.3 Atomic authoring

A `commonplace record` request may create or resolve entities, correct metadata, add type
memberships, add facts, attach evidence, and publish a new graph when projected
knowledge changes. The entire request succeeds or fails as one operation.

Authoring is batch-first:

```sh
commonplace record facts.json --json
commonplace record --jsonl facts.jsonl --json
```

One request may declare request-local entity names and reuse them across several
facts. It returns one success or one failure; there are no partially successful
knowledge writes.

Validation includes:

- schema terms exist;
- predicate endpoints are allowed;
- identifiers are unique;
- entity references resolve exactly;
- literal values match the predicate;
- cited passages exist; and
- every supplied quote and offset matches stored source text.

### 7.4 Withdrawal and history

`commonplace withdraw` marks one active knowledge item withdrawn and republishes the
graph. Withdrawn items remain readable from SQLite history but are absent from
the active graph.

Withdrawal is final. There is no restore lifecycle; a corrected assertion is a
new knowledge item.

A withdrawal request accepts only knowledge-item IDs and validates the complete
resulting active state before mutation. It is rejected if any fact that would
remain active loses every permitted type on a required subject or object
endpoint, unless that dependent fact is included in the same withdrawal request.
Any invalid or already withdrawn ID rejects the complete batch.

## 8. Graph projection and queries

Grafeo is the required query engine and a derived store. It contains all active
knowledge and enough source data to return citation-ready graph results.

The projection has one stable generic shape:

| Element | Required properties |
| --- | --- |
| `Entity` | `id`, `entity_id`, `name` |
| `EntityType` | `id`, `entity_type_id`, `name` |
| `KnowledgeItem` | `id`, `knowledge_item_id`, `kind`, `schema_version` |
| `Value` | `id`, `knowledge_item_id`, `literal_kind`, `literal_json` |
| `Passage` | `id`, `passage_id`, `revision_id`, `ordinal`, `start_byte`, `end_byte`, `text`, `title`, `source_type`, `occurred_at`, `metadata_json` |
| `Document` | `id`, `document_id`, `source_key` |
| `HAS_TYPE` | `knowledge_item_id` |
| `RELATIONSHIP` | `knowledge_item_id`, `predicate` |
| `ASSERTION` | `knowledge_item_id`, `predicate` |
| `SUPPORTED_BY`, `FROM` | no properties |

Edges connect `(Entity)-[:HAS_TYPE]->(EntityType)`,
`(Entity)-[:RELATIONSHIP]->(Entity)`,
`(Entity)-[:ASSERTION]->(Value)`,
`(KnowledgeItem)-[:SUPPORTED_BY]->(Passage)`, and
`(Passage)-[:FROM]->(Document)`. A `Value` node is unique to its assertion and
uses `value:<knowledge_item_id>` as its `id`. Other node IDs use the public
tagged SQLite ID. Aliases and identifiers remain SQLite identity metadata and
are not projected.

Only entities participating in active knowledge are projected. Only passages
supporting active knowledge and their documents are projected. Uncited corpus
passages remain searchable in SQLite. Ordinary document ingestion does not
rebuild Grafeo; source removal does because it can remove projected evidence.
Only entity types and predicates referenced by active knowledge are projected;
the complete vocabulary remains available through `commonplace schema show`.

The complete logical shape is therefore:

- entity and entity-type nodes;
- generic relationship and assertion records;
- value nodes for literal assertions;
- knowledge-item nodes;
- citation-ready passage nodes;
- minimal document nodes;
- evidence relationships from knowledge items to passages; and
- predicate names and literal kinds on their semantic records.

Every knowledge-changing operation builds and verifies a complete candidate
graph before publishing it. Graph reads compare the graph's
`knowledge_version` with SQLite and fail closed on mismatch.

A crash in the narrow interval between graph activation and SQLite commit can
leave a version mismatch. `commonplace graph rebuild` acquires the writer and graph
publication locks, builds a complete candidate from committed SQLite state,
verifies it, and activates it without changing `knowledge_version`. Failure
leaves the previous graph untouched. There is no automatic repair, retained
graph-generation system, or general transaction coordinator.

`commonplace graph query` accepts read-only Cypher through a native Grafeo
`Role::ReadOnly` session. Mutation clauses are rejected by the graph engine
rather than filtered only by string matching. Grafeo's native query deadline
enforces the configured time budget, although cancellation may occur after the
deadline. Ctrl+C terminates the synchronous CLI process rather than cancelling
an individual in-process query. The command reads at most `row_limit + 1` rows
so it can report truncation without retaining a result set.

Graph-query output contains ordered `columns` and `rows`. JSON-native scalar,
list, and object values serialize recursively. Nodes serialize as a tagged
object containing sorted labels and properties. Relationships serialize as a
tagged object containing their type, endpoint `id` values, and properties.
Paths serialize as ordered node and relationship arrays. Unsupported Grafeo
values fail explicitly rather than being stringified.

## 9. CLI contract

The command line is the only stable programmatic interface. Commands return machine-readable JSON with:

- an explicit success or error shape;
- stable tagged IDs such as `doc:42`, `passage:306`, and `knowledge:94`;
- bounded search, graph-query, and batch result collections;
- explicit partial or truncated status; and
- errors that identify the failed input and corrective action.

Primary commands are:

```text
commonplace init
commonplace ingest
commonplace remove
commonplace search
commonplace schema show
commonplace schema apply
commonplace record
commonplace withdraw
commonplace get
commonplace graph schema
commonplace graph query
commonplace graph rebuild
```

`commonplace init` creates the SQLite database, Grafeo storage location, and local
configuration for one knowledge base. It does not create model profiles,
approval state, cache profiles, or a separate model-preparation workflow.
Pinned models load on first use and may be obtained through the selected
runtime's standard cache mechanism.

`commonplace get ID` is the only direct authoritative read command. It accepts tagged
document, revision, passage, entity, and knowledge-item IDs. Document results
include revision IDs; revision results include complete stored text and metadata;
passage results include their exact citation; entity results include active type
and fact IDs; and knowledge-item results include their subtype, evidence, and
withdrawal state. Withdrawn knowledge remains readable by ID. There is no
generic history mode, centered-context read, semantic revision comparison, or
separate noun-specific read command.

`commonplace get` returns the complete requested record, including all referenced IDs,
and does not silently truncate authoritative state. Existing source-size and
per-request write limits bound individual stored values; cumulative revision
and entity histories may grow with use.

`commonplace schema show` returns the complete user vocabulary from SQLite.
`commonplace graph schema` returns the stable physical graph shape, canonical property
names, evidence fields, and example read-only Cypher patterns. Standard
`--help`, these two schema commands, and generated input descriptions replace a
general `commonplace capabilities` command.

Complex JSON boundaries describe themselves:

```sh
commonplace ingest --describe --json
commonplace schema apply --describe --json
commonplace record --describe --json
commonplace graph schema --json
```

Descriptions are generated from the validators used by execution and include an
input JSON Schema plus a minimal valid example. `commonplace record --describe` also
includes current entity types, predicates, endpoint rules, literal kinds, and
schema version. The project does not maintain a second hand-written contract
model.

Every command's JSON result includes:

- a stable operation name and contract version;
- overall status;
- an aggregate summary for batch commands;
- bounded result entries for search, graph-query, and batch commands;
- stable IDs and copy-ready selectors;
- explicit partial or truncation state where applicable;
- per-item failures with stage and safe error code; and
- enough context for the next operation.

If a caller cannot determine whether a successful non-idempotent knowledge
request was received, it inspects canonical knowledge-item IDs before retrying.
The system does not retain request IDs or replay responses.

The CLI parses input and serializes output. Business rules remain in application
workflows.

## 10. Product exclusions

Commonplace does not include:

- hosted APIs or collaboration;
- authorization or policy systems;
- source-specific connectors inside the core;
- automatic semantic extraction;
- natural-language query planning;
- caller-supplied passage or anchor hierarchies;
- seed sets or a second evidence representation;
- first-class mention records;
- domain-specific built-in record kinds;
- schema proposal, approval, or administrator workflows;
- competing type claims or classification-selection workflows;
- record-supersession graphs;
- generated tables for user vocabulary;
- historical schema reconstruction;
- automatic semantic deduplication;
- source deletion inferred from synchronization;
- multiple selectable model profiles;
- background workers or job recovery;
- incremental graph projection;
- historical or withdrawn knowledge in Grafeo;
- retained graph sessions or query handles;
- a general query-plan language, continuation tokens, or retained result sets;
- fixed relationship, decision, count, or proof-inspection query APIs;
- a general capability-discovery command;
- generalized diagnostics or resource accounting; or
- a stable library SDK, compatibility adapters, or migration machinery.
