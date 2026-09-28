# Commonplace

Status: Working revision after SQLite schema review  
Revision: 0.43  
Previously approved: revision 0.42 on 2026-09-27  
Review: Revision 0.43 requires joint rereview with the SQLite persistence design.  
Purpose: Define the smallest clear architecture that preserves the complete
ingestion and query product while remaining easy to understand, change, and
extend.

## 1. Product goal

Build a personal, local knowledge system that lets an agent:

1. ingest one document or a collection of documents efficiently;
2. retrieve relevant source passages through complete hybrid search;
3. explicitly author cited entities, types, relationships, and literal facts;
4. query the resulting graph with full graph-query expressiveness; and
5. receive exact supporting evidence from both retrieval paths.

The system is for one person using their own local documents. It is not a hosted
service, a collaboration platform, or a multi-user authorization system.

The primary product loop is:

```text
ingest sources
    -> hybrid search
    -> agent interpretation
    -> explicit cited knowledge authoring
    -> graph query
    -> source verification
```

The system should minimize agent calls and agent-carried document content.
Deterministic work belongs inside the executable. Semantic interpretation belongs
to the calling agent.

## 2. Design principles

### 2.1 Preserve capabilities, remove machinery

The rewrite is not intended to reduce the useful ingestion or query capability.
It should remove implementation machinery that does not directly improve:

- source ingestion;
- exact evidence retention;
- passage preparation;
- hybrid retrieval;
- explicit knowledge authoring;
- graph projection; or
- text and graph querying.

Features primarily justified by multi-user coordination, hostile concurrency,
distributed execution, exhaustive auditing, or hypothetical scale are out of
scope unless a concrete local workflow requires them.

### 2.2 Code must carry product weight

Every meaningful block of code should earn its cost by doing at least one of:

- delivering a required ingest or query capability;
- protecting data from a realistic local failure;
- preserving exact evidence or deterministic results;
- making a known future change local to one component; or
- materially reducing the number or complexity of agent calls.

Code is suspect when it exists mainly to:

- enumerate hypothetical failure states;
- defend against hostile callers in a trusted local process;
- preserve an interface with no external consumer;
- generalize a behavior that has one implementation;
- prepare for scale that has not been observed;
- capture diagnostics that will rarely be read; or
- make a simple synchronous operation resumable.

Lean does not mean silent failure or careless data handling. Required operations
must still validate their inputs, use transactions, return clear errors, and avoid
publishing incomplete results. The rule is to handle failures that can occur in
the chosen design, not every failure a distributed production platform might
eventually encounter.

### 2.3 Greenfield design with no backward compatibility

This is a new implementation in a new repository. It has no backward
compatibility obligation to any earlier implementation.

The v2 design must not include:

- a reader for an earlier SQLite format;
- a migration or conversion command;
- preservation of earlier database IDs;
- compatibility aliases for earlier CLI commands;
- legacy request or response shapes;
- dual reads, dual writes, or old/new execution paths;
- adapters around earlier service APIs;
- feature flags that select earlier behavior;
- tests whose purpose is to preserve an earlier implementation's incidental
  behavior; or
- dependencies on code, schemas, manifests, models, or internal abstractions from
  another repository.

Earlier behavior may be studied to identify product capabilities and failure
cases. It is not a contract, implementation dependency, or starting architecture.
Every v2 API, table, type, command, and module must be justified directly by this
document and observed v2 usage.

The only supported initialization path is a fresh v2 knowledge base. Existing
sources and explicit knowledge inputs may be ingested again through v2 interfaces.
An older database is never opened, modified, imported, upgraded, or silently
reset.

During early v2 development, an incompatible v2 storage change also uses a fresh
database and reingestion rather than migration code. A migration becomes a
separate product capability only if a real retained v2 dataset later makes it
necessary and the user explicitly chooses to add it.

The passage algorithm, embedding identity, vector representation, and lexical
storage format are part of that exact store format. An incompatible change bumps
the format and causes the application to refuse the old store. The initial
product does not retain a per-document search fingerprint or rebuild derived
search rows in place; create a fresh store and reingest instead.

### 2.4 One personal knowledge base

The first version has one local knowledge base. It does not implement tenants,
corpora, namespaces, principals, grants, writer bindings, or per-user permissions.
Anyone able to open the local database and run the CLI has full access.

### 2.5 SQLite is authoritative

SQLite owns all durable authored truth:

- source documents and revisions;
- exact source text and metadata;
- passages and source offsets;
- embeddings and lexical-search data;
- user-defined schema;
- entities and their types;
- aliases and identifiers;
- relationships and literal assertions;
- evidence associations;
- withdrawals and simple history; and
- the canonical IDs used by every interface.

No KG-owned identifier needs to be persisted in a source file or external staging
directory.

### 2.6 Ladybug is essential

Ladybug is not an optional afterthought. Graph ingestion and graph querying are
core product capabilities and must shape the canonical model from the beginning.

Ladybug is nevertheless derived from SQLite:

- SQLite remains authoritative.
- Ladybug contains no unique authored fact.
- Every graph object retains its canonical SQLite ID.
- The graph can be rebuilt from SQLite.
- A surviving graph file does not override SQLite.

"Rebuildable" describes the recovery model. It does not make the graph
nonessential to the product.

### 2.7 One internal ingestion pipeline

The engine has one normalized document input and one ingestion implementation.
Files, directories, stdin, JSON Lines, and future connectors are adapters that
produce the same input stream. They do not own storage, revision, passage,
embedding, indexing, or graph behavior.

### 2.8 Batch first

Batch ingestion is a foundational operation. A one-document ingestion is a batch
of one, not a separate implementation.

### 2.9 Synchronous and rerunnable

