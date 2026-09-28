# Commonplace: SQLite Persistence Design

Status: Working design
Revision: 0.5
Companions: `design.md`, `class-architecture.md`, `capability-audit.md`

## 1. Goal

Define the complete SQLite schema for the lean v2 implementation and show how
each domain value is persisted, read, updated, withdrawn, and deleted.

The schema must support the approved first product:

- batch document ingestion with independent per-document publication;
- immutable document revisions and exact UTF-8 passage citations;
- lexical and vector search in SQLite;
- additive user-defined vocabulary;
- entities with multiple active types;
- entity and literal facts;
- optional passage evidence;
- explicit withdrawal of type memberships and facts;
- permanent one-document removal;
- complete Ladybug snapshot construction; and
- exact store-format and `knowledge_version` checks.

It must not preserve the current implementation's schemas, IDs, migrations,
receipts, jobs, processing state, authorization model, retained query state, or
compatibility paths.

## 2. Persistence principles

1. SQLite is the only authoritative durable store.
2. Tables correspond to durable domain distinctions, not classes or workflow
   stages.
3. Derived lexical/vector rows publish in the same transaction as their passage.
4. User vocabulary is data in stable generic tables, never generated DDL.
5. Type memberships and facts share one `KnowledgeItemId` lifecycle.
6. Aliases and identifiers are add-only entity metadata.
7. Explicit document removal deletes the source, revisions, passages, search
   rows, and evidence links; affected knowledge may become uncited.
8. Withdrawn knowledge remains readable in SQLite but is excluded from Ladybug.
9. Incompatible format changes require a fresh store; there are no migrations.
10. Foreign keys and uniqueness constraints enforce structural integrity. Domain
    rules that require several rows remain in transaction-level validation.

## 3. Persistence areas and final table inventory

The initial database has 15 ordinary tables and two virtual search tables:

| Area | Tables |
| --- | --- |
| Store | `store_state` |
| Documents and evidence | `documents`, `document_revisions`, `passages` |
| Search | `passage_fts`, `passage_vectors` |
| Vocabulary | `entity_types`, `identifier_schemes`, `predicates`, `predicate_entity_types` |
| Knowledge | `entities`, `entity_aliases`, `entity_identifiers`, `knowledge_items`, `entity_type_memberships`, `facts`, `knowledge_item_evidence` |

Ladybug files are outside this schema and contain no authoritative rows. There
are no tables for jobs, attempts, receipts, permissions, migrations, retained
queries, graph generations, readiness state, mentions, schema snapshots, or
search configurations.

## 4. Domain-object persistence map

| Domain value or operation | SQLite ownership |
| --- | --- |
| `DocumentInput` | resolves `documents.source_key`; creates or reuses a document and current revision |
| `DocumentRevision` | one immutable revision row containing exact text and user-visible metadata |
| `Passage` | one immutable passage row tied directly to one revision |
| lexical search document | one FTS row keyed to the passage |
| dense vector | one vector row keyed to the same passage |
| `SchemaDefinition` | additive vocabulary rows and one monotonic schema version |
| `Entity` | one stable entity row |
| alias | add-only row tied to one entity |
| identifier | add-only `(scheme, value)` row tied to one entity |
| `KnowledgeItem` | shared authored metadata and optional withdrawal state |
| type membership | typed row referencing one knowledge item, entity, and entity type |
| entity or literal fact | one fact row with exactly one entity object or canonical JSON literal |
| `EvidenceRef` | join row from one knowledge item to one passage |
| `kg remove` | deletes one document tree and dependent evidence links, then rebuilds Ladybug |

Application structs are not serialized wholesale. Repositories map explicit
columns to domain values and never expose raw SQLite rows outside `storage/`.

## 5. SQL schema

All ordinary tables are SQLite `STRICT` tables. Foreign keys are enabled on every
connection. Schema term names are validated in Rust as lowercase ASCII
identifiers; entity names, aliases, identifiers, source keys, and source text are
case-sensitive exact strings.

### 5.1 Store state

