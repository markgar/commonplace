# Commonplace persistence design

## 1. Purpose

SQLite stores all authoritative Commonplace data. This document defines the
durable model, transaction ownership, deletion behavior, and invariants required
by the product.

Ladybug files are derived artifacts. They contain no unique authored fact and
never override SQLite.

## 2. Storage principles

1. Ordinary tables use SQLite `STRICT` mode.
2. Foreign keys are enabled on every connection.
3. Public durable objects use independent integer primary keys.
4. Revision and passage content is immutable.
5. FTS and vector rows publish in the same transaction as their passages.
6. User vocabulary is represented as rows in generic tables.
7. Type memberships and facts share one knowledge-item lifecycle.
8. Aliases and identifiers are add-only metadata.
9. Withdrawn knowledge remains in SQLite and is excluded from Ladybug.
10. Incompatible representation changes require a fresh store.

## 3. Table inventory

The database contains 15 ordinary tables and two virtual search tables:

| Area | Tables |
| --- | --- |
| Store | `store_state` |
| Sources | `documents`, `document_revisions`, `passages` |
| Search | `passage_fts`, `passage_vectors` |
| Vocabulary | `entity_types`, `identifier_schemes`, `predicates`, `predicate_entity_types` |
| Entities | `entities`, `entity_aliases`, `entity_identifiers` |
| Knowledge | `knowledge_items`, `entity_type_memberships`, `facts`, `knowledge_item_evidence` |

## 4. Store state

```sql
CREATE TABLE store_state (
    singleton         INTEGER PRIMARY KEY CHECK (singleton = 1),
    format            TEXT NOT NULL CHECK (format = 'kg-v2/1'),
    schema_version    INTEGER NOT NULL DEFAULT 0 CHECK (schema_version >= 0),
    knowledge_version INTEGER NOT NULL DEFAULT 0 CHECK (knowledge_version >= 0),
    created_at        TEXT NOT NULL
) STRICT;
```

Exactly one row exists. `format` identifies the complete durable representation,
including ordinary DDL, passage generation, lexical configuration, embedding
identity and dimensions, and vector encoding.

`schema_version` changes when vocabulary changes. `knowledge_version` changes
when active graph content or evidence changes.

## 5. Sources and evidence

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

The current revision is the greatest `revision_number` for a document. No
mutable current flag is stored.

`metadata_json` is canonical compact JSON with recursively sorted object keys.
`revision_digest` is SHA-256 over an unambiguous encoding of exact text and
canonical user-visible metadata.

Passage offsets are zero-based, half-open UTF-8 byte offsets into revision text.
Application validation confirms that offsets are character boundaries and select
exactly the stored passage text.

## 6. Search

The lexical index is an external-content FTS5 table:

```sql
CREATE VIRTUAL TABLE passage_fts USING fts5(
    text,
    content = 'passages',
    content_rowid = 'passage_id',
    tokenize = 'unicode61'
);
```

The vector table uses the selected SQLite vector extension:

```sql
CREATE VIRTUAL TABLE passage_vectors USING vec0(
    passage_id INTEGER PRIMARY KEY,
    embedding  FLOAT[EMBEDDING_DIMENSIONS]
);
```

Both virtual tables use `passage_id` directly. There are no triggers or mapping
tables. `SearchIndexStore` explicitly inserts and deletes index rows inside the
document transaction.

Only passages from the current revision of each document appear in the virtual
tables. Publishing a replacement revision explicitly deletes the previous
revision's FTS and vector rows before inserting the new rows.

The implementation verifies:

- one FTS row and one vector row for every searchable passage;
- no index row references a missing passage;
- vector dimensions match the store format; and
- vector query results hydrate through the same passage ID.

## 7. User vocabulary

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
    predicate_id  INTEGER NOT NULL
                  REFERENCES predicates(predicate_id) ON DELETE RESTRICT,
    role          TEXT NOT NULL CHECK (role IN ('subject', 'object')),
    entity_type_id INTEGER NOT NULL
                   REFERENCES entity_types(entity_type_id) ON DELETE RESTRICT,
    PRIMARY KEY (predicate_id, role, entity_type_id)
) STRICT;
```

Application validation restricts vocabulary names to lowercase ASCII
identifiers. Entity-valued predicates have subject and object endpoint rows.
Literal-valued predicates have subject endpoint rows and encode their literal
kind in `object_kind`.

Schema application is additive. Existing terms cannot be redefined or removed,
whether or not active knowledge currently references them. A change that inserts
at least one term increments `schema_version` once and assigns that version to
every new term. A no-op application returns `unchanged` and leaves the version
unchanged.

## 8. Entities and identity

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
    created_at            TEXT NOT NULL,
    created_by           TEXT,
    PRIMARY KEY (identifier_scheme_id, value)
) STRICT;
```

Aliases and identifiers are case-sensitive exact strings. Identifiers uniquely
resolve an entity within a scheme. Name and alias lookups may return multiple
candidates and therefore cannot silently resolve ambiguity.

## 9. Authored knowledge

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

Transaction validation enforces:

- `knowledge_items.kind` matches its one subtype row;
- endpoint entity types satisfy the predicate schema;
- literal JSON is a string, integer, boolean, or normalized timestamp matching
  the predicate's `object_kind`;
- active type requirements consider writes in the same request;
- every evidence passage exists;
- supplied quote and offset checks match canonical source text; and
- a withdrawal applies only to an active item; and
- a withdrawal cannot leave an active fact without a permitted active type on a
  required endpoint unless that fact is withdrawn in the same request.

Knowledge-item reads and graph snapshot construction repeat the detail-cardinality
and kind checks. They fail closed if an item has no subtype row, has both subtype
rows, or disagrees with its declared kind.

Knowledge items may have zero evidence rows. Duplicate facts are valid and
receive distinct knowledge IDs.

## 10. Indexes

Indexes support required identity, history, evidence, and graph-read paths:

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

Additional indexes require an observed query need.

## 11. Transaction ownership

### Document publication

One transaction owns a changed document revision, its passages, FTS rows, and
vectors. Publication is all-or-nothing for that document.

The workflow:

1. reads the current revision by descending revision number;
2. updates only `last_ingested_at` and returns `unchanged` when exact values
   match;
3. computes passages and embeddings before opening the write transaction;
4. opens the transaction and rechecks the current revision;
5. deletes search rows for the prior current revision;
6. inserts the new revision, passages, FTS rows, and vectors; and
7. commits and returns `added` or `updated`.

### Schema application

One transaction validates and inserts all requested vocabulary changes, then
increments `schema_version` once when rows are added. A no-op returns
`unchanged`.

### Knowledge authoring and withdrawal

One transaction owns all entity, metadata, knowledge, evidence, withdrawal, and
`knowledge_version` changes for the request. A graph snapshot is read from that
transaction before commit.

### Removal

One transaction deletes the document tree and evidence links, identifies
affected knowledge, increments `knowledge_version`, and produces the candidate
graph snapshot.

Hybrid search and exact `kg get` operations each use one SQLite read transaction.

## 12. Deletion behavior

Virtual search tables do not participate in foreign keys. Document removal first
collects passage and affected knowledge-item IDs, explicitly deletes FTS and
vector rows, and then deletes the document. Foreign keys cascade through
revisions, passages, and evidence links. Removal does not delete or withdraw
knowledge items.

Entities and vocabulary are not implicitly garbage-collected. Their identities
remain stable even when no active knowledge references them.

Knowledge withdrawal updates lifecycle columns and retains subtype and evidence
rows. Permanent knowledge deletion is not a public operation.

## 13. Graph snapshot reads

A graph snapshot is a deterministic read of:

- the current `knowledge_version`;
- active knowledge items and subtype rows;
- referenced entities, active type memberships, aliases, and identifiers;
- referenced predicates and entity types;
- evidence passages and their source revisions and documents; and
- schema terms used by active knowledge.

Snapshot construction uses separate bounded queries for:

1. active memberships ordered by `knowledge_item_id`;
2. active facts ordered by `knowledge_item_id`;
3. referenced entities ordered by `entity_id`;
4. referenced entity types ordered by `entity_type_id`;
5. supporting passages ordered by `passage_id`;
6. source documents ordered by `document_id`; and
7. evidence pairs ordered by `knowledge_item_id` and `passage_id`.

Separate reads prevent one-to-many evidence joins from duplicating semantic
edges. Snapshot construction validates exactly one subtype row per active
knowledge item before returning. The graph stores the same
`knowledge_version` in its metadata.

## 14. Time and serialization

Operational timestamps are UTC RFC 3339 text. SQLite supplies transaction
timestamps so rows created by one operation share a consistent time.

Repository statements use:

```sql
strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
```

Rust parses and normalizes caller-supplied timestamps before binding them.

The canonical JSON encoder supports only null, booleans, signed 64-bit integers,
strings, arrays, and objects recursively composed from those values.
Floating-point numbers are rejected. Revision metadata may use the complete
canonical JSON value set. A fact's `literal_json` is further restricted by its
predicate to a string, signed integer, boolean, or normalized timestamp string.

Canonicalization orders object keys using Rust string ordering and emits compact
JSON using the pinned `serde_json` version. That encoder behavior is part of the
store format. Golden tests cover escaping, Unicode keys, nested values, integer
limits, and float rejection.

`revision_digest` is lowercase SHA-256 over this length-delimited preimage:

```text
"kg-v2-revision\0"
length(text UTF-8)          || text UTF-8
presence + length(title)    || title UTF-8 when present
length(source_type)         || source_type UTF-8
presence + length(time)     || normalized occurred_at when present
length(metadata JSON)       || canonical metadata JSON UTF-8
```

Each length is the unsigned 64-bit big-endian UTF-8 byte length of the field that
follows. Presence is one byte, `0` or `1`.

## 15. Store compatibility

Opening a knowledge base verifies the exact store format and required SQLite
extensions. An incompatible format is rejected without mutation.

Representation changes that affect passage boundaries, embeddings, vector
dimensions, lexical configuration, or table layout require a new store format
and reingestion.