The first version runs ingestion and projection synchronously. Failure scope is
declared by workflow rather than forced through one generic recovery mechanism:

- document ingestion is atomic per document, so successful documents in a batch
  remain committed when another document fails; and
- schema application, knowledge authoring, and withdrawal are atomic for the
  complete request during ordinary execution.

The caller corrects the cause and reruns. Source identity makes document reruns
convergent. A failed knowledge request leaves no ordinary-execution residue and
may be rerun safely; a successful or uncertain knowledge request must be
inspected before repetition because successful authoring is intentionally
non-idempotent.

Do not initially build:

- queues;
- workers;
- leases;
- heartbeats;
- durable job orchestration;
- per-stage resume protocols;
- retry ledgers; or
- distributed transaction emulation.

Repeated local work is preferable to maintaining recovery machinery for an
unlikely personal-use failure.

### 2.10 Explicit query interfaces

Text retrieval and graph retrieval are different operations. The system should
not attempt to infer which one a natural-language prompt requires.

### 2.11 Seams without speculative alternatives

Implement one selected behavior for each v2 decision. Do not make every decision
runtime-configurable and do not retain alternate implementations merely to make
the decision reversible.

Place a small software seam where a plausible later change would otherwise spread
through unrelated capabilities. A seam means one clear ownership boundary, pure
policy function, narrow internal trait, or isolated SQL/projection module. It
does not imply a plugin system, provider registry, compatibility adapter, second
storage path, or public SDK.

Initial seams should keep these changes local:

- passage-generation policy;
- embedding runtime and model;
- reranker;
- fusion policy;
- SQLite vector-extension integration;
- source adapters;
- Ladybug engine integration and projection shape; and
- CLI parsing/serialization versus application workflows.

Canonical data semantics do not need artificial reversibility. An incompatible
early storage or contract decision may require a fresh v2 store and reingestion
rather than migrations, dual paths, or abstraction layers.

The calling agent chooses:

- `kg search` for hybrid passage retrieval;
- `kg graph query` for graph retrieval; or
- both.

## 3. Architecture

```text
                         Input adapters
             file | directory | stdin | JSONL | connector
                              |
                              v
                    Normalized DocumentInput
                              |
                              v
                    Canonical ingest pipeline
                              |
             +----------------+----------------+
             |                                 |
             v                                 v
       source and passages              hybrid projections
          in SQLite                    in SQLite
                                             |
                                  lexical + vector + rerank


                      Explicit knowledge input
                              |
                              v
                  Canonical knowledge authoring
                              |
                              v
              entities, types, assertions, evidence
                         in SQLite
                              |
                              v
                  complete Ladybug projection
                              |
                              v
                    read-only Cypher queries
```

The implementation should be layered so that each layer has one clear
responsibility and can be changed without rewriting neighboring layers.

Suggested dependency direction:

```text
CLI and adapters
    -> application workflows
        -> domain operations
            -> SQLite repositories
            -> embedding/ranking providers
            -> Ladybug projector/query executor
```

Lower layers must not depend on the CLI. SQLite and Ladybug details must not leak
through every domain type.

## 4. Source ingestion

### 4.1 Normalized document input

Every adapter produces the same logical input:

```text
DocumentInput
    source_key     stable identity assigned by the source adapter
    text           exact normalized source text
    title          optional display title
    source_type    descriptive source kind
    occurred_at    optional source timestamp
    metadata       small JSON object
```

The `source_key` identifies the logical source:

- an email adapter uses the immutable message ID;
- an editable Teams message keeps its message ID across edits;
- a rolling meeting summary keeps one summary key across replacements;
- a directory scanner uses a normalized root-relative path;
- stdin requires the caller to supply a source key.

SQLite maps the external source key to a KG-owned internal document ID.

### 4.2 Input adapters

The initial core should support:

```sh
kg ingest document.md
kg ingest document-a.md document-b.md
kg ingest ./notes --recursive
kg ingest --stdin --source-key teams/message/123
kg ingest --jsonl documents.jsonl
kg ingest --manifest corpus.json
```

The direct file and directory adapters should initially support Markdown and
plain UTF-8 text. They exist because deterministic local scanning is faster and
cheaper than making an agent open and transmit every file.

For directory scanning, the default source key is the normalized path relative
to the supplied root. Renaming a file therefore creates a new source. A manifest
or JSON Lines input may provide a stable source key when identity must survive a
path change. Do not implement inode tracking or rename detection.

Email, Teams, websites, PDFs, and future sources do not require separate core
pipelines. An agent or connector may obtain and interpret them, then stream
normalized documents through stdin or JSON Lines. Sensitive sources therefore do
not have to be serialized into a staging directory.

Format-specific adapters may be added later without changing the ingestion core.

### 4.3 Batch behavior

One command may ingest one or many documents. The command should:

1. enumerate or receive all inputs;
2. validate the batch envelope;
3. reject duplicate source keys within the request;
4. process each document through the same pipeline;
5. load one embedding-model session for the command and reuse it for every changed
   document;
6. embed passage text in bounded inference batches, which may contain passages
   from multiple documents;
7. report one outcome per document; and
8. return a compact aggregate summary.

The batch must not initialize or reload the embedding model once per document.
Batching does not mean retaining every document or passage in memory at once:
enumeration, passage preparation, embedding, and publication may be streamed in
bounded groups while the same loaded model remains alive.

Expected per-document outcomes:

- `added`;
- `updated`;
- `unchanged`; or
- `failed`.

One malformed document should not corrupt another document. Successful documents
may remain committed when another item fails. The aggregate command status is:

- `complete` when every document is `added`, `updated`, or `unchanged`;
- `partial` when at least one document succeeds and at least one fails; or
- `failed` when no document succeeds.