```sql
CREATE TABLE store_state (
    singleton         INTEGER PRIMARY KEY CHECK (singleton = 1),
    format            TEXT NOT NULL CHECK (format = 'kg-v2/1'),
    schema_version    INTEGER NOT NULL DEFAULT 0 CHECK (schema_version >= 0),
    knowledge_version INTEGER NOT NULL DEFAULT 0 CHECK (knowledge_version >= 0),
    created_at        TEXT NOT NULL
) STRICT;
```

`SqliteDatabase` accepts only an empty target or this exact format. Passage
generation, embedding identity and dimensions, vector representation, lexical
configuration, and ordinary-table DDL are all part of `kg-v2/1`. An incompatible
change creates a new format and requires a fresh store.

### 5.2 Documents and evidence

```sql
CREATE TABLE documents (
    document_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    source_key       TEXT NOT NULL UNIQUE CHECK (length(source_key) > 0),
    created_at       TEXT NOT NULL,
    last_ingested_at TEXT NOT NULL
) STRICT;

CREATE TABLE document_revisions (
    revision_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id      INTEGER NOT NULL
                     REFERENCES documents(document_id) ON DELETE CASCADE,
    revision_number  INTEGER NOT NULL CHECK (revision_number > 0),
    revision_digest  TEXT NOT NULL CHECK (length(revision_digest) = 64),
    text             TEXT NOT NULL,
    title            TEXT,
    source_type      TEXT NOT NULL CHECK (length(source_type) > 0),
    occurred_at      TEXT,
    metadata_json    TEXT NOT NULL CHECK (
                         json_valid(metadata_json)
                         AND json_type(metadata_json) = 'object'
                     ),
    created_at       TEXT NOT NULL,
    UNIQUE (document_id, revision_number)
) STRICT;

CREATE TABLE passages (
    passage_id   INTEGER PRIMARY KEY AUTOINCREMENT,
    revision_id  INTEGER NOT NULL
                 REFERENCES document_revisions(revision_id) ON DELETE CASCADE,
    ordinal      INTEGER NOT NULL CHECK (ordinal >= 0),
    start_byte   INTEGER NOT NULL CHECK (start_byte >= 0),
    end_byte     INTEGER NOT NULL CHECK (end_byte >= start_byte),
    text         TEXT NOT NULL,
    UNIQUE (revision_id, ordinal),
    UNIQUE (revision_id, start_byte, end_byte)
) STRICT;
```

Text is valid UTF-8 stored as SQLite `TEXT` without newline or Unicode
normalization. Passage ranges are zero-based, half-open UTF-8 byte offsets and
must select exactly `passages.text` from the revision. `metadata_json` is one
canonical compact JSON object produced by Rust with recursively sorted object
keys; the database does not normalize it into key/value rows.

Revision content is immutable. The current revision is the row with the greatest
`revision_number` for its document. Returning to earlier content creates a new
revision number; there is no current pointer, mutable flag, or restore operation.
`revision_digest` hashes exact text plus canonical user-visible metadata and is
used only to compare a submitted document with its current revision.

### 5.3 Search

The logical FTS contract is:

```sql
CREATE VIRTUAL TABLE passage_fts USING fts5(
    text,
    content = 'passages',
    content_rowid = 'passage_id',
    tokenize = 'unicode61'
);
```

The vector table has the same logical key:

```sql
CREATE VIRTUAL TABLE passage_vectors USING vec0(
    passage_id INTEGER PRIMARY KEY,
    embedding  FLOAT[EMBEDDING_DIMENSIONS]
);
```

The packaging spike must verify and then substitute the exact selected
vector-extension DDL and Rust binding syntax. The contract is fixed: one vector
per searchable passage, keyed directly by `passage_id`.

Only passages for each document's current revision have FTS and vector rows.
Historical passages remain in `passages` for citation reads but are not search
candidates. `SearchIndexStore` explicitly inserts and deletes both virtual-table
rows in the document publication transaction. Do not add triggers, mapping
tables, projection IDs, readiness rows, or background reconciliation.

### 5.4 User vocabulary

```sql
CREATE TABLE entity_types (
    entity_type_id     INTEGER PRIMARY KEY AUTOINCREMENT,
    name               TEXT NOT NULL UNIQUE CHECK (length(name) > 0),
    description        TEXT,
    introduced_version INTEGER NOT NULL CHECK (introduced_version > 0)
) STRICT;

CREATE TABLE identifier_schemes (
    identifier_scheme_id INTEGER PRIMARY KEY AUTOINCREMENT,
    name                 TEXT NOT NULL UNIQUE CHECK (length(name) > 0),
    description          TEXT,
    introduced_version   INTEGER NOT NULL CHECK (introduced_version > 0)
) STRICT;

CREATE TABLE predicates (
    predicate_id       INTEGER PRIMARY KEY AUTOINCREMENT,
    name               TEXT NOT NULL UNIQUE CHECK (length(name) > 0),
    object_kind        TEXT NOT NULL CHECK (
                           object_kind IN (
                               'entity', 'string', 'integer',
                               'boolean', 'timestamp'
                           )
                       ),
    description        TEXT,
    introduced_version INTEGER NOT NULL CHECK (introduced_version > 0)
) STRICT;

CREATE TABLE predicate_entity_types (
    predicate_id   INTEGER NOT NULL
                   REFERENCES predicates(predicate_id) ON DELETE RESTRICT,
    role           TEXT NOT NULL CHECK (role IN ('subject', 'object')),
    entity_type_id INTEGER NOT NULL
                   REFERENCES entity_types(entity_type_id) ON DELETE RESTRICT,
    PRIMARY KEY (predicate_id, role, entity_type_id)
) STRICT;
```

One role-tagged endpoint table replaces separate subject/object tables.
`domain.schema` enforces the cross-row rules:

- every predicate has at least one subject type;
- an entity-valued predicate has at least one object type;
- a literal-valued predicate has no object-type rows; and
- an existing term cannot be redefined.

Schema application increments `store_state.schema_version` once only when at
least one new term is inserted. Every new term in that command receives the same
introduced version. A no-op additive apply leaves the version unchanged.

### 5.5 Entities and identity metadata

```sql
CREATE TABLE entities (
    entity_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    canonical_name TEXT NOT NULL CHECK (length(canonical_name) > 0),
    created_at     TEXT NOT NULL,
    created_by     TEXT
) STRICT;

CREATE TABLE entity_aliases (
    entity_id  INTEGER NOT NULL
               REFERENCES entities(entity_id) ON DELETE RESTRICT,
    alias      TEXT NOT NULL CHECK (length(alias) > 0),
    created_at TEXT NOT NULL,
    created_by TEXT,
    PRIMARY KEY (entity_id, alias)
) STRICT;

CREATE TABLE entity_identifiers (
    entity_id            INTEGER NOT NULL
                         REFERENCES entities(entity_id) ON DELETE RESTRICT,
    identifier_scheme_id INTEGER NOT NULL
                         REFERENCES identifier_schemes(identifier_scheme_id)
                         ON DELETE RESTRICT,
    value                TEXT NOT NULL CHECK (length(value) > 0),
    created_at           TEXT NOT NULL,
    created_by           TEXT,
    PRIMARY KEY (identifier_scheme_id, value)
) STRICT;
```

Canonical names and aliases may match multiple entities; exact name/alias
resolution then reports ambiguity. An identifier value identifies at most one
entity within its scheme. Aliases and identifiers use these natural keys rather
than public surrogate IDs. Entities, aliases, and identifiers are not deleted or
withdrawn in v2.

### 5.6 Authored knowledge