Rerunning the same source batch converges without duplicating documents.

Ordinary ingestion never treats a missing file as deletion. Source removal is an
explicit command. A future synchronization adapter may deliberately reconcile a
complete external source, but it is not implicit directory behavior.

The initial removal command accepts exactly one source key and permanently
deletes that document, its revisions, passages, lexical rows, and vectors.
Evidence links to those passages are deleted with them. Authored knowledge is not
withdrawn; an item whose last citation was removed becomes uncited, which is
valid in this model. The command reports the number of detached evidence links
and affected knowledge-item IDs, rebuilds Ladybug, and succeeds or rolls back as
one knowledge-versioned operation. Reingesting the same source key creates a new
document identity. Do not add batch removal, restore, tombstones, cascade
withdrawal, or deletion inferred from directory absence.

### 4.4 Source revisions

Sources are historical records of what was observed, received, or said. They are
normally immutable:

- email is normally immutable;
- a meeting summary is normally immutable;
- many imported artifacts never change;
- a new meeting, email, or summary is a new source rather than an update to an
  earlier source; and
- changing conclusions do not rewrite the source that originally supported them.

If a new source key is submitted, create a document.

If an existing source key is submitted with identical content and relevant
metadata, report `unchanged`.

Changed content under an existing source key is an exceptional source correction
or an edit to a genuinely editable source. Create a new immutable revision and
make it current. Do not use source revision as the ordinary mechanism for changing
proposals, outcomes, decisions, status, or interpretation.

All user-visible source metadata is part of the immutable revision:

- title;
- source type;
- occurred-at timestamp; and
- custom metadata.

Changing any of these values under the same source key creates a new revision,
even when the text is unchanged. Internal operational values such as
`last_ingested_at` may update in place and are never presented as historical
source metadata.

Keep revision history because it is inexpensive and protects citations to earlier
content. Do not implement a generalized source-state machine for every metadata,
policy, activation, and processing transition.

Explicit removal is the exception to retained revision history: it deletes the
selected document and its evidence. Ordinary source updates still preserve all
prior revisions.

### 4.5 Complete ingest workflow

Normal ingestion should complete all deterministic source preparation in one
command:

```text
validate input
    -> resolve source identity
    -> store source revision
    -> produce passages
    -> produce embeddings
    -> update lexical index
    -> publish searchable representation
```

The calling agent should not normally have to invoke separate add, passage,
embedding, indexing, and readiness commands.

There is no public indexing lifecycle in the initial product. Do not expose index
status, pending work, cleanup, attempt history, or per-document rebuild commands.
If the passage algorithm, embedding identity, vector representation, or lexical
storage changes incompatibly, create a fresh store and rerun `kg ingest` over the
source collection. Do not add per-document search fingerprints, mixed derived
formats, an in-place reindex branch, or a public rebuild command initially.

## 5. Exact evidence

Exact evidence is a defining capability.

Each passage must retain:

- canonical passage ID;
- canonical document and revision IDs;
- exact quote;
- start and end offsets;
- passage ordering;
- source title and relevant metadata; and
- enough information to verify the quote against the stored revision.

`start` and `end` are zero-based, half-open UTF-8 byte offsets into the exact
stored revision text. Both must fall on valid UTF-8 character boundaries, and
the byte slice `text[start..end]` must equal the stored passage quote. Do not
normalize newlines or Unicode before computing or verifying offsets.

Search results and authored knowledge must reference canonical passages or exact
source ranges. A citation remains resolvable after the source receives a new revision. Explicit
source removal deletes citations into that source.

The system should clearly distinguish:

- historical evidence that still exists; and
- current support attached to active authored knowledge.

It does not need generalized authorization fences, metadata-snapshot permissions,
or an elaborate anchor-set lifecycle.

## 6. Passage preparation and hybrid search

Hybrid search is required in the first working product.

### 6.1 Index representation

SQLite stores:

- canonical passages;
- lexical-search rows;
- normalized passage embeddings through a SQLite vector extension; and
- the small amount of configuration identity required to detect incompatible
  embeddings.

Dense retrieval uses the SQLite vector extension from the first version rather
than loading every vector for an application-level exact scan. The specific
extension is selected with the implementation language and packaging approach.
Do not build a second vector-storage path.

All normalized text uses one deterministic paragraph-aware passage function:

1. accumulate whole paragraphs up to a fixed target size;
2. preserve exact source offsets and text;
3. split a single oversized paragraph into fixed nonoverlapping windows; and
4. produce stable passage order and identity from the source revision and
   boundaries.

There is no overlap and no caller- or adapter-supplied passage boundary in the
initial product. Source-specific segmentation may be added later by changing the
passage function and reingesting sources; passages are derived data.

Start with:

- one passage-generation strategy;
- one embedding model;
- one embedding representation;
- one lexical configuration;
- one fusion formula; and
- one reranker.

Do not initially build multiple selectable model profiles, generalized provider
registries, or compatibility matrices.

The embedding model and reranker run locally and are pinned by the application.
They use the selected runtime's standard model cache. `kg init` creates the local
knowledge base but does not manage profiles, attach existing identities, record
model approvals, or implement a separate model-preparation workflow.

Model-provider construction is lightweight. Each provider loads its pinned model
weights lazily on its first actual inference call and retains the loaded session
for the rest of that CLI process. Commands that do not embed or rerank must not
load either model. On first relevant use, the runtime may retrieve missing model
artifacts through its ordinary supported mechanism. If required artifacts are
unavailable, that command fails clearly. Source text is never sent to a remote
inference service.

Within one `kg ingest` invocation, initialize the embedding model once and reuse
that session across the complete document batch. Feed it bounded groups of
passages rather than creating a model session per document or one unbounded
in-memory inference request.

### 6.2 Search pipeline

`kg search` performs the complete retrieval operation:

```text
query
    -> lexical candidates
    -> vector candidates
    -> deterministic fusion and deduplication
    -> reranking
    -> exact evidence hydration
    -> ordered cited results
```

Search must not require the agent to issue a second read for every returned
passage. Each bounded result should already include:

- exact passage text;
- source identity and metadata;
- offsets and citation ID;
- ranking score or rank information;
- explicit truncation; and
- stable IDs for later reference.

There is no lexical-only silent fallback when the advertised operation is hybrid
search. Failure to initialize the required model or representation is an explicit
error.

### 6.3 Search interface

```sh
kg search "What delayed Atlas?" \
  --since 30d \
  --source-type email \
  --source-type meeting-summary \
  --limit 20 \
  --json
```

The command should be usable in one agent call. Internal search stages remain
implementation details unless lightweight diagnostics are specifically requested.

The initial search filters are:

- optional `--since`; and
- repeatable `--source-type`.

Both filters apply identically before lexical and vector candidate selection.
Do not initially add arbitrary metadata expressions, entity resolution, subject
expansion, or graph-derived search scope. Structured scope belongs in Cypher.

## 7. User-defined knowledge schema

The user must be able to define and evolve the knowledge vocabulary without code
changes.

The physical SQLite schema remains stable. User-defined vocabulary is stored as
data, not generated DDL.

Minimum schema concepts:

- entity types;
- identifier schemes;
- predicates;
- predicate object kind: entity or supported literal kinds;
- allowed subject types;
- allowed object types; and
- optional descriptive text.

Example:

```json
{
  "entity_types": [
    {"name": "person"},
    {"name": "project"},
    {"name": "product"},
    {"name": "initiative"}
  ],
  "predicates": [
    {
      "name": "owns",
      "subject_types": ["person"],
      "object_kind": "entity",
      "object_types": ["project", "product"]
    },
    {
      "name": "decision",
      "subject_types": ["project", "product", "initiative"],
      "object_kind": "string"
    }
  ]
}
```

Apply a schema in one command:

```sh
kg schema apply schema.json --json
kg schema apply schema.json --check --json
kg schema show --json
```

The first version should permit additive evolution and reject incompatible
redefinition of terms already used by knowledge.

Maintain one monotonic schema-version integer. Store each type, identifier scheme,
and predicate once with the version in which it was introduced. Authored
knowledge records the schema version under which it was validated.

Because evolution is additive and existing terms cannot be redefined or removed,
the current schema is always sufficient to interpret older facts. Do not store a
complete repeated schema snapshot or provide historical schema reconstruction.

SQLite is the complete authoritative vocabulary catalog. Applying schema updates
SQLite only and does not rebuild Ladybug. The graph projects only types and
predicate identifiers used by current active knowledge; unused vocabulary is
discoverable through `kg schema show`, not Cypher. `kg graph schema` describes
the fixed physical graph representation rather than duplicating the full user
vocabulary.

Do not initially implement:

- schema proposal workflows;
- evidence-backed schema-generation records;
- approval attestations;
- administrator identities;
- protected rationales;
- schema retry receipts; or
- separate mandatory validate and apply calls.

## 8. Knowledge model

### 8.1 Stable generic tables

Use a small stable relational model rather than generating domain-specific tables.
Conceptually:

```text
entities
entity_types
knowledge_items
entity_type_memberships
aliases
identifiers
assertions
knowledge_item_evidence
withdrawals
```

Names such as `project`, `person`, and `owns` are schema rows referenced by generic
knowledge rows.

`knowledge_items` supplies one canonical `knowledge_item_id`, authored metadata,
and withdrawal state for type memberships and assertions. Their concrete rows
remain in dedicated tables for direct validation and querying.

Aliases and identifiers are add-only entity identity metadata initially. They
use their natural entity/value keys and have no public row IDs, citations,
withdrawal history, or projected knowledge-item nodes. During experimental v2,
correcting one requires a fresh store/reload.

Entity type memberships use their own table rather than being encoded as generic
assertions. This keeps entity loading, endpoint validation, indexes, and Ladybug
`HAS_TYPE` projection direct. Type memberships and assertions share the same
`knowledge_item_id`, `created_at`, optional `created_by`, zero-or-more passage
citations, and optional withdrawal lifecycle.

Raw SQL discoverability is not a primary product interface. The supported
discovery surfaces are the CLI, hybrid search, and Ladybug graph.

### 8.2 Entities can be more than one thing

An entity has one stable identity and zero or more active types.

Example:

```text
Atlas
    type: project
    type: product
    type: initiative
```

Each type membership is explicitly authored and evidence-backed. A mistaken type
may be withdrawn without deleting the entity.

Do not implement competing classification claims, review sets, selected
classifications, or selection-event witnesses.

A predicate endpoint is valid when the entity has at least one active type allowed
by that endpoint.

### 8.3 Authored facts

The knowledge authoring model supports:

- entity creation;
- type memberships;
- add-only aliases;
- add-only identifiers;
- entity-to-entity relationships;
- entity-to-literal assertions, including decisions;
- zero or more exact passage citations on every authored type membership or fact.

Automatic extraction is not part of the KG engine. The calling agent interprets
search results and explicitly submits knowledge.

Decisions, proposed outcomes, actions, blockers, conflicts, statuses, and similar
concepts are ordinary user-defined predicates and facts. They have no dedicated
tables, encodings, lifecycle services, or built-in query commands. Cypher selects,
traverses, and counts them using the active schema.

Evidence is optional for first-class authored knowledge. There is one support
representation: `knowledge_item_evidence(knowledge_item_id, passage_id)`. A cited
type membership or fact has one or more rows; an uncited item has none. Do not
create seed sets, seed slots, or a second support kind. Query results always
return a support array, which may be empty, so callers can distinguish grounded
and ungrounded knowledge directly.