```sql
CREATE TABLE knowledge_items (
    knowledge_item_id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind              TEXT NOT NULL CHECK (
                          kind IN ('type_membership', 'fact')
                      ),
    schema_version    INTEGER NOT NULL CHECK (schema_version > 0),
    created_at        TEXT NOT NULL,
    created_by        TEXT,
    withdrawn_at      TEXT,
    withdrawn_by      TEXT,
    CHECK (withdrawn_at IS NOT NULL OR withdrawn_by IS NULL)
) STRICT;

CREATE TABLE entity_type_memberships (
    knowledge_item_id INTEGER PRIMARY KEY
                      REFERENCES knowledge_items(knowledge_item_id)
                      ON DELETE CASCADE,
    entity_id         INTEGER NOT NULL
                      REFERENCES entities(entity_id) ON DELETE RESTRICT,
    entity_type_id    INTEGER NOT NULL
                      REFERENCES entity_types(entity_type_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE facts (
    knowledge_item_id INTEGER PRIMARY KEY
                      REFERENCES knowledge_items(knowledge_item_id)
                      ON DELETE CASCADE,
    subject_entity_id INTEGER NOT NULL
                      REFERENCES entities(entity_id) ON DELETE RESTRICT,
    predicate_id      INTEGER NOT NULL
                      REFERENCES predicates(predicate_id) ON DELETE RESTRICT,
    object_entity_id  INTEGER
                      REFERENCES entities(entity_id) ON DELETE RESTRICT,
    literal_json      TEXT CHECK (
                          literal_json IS NULL OR json_valid(literal_json)
                      ),
    CHECK (
        (object_entity_id IS NOT NULL AND literal_json IS NULL)
        OR
        (object_entity_id IS NULL AND literal_json IS NOT NULL)
    )
) STRICT;

CREATE TABLE knowledge_item_evidence (
    knowledge_item_id INTEGER NOT NULL
                      REFERENCES knowledge_items(knowledge_item_id)
                      ON DELETE CASCADE,
    passage_id        INTEGER NOT NULL
                      REFERENCES passages(passage_id) ON DELETE CASCADE,
    PRIMARY KEY (knowledge_item_id, passage_id)
) STRICT;
```

One `facts` table represents both entity and literal objects. `object_kind` on
the referenced predicate determines which fact column is valid and how
`literal_json` is decoded:

- string: JSON string;
- integer: JSON integer within the supported signed range;
- boolean: JSON `true` or `false`;
- timestamp: normalized UTC RFC 3339 JSON string.

The application validates predicate kind, endpoint types, literal type, and the
one-to-one match between `knowledge_items.kind` and its detail table before
insertion. Reads and graph snapshot construction fail closed if a knowledge item
has zero detail rows, appears in both detail tables, or disagrees with `kind`.
These cross-table domain rules do not justify triggers.

Withdrawal sets `withdrawn_at` and optional `withdrawn_by` once on the shared
knowledge-item row. There is no withdrawal table because restore and repeated
lifecycle events are not supported. Typed detail and evidence rows remain
unchanged and readable.

No semantic uniqueness constraint exists on memberships or facts. Repeating a
successful authoring request creates distinct knowledge-item IDs as explicitly
required by the non-idempotent write contract.

## 6. Transaction ownership

| Workflow | SQLite boundary |
| --- | --- |
| initialize/open | create or verify the exact complete format |
| ingest batch | one write transaction per document |
| remove document | one write transaction held through candidate graph construction |
| schema apply | one transaction for the complete schema input |
| record knowledge | one transaction for the complete record input, held through candidate graph construction |
| withdraw knowledge | one transaction for the complete ID set, held through candidate graph construction |
| hybrid search | one read transaction for candidates and evidence hydration |
| graph snapshot | read through the caller's open knowledge/removal transaction |
| exact get | one read transaction |

No repository method begins or commits a hidden transaction.

## 7. Deletion and foreign-key direction

The intended deletion root is:

```text
documents
  -> document_revisions
      -> passages
          -> passage_fts         explicit delete
          -> passage_vectors     explicit delete
          -> knowledge_item_evidence
```

Virtual tables do not participate in foreign keys, so `remove_document` first
collects passage and affected knowledge-item IDs, explicitly deletes FTS/vector
rows, and then deletes `documents`; ordinary foreign-key cascades remove
revisions, passages, and evidence joins. The workflow increments
`knowledge_version`, builds the candidate graph through the still-open
transaction, and uses the standard graph activation/SQLite commit window.

Document removal must not delete `knowledge_items`, memberships, or facts.
Deleting evidence joins may leave an active or withdrawn item with zero
citations.

Vocabulary and entity rows are not deleted in v2. Knowledge withdrawal changes
only the shared knowledge-item lifecycle; it does not delete the typed membership
or fact row.

## 8. Repository-to-table ownership