Hybrid search results are always grounded because they are passages. Only
explicitly authored knowledge may be uncited.

Do not store first-class entity mentions initially. Textual occurrence belongs to
hybrid search; the graph contains explicit authored entities and facts plus their
supporting passages. Add mention records only if real graph queries cannot be
answered by those two paths.

### 8.4 Uniform lifecycle

Authored knowledge uses one lifecycle:

- add an immutable evidence-backed fact;
- withdraw that exact fact when it is no longer accepted;
- preserve the withdrawn fact and its evidence in history.

Avoid separate correction and withdrawal protocols for type memberships,
relationships, decisions, and other assertions. Aliases and identifiers are
add-only metadata and are not accepted by `kg withdraw` initially.

Corrections are new facts plus withdrawal of the incorrect facts.

A withdrawal batch is validated against the complete resulting active state. It
is rejected if a remaining active fact would lose every permitted subject or
object type, unless that dependent fact is included in the same withdrawal
request.

Each authored type membership or fact records:

- database-generated `created_at`; and
- optional free-text `created_by`, such as `user`, `copilot`, or an importer
  label.

These fields are informational only. They do not create ownership, permissions,
writer bindings, protected provenance, or different lifecycle rules.

Source evidence and authored knowledge have intentionally different temporal
semantics:

- a meeting note remains the immutable record of what was said;
- a proposal recorded from that meeting remains historical evidence that the
  proposal existed;
- a later decision or changed outcome is recorded as a new fact with its own
  evidence; and
- a fact that should no longer appear as current is explicitly withdrawn.

A source edit or replacement does not automatically withdraw facts that cite an
older revision; their evidence remains historically readable. Explicit source
removal deletes its evidence links but does not withdraw the affected facts,
which may become uncited.

### 8.5 Entity resolution during authoring

`kg record` may refer to an existing entity by:

- canonical SQLite entity ID;
- exact identifier; or
- exact unique name or alias.

Resolution occurs inside the same SQLite transaction as authoring. Ambiguity is
an explicit error. Names never silently merge multiple entities.

This avoids forcing the agent to perform a prior entity read and copy a
selection witness merely to author a relationship.

### 8.6 Batch knowledge authoring

Knowledge authoring is batch-first:

```sh
kg record facts.json --json
kg record --jsonl facts.jsonl --json
```

One request may declare local entities and use them in several facts. During
ordinary command execution, validation, entity resolution, entity creation, type
assignments, facts, evidence links, and the required graph rebuild either all
succeed or the attempted knowledge change is rolled back. Withdrawals use the
separate `kg withdraw` command.

The command should be able to create or reuse entities, assign types, add aliases
or identifiers, create relationships or decisions, and bind exact evidence
without requiring a chain of separate CLI calls.

If any part of the knowledge input is invalid or ambiguous, or if its graph
rebuild fails, the command returns an error without retaining that command's
canonical knowledge changes. The previous complete graph remains in place. The
agent corrects the cause and reruns the complete input. Do not implement partial
knowledge-batch result states or a public recovery workflow for a failed command.

Knowledge authoring is append-only and does not provide automatic semantic
deduplication or retry-key idempotency. Repeating the same authoring input may
create another fact. This is acceptable for a personal knowledge base:

- every fact has a stable SQLite ID;
- graph queries may identify likely duplicates;
- the agent can present them for user review; and
- unwanted facts can be explicitly withdrawn.

Counts describe stored active fact records, not inferred unique real-world events.
Source ingestion remains idempotent because source keys have explicit identity;
knowledge facts do not. If a caller is uncertain whether a knowledge request
succeeded, it must inspect the returned or stored canonical item IDs before
retrying. Do not add persistent request IDs, stored response replay, or uncertain
commit recovery initially. The `record_knowledge` command boundary is the seam
where that capability may later wrap the existing workflow if observed use
requires it.

## 9. Ladybug projection

### 9.1 Product role

Every successful knowledge ingestion or authoring workflow should leave the
Ladybug graph queryable. Graph construction is part of the normal product flow,
not a developer-only example.

Document-only ingestion does not invent graph knowledge from prose. It prepares
source evidence and hybrid search. When structured knowledge is supplied or
authored, the same top-level workflow updates SQLite and rebuilds or refreshes
Ladybug.

### 9.2 Stable physical graph

User-defined schema terms should not require Ladybug DDL changes. Use a small
stable graph representation, conceptually:

```text
(Entity)-[:HAS_TYPE]->(EntityType)
(Entity)-[:RELATIONSHIP]->(Entity)
(Entity)-[:ASSERTION]->(Value)
(KnowledgeItem)-[:SUPPORTED_BY]->(Passage)
(Passage)-[:FROM]->(Document)
```

User predicates such as `owns`, `depends_on`, and `decision` are properties or
referenced schema IDs on generic fact/relationship records.

Aliases and identifiers remain SQLite identity metadata and are not projected
into Ladybug initially. Graph queries use canonical entity IDs and names.

Every projected entity, fact, passage, and document contains its canonical SQLite
ID. Graph results can therefore be traced back to authoritative source records.

Every semantic type-membership, relationship, or assertion edge carries its
canonical `knowledge_item_id`. A `KnowledgeItem` node with the same ID carries
the item kind and connects to supporting passages. This gives Cypher one explicit
join from a semantic edge to its evidence without projecting withdrawn history.

Only entities participating in at least one active type membership or fact are
graph-active. A bare entity may remain in SQLite but is not projected.

Passage projections are citation-ready. Each projected passage carries the
revision-specific display metadata needed for that citation. A Cypher query can
return:

- exact passage quote;
- passage offsets and ordinal;
- canonical passage, revision, and document IDs;
- document source key;
- title;
- source type; and
- source timestamp.

Document nodes carry only stable `document_id` and `source_key`; title, source
type, source timestamp, and other revision metadata live on the passage so
citations from several revisions cannot be mislabeled. This intentionally
duplicates bounded derived query data in Ladybug so an agent does not need a
second SQLite read for every graph result. SQLite remains authoritative, and the
copied fields are replaced by the next complete graph rebuild.

Only passages supporting active `KnowledgeItem` nodes are projected, along with
the documents containing those passages. Uncited passages remain searchable in
SQLite and are not duplicated into Ladybug. Ordinary document ingestion therefore
does not rebuild the graph; graph projection changes only when authored knowledge
or its active support changes.

### 9.3 Projection strategy

Begin with the simplest correct projection strategy. For a modest personal graph,
use a complete synchronous rebuild after every successful knowledge change,
including withdrawal.

Assign one monotonic `knowledge_version` to the candidate SQLite state and write
that same version into the candidate graph. Build the candidate graph while the
SQLite transaction remains open and retain the previous complete graph until the
candidate is ready. If validation or graph construction fails, delete the
candidate and roll back SQLite. The caller can rerun the original knowledge input
without duplicating facts from the failed attempt.

The short finalization step activates the candidate graph while retaining the
previous graph, commits SQLite, and then discards the previous graph. If the
SQLite commit returns an ordinary error, restore the previous graph and report
failure. These two local resources still do not have a shared atomic commit: a
process crash in that narrow window may leave their versions different. Every
graph open compares the graph's `knowledge_version` with SQLite and refuses the
query on mismatch; it never returns a graph known to be stale or ahead.

The initial recovery for that rare crash state is a fresh v2 store and
reingestion. Do not add immutable graph generations, pointer selection, orphan
cleanup, automatic repair, retry ledgers, a public rebuild command, or a general
transaction coordinator. Keep graph file activation behind `GraphRuntime` so a
later measured need can add immutable generations without changing knowledge
workflows, projection, Cypher, or the CLI.

`GraphRuntime` is the only stateful graph owner in the application context. A
deterministic snapshot-building query maps the open SQLite transaction into
`GraphSnapshot`; there is no separate stateful projector object.

Ladybug contains only graph-active entities, current type memberships, current
facts, and their active supporting passages and documents.
Withdrawn facts and historical knowledge remain readable in SQLite but are not
projected. Cypher therefore describes the current accepted graph without temporal
flags or withdrawal filtering.

Do not initially implement:

- incremental graph change feeds;
- retained graph sessions across CLI calls;
- graph generation orchestration;
- background refresh workers; or
- cross-database transaction emulation.

The implementation must still prevent queries from treating an incomplete graph
as complete.

## 10. Graph querying

The initial graph interface is read-only Cypher:

```sh
kg graph query --cypher '
  MATCH (a:Entity)-[r:RELATIONSHIP]->(b:Entity)
  WHERE a.name = "Atlas" AND r.predicate = "owns"
  RETURN a, r, b
' --json
```

This preserves Ladybug's expressive graph capability without implementing:

- a natural-language graph router;
- a custom graph planner;
- a second graph query language;
- many fixed one-hop operations;
- relationship-specific service methods;
- retained result handles; or
- separate count and proof-inspection protocols.

The graph contract must be documented well enough for an agent to generate valid
Cypher. Provide:

```sh
kg graph schema --json
```

It returns:

- the stable physical graph shape;
- canonical property names;
- evidence and citation fields;
- example read-only patterns.

The graph command relies on native Ladybug facilities for the safety required by
a local CLI:

- a genuinely read-only transaction or session;
- real interruption, cancellation, or a hard execution timeout;
- a result-row limit;
- explicit truncation;
- deterministic JSON serialization; and
- clear parse, execution, and resource errors.

The integration spike must prove that mutation attempts are rejected and a
deliberately expensive query is actually terminated. String inspection,
keyword blocklists, and abandoning a waiting thread do not satisfy this
contract. If the selected Ladybug integration cannot provide native read-only
execution and real cancellation, the graph-query architecture or runtime choice
must be reopened before implementation proceeds. Do not ship the required graph
capability behind a custom parser, watchdog, or worker supervisor.

## 11. CLI shape

The CLI should be organized around complete user intents:

```text
kg init
kg ingest
kg remove <source-key>
kg search
kg schema show
kg schema apply
kg record
kg withdraw
kg get
kg graph schema
kg graph query
```

The exact names may change, but the workflow boundaries should remain.

`kg get ID` is the only direct authoritative read command. It accepts typed IDs
for:

- document;
- revision;
- passage;
- entity; and
- knowledge item.

A document result includes its revision IDs. An entity result includes its
current type and fact IDs. A knowledge-item result is tagged as a type membership
or fact and includes its evidence and withdrawal state. A withdrawn item remains
readable by ID. The first
version has no generic history mode, centered source context, revision comparison,
or separate evidence/entity/fact read commands.

The CLI JSON contract is the only stable external programmatic interface in the
initial product. V2 does not promise a Python SDK, Rust crate API, .NET assembly
API, or stable importable service layer. Internal modules and types exist only to
organize the implementation and may change without compatibility work.

V2 is implemented in Rust and distributed as a self-contained release. A user
must not need Rust, Python, Node.js, .NET, a source checkout, or a build tool to
run `kg`. The release may contain the executable, pinned model files, and required
native database/runtime libraries; "self-contained" does not require every byte
to be embedded in one physical executable.