| Repository/module | Tables |
| --- | --- |
| `SqliteDatabase` | `store_state`, initialization, format verification, transaction creation |
| `DocumentStore` / document queries | `documents`, `document_revisions`, `passages` |
| `SearchIndexStore` / search queries | `passage_fts`, `passage_vectors`, joins to current passages |
| `EvidenceQueries` | revisions, passages, and `knowledge_item_evidence` |
| `KnowledgeStore` / knowledge queries | vocabulary, entities, identity metadata, knowledge items, memberships, facts, withdrawal columns |
| `build_graph_snapshot` | read-only joins across active knowledge and its remaining evidence |

These are cohesive SQL modules, not one repository class per table.

## 9. Timestamps

Persist instants as normalized UTC RFC 3339 `TEXT`. Rust parses and normalizes
caller-supplied timestamps before binding. Repository insert/update statements
obtain `created_at`, `withdrawn_at`, and `last_ingested_at` from SQLite using one
shared expression:

```sql
strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
```

This is human-inspectable, lexically ordered, and sufficient for the personal
MVP. Do not add clock-watermark, trusted-time, timezone-preservation, or
nanosecond machinery.

## 10. Required indexes

Primary keys and `UNIQUE` constraints create the basic indexes. Add only:

```sql
CREATE INDEX entity_name_idx
    ON entities(canonical_name, entity_id);

CREATE INDEX entity_alias_value_idx
    ON entity_aliases(alias, entity_id);

CREATE INDEX entity_identifier_entity_idx
    ON entity_identifiers(entity_id, identifier_scheme_id, value);

CREATE INDEX knowledge_item_active_idx
    ON knowledge_items(knowledge_item_id)
    WHERE withdrawn_at IS NULL;

CREATE INDEX membership_entity_idx
    ON entity_type_memberships(entity_id, entity_type_id, knowledge_item_id);

CREATE INDEX membership_type_idx
    ON entity_type_memberships(entity_type_id, entity_id, knowledge_item_id);

CREATE INDEX fact_subject_idx
    ON facts(subject_entity_id, predicate_id, knowledge_item_id);

CREATE INDEX fact_object_idx
    ON facts(object_entity_id, predicate_id, knowledge_item_id)
    WHERE object_entity_id IS NOT NULL;

CREATE INDEX evidence_passage_idx
    ON knowledge_item_evidence(passage_id, knowledge_item_id);
```

Do not add indexes for speculative analytics. Add an index only after an approved
workflow query or measured plan needs it.

## 11. Workflow persistence

### 11.1 Ingest one document

1. Read the current revision by descending `revision_number`.
2. If its digest and exact values match, update only `last_ingested_at` and return
   `unchanged`.
3. Outside the write transaction, generate passage drafts and embeddings.
4. Open the write transaction and recheck the current revision.
5. Delete FTS/vector rows for the previous current revision.
6. Insert the new revision and passages, obtaining passage IDs.
7. Insert FTS/vector rows keyed by those IDs.
8. Commit and return `added` or `updated`.

A failed item rolls back only its document transaction. The batch continues.

### 11.2 Remove one document

1. Resolve the source key or return `not_found`.
2. Collect all passage IDs and affected knowledge-item IDs.
3. Delete their FTS and vector rows explicitly.
4. Delete the document; foreign keys cascade through revisions, passages, and
   evidence links.
5. Increment `knowledge_version`.
6. Build and activate the candidate graph through the ordinary finalization
   window.
7. Commit and return removed-row counts plus affected item IDs.

### 11.3 Apply schema

1. Load all referenced existing terms.
2. Validate the complete additive definition.
3. If it adds nothing, return `unchanged`.
4. Increment `schema_version` once.
5. Insert every new term and endpoint row with that version.
6. Commit without rebuilding Ladybug.

### 11.4 Record knowledge

1. Validate and resolve the complete request.
2. Insert or reuse declared entities and add identity metadata.
3. For each membership or fact, insert `knowledge_items`, its one typed detail
   row, and zero or more evidence rows.
4. Increment `knowledge_version` once for the request.
5. Build the candidate graph from active knowledge in the open transaction.
6. Activate and commit through the standard finalization window.

### 11.5 Withdraw knowledge

1. Resolve the complete submitted `KnowledgeItemId` set.
2. Reject unknown or already withdrawn knowledge-item IDs before mutation;
   withdrawal accepts no other ID type.
3. Validate the complete resulting active state. Reject the request if a fact
   that would remain active loses every permitted type on a required endpoint,
   unless that fact is also included in the withdrawal.
4. Set withdrawal columns for every item.
5. Increment `knowledge_version` once.
6. Build the candidate graph and finalize as one whole request.

### 11.6 Search and exact reads

Hybrid search opens one read transaction, retrieves lexical and vector candidate
passage IDs, fuses/reranks them, and hydrates passages and revisions before the
transaction closes. Because only current passages exist in the search virtual
tables, candidate queries need no readiness or current-revision state joins.

`kg get` reads documents, revisions, passages, entities, or knowledge items
through direct indexed queries. A knowledge-item read joins its kind-specific
detail and evidence rows and returns withdrawal state.

## 12. Rust object mapping

Use small row types only where decoding is reused:

```text
DocumentId(i64)            <-> documents.document_id
RevisionId(i64)            <-> document_revisions.revision_id
PassageId(i64)             <-> passages.passage_id
EntityTypeId(i64)          <-> entity_types.entity_type_id
IdentifierSchemeId(i64)    <-> identifier_schemes.identifier_scheme_id
PredicateId(i64)           <-> predicates.predicate_id
EntityId(i64)              <-> entities.entity_id
KnowledgeItemId(i64)       <-> knowledge_items.knowledge_item_id
```

Repository decoding constructs domain values explicitly:

- `DocumentRecord` joins a document with its highest revision when current state
  is needed;
- `Passage` joins its revision only when document metadata is required;
- `PredicateDefinition` loads one predicate plus its role-tagged endpoint rows;
- `KnowledgeItem` loads the shared lifecycle row plus exactly one membership or
  fact detail;
- `EvidenceRef` is only a typed `PassageId`; and
- `GraphSnapshot` is produced by dedicated read queries over active items and
  their remaining evidence.

Do not serialize Rust structs into opaque blobs, add an ORM, generate one model
class per table, or create generic repository traits.

## 13. Enforced invariants

SQLite enforces:

- referential integrity and document-deletion cascades;
- nonempty keys and names;
- enum domains;
- one entity or literal object per fact;
- unique term names and identifiers;
- unique passage ordinal/range within a revision;
- unique evidence links; and
- one typed detail row per knowledge-item ID at the table-key level.

Rust transaction validation enforces:

- exact UTF-8 passage slicing;
- canonical JSON and timestamp representations;
- the highest revision is the only searchable revision;
- predicate endpoint and literal-kind compatibility;
- complete predicate endpoint definitions;
- every knowledge item has exactly one detail row, never both, and its `kind`
  matches that row;
- evidence passages exist before authoring;
- aliases/identifiers are add-only;
- withdrawal inputs are all valid and active before mutation; and
- candidate graph construction uses the transaction's exact
  `knowledge_version`.

Do not add triggers solely to duplicate these workflow validations.

Knowledge-item reads and graph snapshot construction repeat the detail-cardinality
and kind checks and fail closed on corruption rather than silently omitting or
duplicating rows.

## 14. Deterministic graph snapshot reads

Build the graph snapshot with separate bounded queries for:

1. active type memberships ordered by `knowledge_item_id`;
2. active facts ordered by `knowledge_item_id`;
3. distinct entities referenced by those rows, ordered by `entity_id`;
4. distinct used entity types ordered by `entity_type_id`;
5. distinct supporting passages ordered by `passage_id`; and
6. distinct source documents for those passages ordered by `document_id`.

Evidence is loaded separately as `(knowledge_item_id, passage_id)` ordered by
both IDs. Do not construct one wide membership/fact/evidence join whose
one-to-many rows duplicate semantic edges. Snapshot construction validates
exactly one detail row per active knowledge item before returning.

## 15. Canonical serialization and revision digest

Metadata and literal JSON support only null, booleans, signed 64-bit integers,
strings, arrays, and objects recursively composed from those values. Floating
point numbers are rejected.