Before implementation proceeds beyond a disposable integration spike, prove that
the selected Ladybug version, SQLite vector extension, embedding model, reranker,
and release packaging work together on the first target platform. The spike must
run on a machine without a development toolchain or language runtime installed.
If that path requires maintaining substantial custom database bindings or
packaging machinery, reopen the language decision rather than carrying that
weight into the product.

The spike produces a short pass/fail record containing the tested platform,
component versions, release artifact contents, exact build/run commands, and
observed results for ingestion, vector search, reranking, graph rebuild,
read-only mutation rejection, query cancellation, and execution without an
installed language runtime. Missing evidence is a failed gate, not an assumption.

Complex JSON input boundaries expose machine-readable descriptions generated from
the same validators that execute the commands:

```sh
kg ingest --describe --json
kg schema apply --describe --json
kg record --describe --json
kg graph schema --json
```

Descriptions include the input JSON Schema and a minimal valid example. The record
description additionally includes the current user-defined entity types,
predicates, endpoint rules, literal kinds, and schema version so an agent does not
have to guess or perform separate discovery calls.

Do not hand-maintain duplicate schema documents or create a separate contracts
subsystem. Ordinary commands still return precise validation errors.

### 11.1 Efficient agent workflows

Document ingestion:

```sh
kg ingest ./notes --recursive --json
```

One non-file document:

```sh
cat message.json | kg ingest --stdin --source-key teams/message/123 --json
```

Streamed batch:

```sh
connector export --jsonl | kg ingest --jsonl - --json
```

Hybrid retrieval:

```sh
kg search "release planning" --limit 20 --json
```

Explicit authoring:

```sh
kg record facts.json --json
```

Graph retrieval:

```sh
kg graph query --file query.cypher --json
```

The agent should not routinely need to:

- call capabilities before normal work;
- add documents one by one;
- separately invoke passage and index preparation;
- read every search hit in another call;
- resolve an unambiguous entity in a prior command;
- separately hydrate evidence for ordinary bounded results; or
- choose between internal service implementations.

### 11.2 JSON contract

JSON is a first-class agent interface. Commands should return:

- a stable operation name and version;
- overall status;
- aggregate batch summary where relevant;
- bounded result entries;
- stable IDs and copy-ready selectors;
- explicit partial or truncation state;
- per-item failures with stage and safe error code; and
- enough context for the next semantic decision.

Avoid returning duplicated internal representations of the same result.

## 12. Failure handling

This is a trusted local application. Failure handling should be clear but small.

Required behavior:

- SQLite transactions protect each canonical document independently.
- Document batch items do not corrupt each other, and aggregate status is
  `complete`, `partial`, or `failed`.
- Schema, knowledge, and withdrawal requests are whole-request atomic during
  ordinary execution.
- Commands never report success for a failed stage.
- Rerunning the same source input does not duplicate the document.
- A failed knowledge request is residue-free and safely rerunnable.
- A successful knowledge request is non-idempotent; uncertain callers inspect
  canonical item IDs before deciding whether to retry.
- An incomplete hybrid or graph representation is not queried as complete.
- Unexpected errors are visible.

Not initially required:

- unknown-commit recovery protocols;
- 30-day retry-key tombstones;
- durable response replay;
- worker fencing;
- lease recovery;
- generalized diagnostic report retention;
- cross-process snapshots;
- global resource-accounting frameworks; or
- defensive behavior for hostile local callers.

## 13. Required v2 product capabilities

The v2 product requires:

- one and batch document ingestion;
- stable source identity and updates;
- exact source text;
- immutable source revisions;
- explicit source removal;
- deterministic passage preparation;
- exact citations;
- lexical retrieval;
- dense vector retrieval;
- fusion and deduplication;
- reranking;
- complete hybrid search;
- user-defined entity types and predicates;
- additive schema evolution;
- entities with multiple active types;
- aliases and identifiers;
- relationships and literal assertions with zero or more exact citations;
- explicit decisions;
- no built-in business record kinds; decisions, actions, blockers, and outcomes
  are ordinary schema-defined facts;
- batch knowledge authoring;
- exact entity resolution with ambiguity errors;
- simple fact withdrawal and history;
- complete SQLite-to-Ladybug graph projection;
- full read-only Cypher querying; and
- evidence traceability from graph results to SQLite sources.

## 14. Deliberately excluded from the initial design

Unless later use demonstrates a concrete need, do not build:

- hosted or network service interfaces;
- multi-user identity and authorization;
- corpus or namespace policy machinery;
- source connectors inside the core;
- automatic semantic extraction;
- natural-language query routing;
- generated domain-specific SQL tables;
- schema proposal and human-approval protocols;
- competing type claims and classification selection;
- per-fact specialized lifecycle rules;
- generalized processing orchestration;
- background workers and job recovery;
- multiple model profiles;
- compatibility layers for any earlier database, CLI, API, or output shape;
- migration or import code for another implementation;
- reuse of another implementation's internal service abstractions;
- retained query result handles;
- execution-explanation frameworks;
- durable diagnostic stores;
- arbitrary operational audit systems; or
- performance architecture for unobserved scale.

## 15. Immediate acceptance experiment

The first complete vertical slice should prove:

1. Initialize a fresh personal KG.
2. Batch-ingest a directory of Markdown documents.
3. Stream one non-file document with a stable source key.
4. Rerun ingestion and observe unchanged documents rather than duplicates.
5. Change one editable source and preserve its prior revision.
6. Run hybrid search and receive ordered exact citations.
7. Apply a small user-defined schema.
8. Record an entity with multiple types and an evidence-backed relationship.
9. Record a cited literal decision.
10. Build the Ladybug projection as part of authoring.
11. Run read-only Cypher that traverses the relationship and returns the decision
    with canonical evidence references.
12. Withdraw one authored fact and verify that it disappears from current graph
    queries while remaining readable in history.