Rust canonicalization converts objects to key order defined by Rust string
ordering and serializes compact JSON with the pinned `serde_json` version.
That encoder behavior is part of the store format. Golden tests cover escaping,
Unicode keys, nested objects, arrays, integer limits, and rejection of floats.

`revision_digest` is lowercase SHA-256 over this length-delimited preimage:

```text
"kg-v2-revision\0"
length(text UTF-8)          || text UTF-8
presence + length(title)    || title UTF-8 when present
length(source_type)         || source_type UTF-8
presence + length(time)     || normalized occurred_at when present
length(metadata JSON)       || canonical metadata JSON UTF-8
```

Every length is the UTF-8 byte length of the immediately following field and is
encoded as an unsigned 64-bit big-endian value. Presence is one byte, `0` or `1`.
This prevents field-boundary ambiguity without serializing the entire source into
another wrapper format.

## 16. Graph/SQLite finalization

For removal, authoring, and withdrawal:

1. keep the SQLite write transaction open;
2. build and verify a candidate graph;
3. activate the candidate while retaining the previous graph;
4. commit SQLite;
5. if commit returns an error, restore the previous graph and report failure;
6. after a successful commit, discard the previous graph.

A process crash during steps 3-4 can still leave a version mismatch. Graph reads
compare `knowledge_version` and fail closed; fresh-store reingestion is the
initial crash recovery. The temporary previous graph exists only for ordinary
commit-error compensation and is not a retained generation system.

## 17. Settled schema decisions

### 17.1 Primary keys

Every publicly addressable durable domain row uses an independent
`INTEGER PRIMARY KEY AUTOINCREMENT`. Alias, identifier, endpoint, and evidence
join rows use natural composite keys. Rust exposes distinct newtypes over `i64`;
the CLI renders tagged IDs such as `doc:42`, `rev:81`, `passage:306`,
`entity:17`, and `knowledge:94`.

`AUTOINCREMENT` prevents explicit deletion followed by insertion from reusing a
previous public ID. IDs are local to one store and are not designed for database
merge, offline generation, or cross-store identity.

FTS and vector rows use the passage integer ID directly, avoiding a separate
mapping table.

### 17.2 Text and JSON

Exact valid UTF-8 uses SQLite `TEXT`. Revision metadata and literal fact values
use the pinned canonical compact JSON representation defined above. No metadata
key/value tables are created.

### 17.3 Current revision

The greatest document-local `revision_number` is current. Revision rows are
immutable; there is no current pointer or flag.

### 17.4 Search identity

FTS and vector rows use `passage_id` directly and are maintained explicitly by
`SearchIndexStore` in the document transaction. There are no triggers or mapping
tables.

### 17.5 Predicate endpoints

One `predicate_entity_types` table distinguishes subject and object rows with a
checked role value.

### 17.6 Predicate direction

All v2 predicates are directed. Symmetric predicates and automatic reverse-edge
projection are omitted.

### 17.7 Fact representation

One `facts` table stores exactly one `object_entity_id` or canonical
`literal_json`.

### 17.8 Withdrawal

Nullable withdrawal columns live on `knowledge_items`. There is no withdrawal
event table or restore lifecycle.

### 17.9 Time

All instants are normalized UTC RFC 3339 text. SQLite generates operational
timestamps.

### 17.10 Indexes and constraints

Only indexes required by identity resolution, graph snapshot construction,
evidence deletion, and approved query paths are created. Cross-row semantic
rules remain explicit transaction validation.

## 18. Remaining implementation gate

The packaging spike must confirm:

- the exact `sqlite-vec` or selected extension DDL;
- vector dimension declaration and value binding from Rust;
- integer `passage_id` lookup;
- transactional insert/delete behavior alongside ordinary tables and FTS5; and
- release loading on the clean target machine.

Failure reopens the vector-extension or language choice. It does not justify a
second vector backend or compatibility layer.

## 19. Approval and review

This document is not yet approved. After the schema decisions are complete, it
must receive an independent read-only review together with the approved product
and class-architecture documents. Implementation must use the reviewed schema
revision rather than deriving a different schema ad hoc.