13. Close and reopen the application and reproduce the same search and graph
    results.

This is the minimum useful product. Work that does not enable this experiment,
protect its data integrity, or fix an observed correctness problem should be
deferred.

Release-gating acceptance also proves:

- one embedding-model load for a multi-document command;
- at least one bounded embedding call spanning documents;
- exact vector-to-passage correlation and independent document publication;
- complete document removal, citation detachment, graph rebuild, and new identity
  on later reingestion;
- exact UTF-8 citation slicing;
- whole-request knowledge rollback on validation or graph-build failure;
- whole-request withdrawal rollback;
- add-only alias and identifier behavior;
- projection of only schema terms used by active knowledge;
- duplicate successful knowledge writes receiving distinct item IDs;
- explicit rejection of an incompatible store/search format;
- graph-version mismatch refusal;
- native rejection of graph mutations; and
- actual cancellation of an over-budget graph query.

Acceptance starts from an empty v2 database and original source/knowledge inputs.
No acceptance case may depend on opening, migrating, importing, or comparing
against an earlier implementation's database or API.

## 16. Bounded implementation choices

The architecture is settled. These concrete values are selected and verified
during implementation:

1. Exact pinned embedding model, vector extension, vector representation, fusion
   constants, and reranker after the packaging spike.
2. Exact JSON formats for `DocumentInput`, schema input, record input, and command
   results.
3. Small, concrete limits for source size, batch size, result rows, and query
    duration.

## 17. Decision record

Settled decisions for this revision:

- V2 is a greenfield implementation with no backward compatibility.
- V2 accepts only a fresh v2 database and v2 input contracts.
- Earlier databases, IDs, APIs, CLI forms, result shapes, and internal modules are
  not supported or preserved.
- During early development, incompatible v2 storage changes use a fresh database
  and reingestion rather than migrations.
- The system is a personal local knowledge base.
- Every abstraction and defensive path must justify its weight through a required
  capability, realistic correctness need, change locality, or fewer agent calls.
- SQLite is authoritative.
- Ladybug and graph querying are essential product capabilities.
- Ladybug is derived and rebuildable, but not treated as an optional bolt-on.
- Ladybug contains citation-ready passage and document fields so one Cypher query
  can return graph relationships with their evidence.
- Ladybug projects current active knowledge only and is completely rebuilt after
  each successful knowledge change.
- Ingestion is batch-first and has one internal pipeline.
- One batch-ingestion command loads the embedding model once and reuses it across
  all changed documents using bounded passage batches.
- Directory, file, stdin, JSONL, and connector inputs are adapters to the same
  normalized document stream.
- KG-owned IDs remain in SQLite.
- Source keys determine document identity.
- Changed content under the same source key creates a source revision.
- User-visible source text and metadata are revisioned together.
- Sources normally record immutable events; evolving proposals, decisions, and
  outcomes are represented by new or withdrawn knowledge facts.
- Source revision or removal does not implicitly withdraw authored knowledge.
- The synchronous command is rerun after failure; no durable processing
  orchestration is required initially.
- Hybrid lexical/vector/fusion/reranking search is essential.
- Dense retrieval uses one SQLite vector extension from the initial version.
- Passage and hybrid-index preparation exist only as stages of `kg ingest`; there
  is no separate public indexing API.
- One pinned local embedding model and reranker use the runtime's standard cache;
  there are no model profiles or approval workflows.
- Embedding and reranking providers load their weights lazily on first inference;
  commands that do not use a model do not initialize it.
- Search and graph query remain separate commands.
- Hybrid search supports only time and source-type filtering initially.
- The graph interface is read-only Cypher.
- User-defined schema is required and is stored as data in stable generic tables.
- Schema evolution is simple and additive.
- Schema terms are stored once with their introduced version; complete historical
  schema snapshots are not retained.
- Entities may have multiple simultaneous evidence-backed types.
- Entity types use a dedicated membership table with the same add/withdraw and
  optional-evidence lifecycle as other knowledge.
- Type memberships and facts share one `KnowledgeItemId` lifecycle and may have
  zero or more passage citations; aliases and identifiers are add-only metadata
  outside that lifecycle initially.
- Competing classification claims and selection machinery are excluded.
- Authored facts use one add/withdraw lifecycle.
- Knowledge authoring may create duplicate facts; deduplication is an explicit
  agent-and-user cleanup workflow rather than an ingestion subsystem.
- During ordinary execution, one submitted knowledge command and its graph build
  succeed together or roll back the attempted knowledge change. A rare crash in
  final activation is detected by `knowledge_version`; graph queries refuse a
  mismatch, and initial recovery is a fresh store rather than repair machinery.
- Authored entities and facts retain `created_at` plus optional informational
  `created_by`; attribution has no authorization semantics.
- First-class mention records are excluded initially.
- All source types use one deterministic paragraph-aware, nonoverlapping passage
  strategy; adapters do not supply passage boundaries initially.
- Entity reuse may resolve exact IDs, identifiers, or unambiguous names inside
  the authoring transaction.
- The CLI is designed around complete agent intents and minimizes chained calls.
- The CLI JSON contract is the only stable public interface; internal code APIs
  and the implementation language are not product contracts.
- Internal seams preserve change locality for likely substitutions without
  shipping alternate implementations, plugin frameworks, or compatibility code.
- Rust is the selected implementation language, contingent on a successful
  packaging spike proving Ladybug, SQLite vector search, local embedding,
  reranking, and a self-contained release with no installed interpreter or
  language runtime.
- Exact follow-up inspection uses one minimal typed `kg get ID` command.
- Complex JSON inputs expose generated descriptions from their actual validators;
  `kg record --describe` also includes the current user vocabulary.
