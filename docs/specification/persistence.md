# Commonplace persistence design

## 1. Purpose

SQLite stores all authoritative Commonplace data. This document defines the
durable model, transaction ownership, deletion behavior, and invariants required
by the product.

Oxigraph directories are derived artifacts. They contain no unique authored fact and
never override SQLite.

## 2. Storage principles

1. Ordinary tables use SQLite `STRICT` mode.
2. Foreign keys are enabled on every connection.
3. Public durable objects use independent integer primary keys.
4. Revision and passage content is immutable.
5. FTS and vector rows publish in the same transaction as their passages.
6. User vocabulary is represented as rows in generic tables.
7. Type memberships and facts share one knowledge-item lifecycle.
8. Alias and identifier corrections are explicit and transactional.
9. Withdrawn knowledge remains in SQLite and is excluded from Oxigraph.
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
    format            TEXT NOT NULL CHECK (format = 'commonplace-store/3'),
    schema_version    INTEGER NOT NULL DEFAULT 0 CHECK (schema_version >= 0),
    knowledge_version INTEGER NOT NULL DEFAULT 0 CHECK (knowledge_version >= 0),
    created_at        TEXT NOT NULL
) STRICT;
```

Exactly one row exists. `format` identifies the complete durable representation,
including ordinary DDL, passage generation, lexical configuration, embedding
identity and dimensions, vector encoding, and the derived graph representation.

`commonplace-store/3` requires explicit temporal state in revisions, evidence and
RDF, with the section 14 digest encoding. Store/config markers change together.
All version-2 stores, including empty stores and stores with only non-null dates,
are incompatible and rejected before mutation. Use a fresh directory and explicit
caller-controlled reingestion. No migration, relabelling, old-null inference,
deletion, or automatic graph repair/conversion is provided. Reingestion does not
transfer prior citation IDs or authored knowledge. The version-1 Grafeo layout
also remains incompatible; /3 retains /2's Oxigraph layout.

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
    temporal_state   TEXT NOT NULL CHECK (temporal_state IN ('dated', 'timeless', 'unknown')),
    occurred_at      TEXT,
    metadata_json    TEXT NOT NULL CHECK (
                         json_valid(metadata_json)
                         AND json_type(metadata_json) = 'object'
                     ),
    created_at       TEXT NOT NULL,
    UNIQUE (document_id, revision_number),
    CHECK ((temporal_state = 'dated' AND occurred_at IS NOT NULL)
        OR (temporal_state IN ('timeless', 'unknown') AND occurred_at IS NULL))
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

### First-ingestion representation

P3 originally completed source ingestion under /2; the explicit temporal-intent
follow-on adopts /3 without a migration or second representation path.
Temporal state is immutable revision metadata and participates in exact matching
and digests. A temporal-only change creates a new revision of the same document;
historical evidence retains its cited revision's state and time.

The fixed passage maximum is **1024 UTF-8 bytes, including overlap**. Paragraphs
end after a blank LF or CRLF line (a line containing only spaces/tabs is blank);
all delimiter bytes remain in the preceding paragraph. The final unterminated portion is also a
paragraph. Consecutive paragraph units are greedily accumulated while their
combined length is at most the maximum. Before an oversized paragraph, flush any
pending whole paragraphs. Split the oversized paragraph into approximately
balanced chunks with **target overlap: 102 bytes (10% of maximum passage size)**,
the floor of 1024/10, not 10% of each smaller balanced chunk. Only chunks within
that paragraph overlap; ordinary packing and boundaries between paragraph units
remain nonoverlapping.

Compute a conservative UTF-8-feasible chunk count while reserving target
context: start at the paragraph end, subtract at most 1024 bytes and round
forward to a character boundary to obtain the earliest last-chunk start.
For each preceding suffix, subtract 1024 bytes from the prior suffix start plus
102 target-overlap bytes, again rounding forward, until the paragraph start is
reached. Retain these suffix starts to bound forward cuts so the remaining
chunks always fit. This count is not a guarantee of mathematically minimal
chunk count after whitespace/UTF-8 adjustments.

For each nonfinal chunk, the target length is the remaining source byte length
plus 102 bytes per remaining boundary, divided by the remaining chunk count,
rounded up. Among feasible character endpoints
within 1024 bytes of the start, prefer the endpoint immediately after whitespace
closest to the target, only if its distance is at most one quarter of the target
length (rounded down). Otherwise choose the closest feasible UTF-8 endpoint.
Break equal-distance ties toward the earlier endpoint. The next chunk's start
targets the emitted endpoint minus 102 bytes. Prefer the closest after-whitespace
boundary within 25 bytes of that target; otherwise choose the closest character
boundary, again breaking ties earlier. Constrain this start by the remaining
suffix capacity and strictly forward start progress. Both endpoints advance
strictly; emit the final remainder. Whitespace and UTF-8 adjustments can change
the achieved overlap.

Thus a whitespace-free 1100-byte ASCII paragraph produces `[0,601)` and
`[499,1100)`: 601/601 bytes, with 102 repeated source bytes, rather than a tiny
tail. Every passage is an exact contiguous source slice at most 1024 bytes
including overlap. Their union covers every byte with no gaps or empty passages;
legitimate short sources are not rejected and empty text has zero passages.
Publication validates first start zero, strictly increasing starts and ends,
no gaps, UTF-8 slices, final passage lengths and complete union coverage; the
generator owns where overlap is introduced. Ordinals start at zero.
Golden tests pin these boundaries and the existing section 14 digest encoding.

The approved balanced, target-overlapping boundary-generator replacement
originally shipped for newly prepared revisions under `/2`; `/3` continues that
generator. Within a compatible store, existing persisted passages
remain authoritative, including historical rigid or balanced nonoverlapping
boundaries. An unchanged digest does not cause re-preparation, reindexing, or a
new revision.
Identity and digest rules are unchanged; no migration or automatic rebuild occurs.

Embedding uses the P2 selection:
`Qdrant/all-MiniLM-L6-v2-onnx@8f518e882455312b086101e60691f5e6e2f05c3c`,
FastEmbed 7.1.0, mean pooling, token limit 256, finite unit-normalized 384-element
vectors. SQLite vector values are little-endian IEEE-754 float32 blobs;
sqlite-vec 0.1.6 and FTS5 `unicode61` remain unchanged. Runtime and tokenizer
dependency pins live in Cargo manifests/lockfiles; immutable model-artifact
checksums are enforced by the production provider. Changing these representation
choices requires the explicit format treatment in section 15.

## 6. Search

The lexical index is a contentless FTS5 table:

```sql
CREATE VIRTUAL TABLE passage_fts USING fts5(
    text,
    content = '',
    contentless_delete = 1,
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

Only passages from the current revision of each document are indexed. Search
returns `passage_id` values and always hydrates text and metadata from the
authoritative `passages` table. Publishing a replacement revision explicitly
deletes the previous revision's FTS and vector rows before inserting the new
rows.

The implementation verifies:

- the FTS and vector row IDs equal the current searchable passage IDs;
- no index row references a missing passage;
- vector dimensions match the store format; and
- vector query results hydrate through the same passage ID.

Search checks both index ID sets against canonical current passages in its one
read snapshot, including empty indexes, and rejects incompatible vector
declarations or missing/extra index rows without repair. Both candidate paths
apply current-revision, source-type, and optional passage-only literal phrase
predicates before their limits. With
`--since`, their identical temporal predicate admits timeless revisions OR dated
revisions at/after the inclusive normalized cutoff, excluding unknown and older
dated revisions. Canonical UTC text comparisons retain nanosecond precision
without SQLite date rounding. No ingestion timestamp is a substitute.
One aggregate query over current revisions after source-type filtering counts
sources in all four coverage categories in the same read snapshot, independent
of passages, overlap, query, literal phrase constraint, candidates and results,
including empty text sources.
Dense candidates use the stock sqlite-vec `vec_distance_L2` function
inside SQLite over eligible `vec0` rows, with distance/passage-ID ordering.
This exhaustive native SQL evaluation permits deterministic cutoff ties and
prefiltering without an application-level vector scan or index-layout change.
Work scales with the eligible corpus; only bounded candidates leave SQLite.
The retrieval constants in implementation section 4 are unchanged by /3.
The phrase constraint uses one shared deterministic SQLite scalar function on
canonical `passages.text`, with Rust Unicode whole-string lowercase substring
semantics as specified in product section 5, not SQLite's ASCII-only `lower` or
the contentless FTS text column. No durable table or index changes are required.

Explicit document scope is an operation-local optional JSON array of numeric
document IDs in the shared SQL predicate, intersected before lexical/vector limits.
Null means absent; empty array matches nothing. The inclusive upper event bound
uses the same canonical UTC text comparison and timeless/unknown semantics as
since. Current-source temporal aggregation applies scope/type first and counts
eligible/timeless/older/newer/unknown buckets independently of phrase and query.
Scope inspection reports missing selected IDs and counts filtered current sources
and eligible passages in the same snapshot. These read/output additions require
no durable change: store/config /3 remains the one supported format.

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
whether or not active knowledge currently references them. Any effective
vocabulary change, including adding a predicate endpoint, increments
`schema_version` once. New terms record that version. A no-op application
returns `unchanged` and leaves the version unchanged.

### 7.1 Permanent schema freeze marker

An initialized store may contain this optional final file:

```text
schema-freeze.json
```

Its exact UTF-8 bytes are compact JSON followed by LF:

```json
{"format":"commonplace-schema-freeze/1","schema_version":N}
```

`N` is a nonnegative signed-64-bit integer equal to authoritative SQLite
`store_state.schema_version`. Unknown fields, malformed JSON or UTF-8, an unknown
format, a negative/out-of-range version, a non-regular final path, or a version
different from SQLite is invalid freeze state. Commonplace never modifies,
removes, replaces, or bypasses a final marker.

Marker absence means an existing compatible version-3 store is unfrozen. This
optional capability requires no SQLite/configuration migration or store-format
change. The marker duplicates no vocabulary and records no knowledge version or
timestamp.

Creation uses the single store-root scratch path
`.schema-freeze.pending`. While holding the writer lock, freeze removes only
that named non-authoritative scratch path, creates it with create-new semantics,
writes and synchronizes the complete bytes, renames it atomically to the final
path, and synchronizes the store directory. A retry never promotes existing
pending bytes. Any pending path makes schema write state uncertain and blocks
every non-check schema application until `schema freeze` completes or confirms
the permanent final state. Freeze removes a file, symlink, or empty directory at
the reserved pending path without following symlinks. A nonempty directory or
other removal failure is explicit and must be cleared before retry; it never
causes Commonplace to alter the final marker.

A valid final marker at the current SQLite version blocks effective vocabulary
changes but permits a non-check no-op. Invalid final state blocks every non-check
schema application, including a no-op. `schema show` and check-mode application
remain SQLite-authoritative reads and do not require a valid marker.

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

Metadata corrections may delete aliases and identifiers through
`commonplace record`.
The request must identify the owning entity by canonical ID. Deletion is
transactional, does not delete the entity, and is not retained as knowledge
history.

Read-only entity discovery uses these authoritative metadata tables and active
knowledge memberships, not RDF. Canonical-ID keyset pagination and existing exact
name/alias/scheme/value resolution require no metadata rewrite, normalization,
schema term creation, new index or projection extension.

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
increments `schema_version` once when any term or endpoint row is added. A no-op
returns `unchanged`. Non-check application performs the section 7.1 freeze-state
check under the same process writer lock before mutation. Freeze itself reads
the current version without changing SQLite and publishes only the durable
store-local marker.

### Knowledge authoring and withdrawal

One transaction owns all entity, metadata, knowledge, evidence, withdrawal, and
`knowledge_version` changes for the request. A graph snapshot is read from that
transaction before commit.

### Removal

One transaction deletes the document tree and evidence links, identifies
affected knowledge, increments `knowledge_version`, and produces the candidate
graph snapshot.

Hybrid search and exact `commonplace get` operations each use one SQLite read
transaction.

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
- referenced entities and active type memberships;
- referenced predicates and entity types;
- evidence passages and their source revisions and documents; and
- schema terms used by active knowledge.

Snapshot construction uses separate bounded queries for:

1. active memberships ordered by `knowledge_item_id`;
2. active facts ordered by `knowledge_item_id`;
3. referenced entities ordered by `entity_id`;
4. referenced entity types ordered by `entity_type_id`;
5. referenced predicates ordered by `predicate_id`;
6. supporting passages ordered by `passage_id`;
7. their source revisions ordered by `revision_id`;
8. source documents ordered by `document_id`; and
9. evidence pairs ordered by `knowledge_item_id` and `passage_id`.

Separate reads prevent one-to-many evidence joins from duplicating semantic
items. Snapshot construction validates exactly one subtype row per active
knowledge item before returning. The graph stores the same
`knowledge_version` in its metadata.

### 13.1 Canonical RDF projection

The projection uses ordinary RDF 1.1. Knowledge and evidence triples are in the
default graph. The application property prefix `c:` expands to
`urn:commonplace:property:`; `xsd:` expands to
`http://www.w3.org/2001/XMLSchema#`. All application IRIs are reserved, not
user-selected namespaces.

Canonical resources use the following reversible mapping, with `N` the decimal
SQLite primary key without leading zeros:

| SQLite record | Resource IRI | Canonical tagged ID |
| --- | --- | --- |
| Document | `urn:commonplace:doc:N` | `doc:N` |
| Document revision | `urn:commonplace:revision:N` | `revision:N` |
| Passage | `urn:commonplace:passage:N` | `passage:N` |
| Entity | `urn:commonplace:entity:N` | `entity:N` |
| Knowledge item | `urn:commonplace:knowledge:N` | `knowledge:N` |
| Entity type | `urn:commonplace:type:N` | `entity-type:N` |
| Predicate | `urn:commonplace:predicate:N` | `predicate:N` |

Each resource also has `c:id` with its canonical tagged ID as an `xsd:string`;
the IRI prefix does not create a second ID allocation mechanism. Document,
revision, passage, entity, and knowledge IDs resolve through `get`. Vocabulary
IDs resolve through `schema show`. No canonical resource uses a blank node.

The table below defines the other properties. Property names expand through
`c:`. A link targets the corresponding resource above. Unless stated otherwise,
properties are single-valued and required.

| Resource | Properties and source |
| --- | --- |
| Entity | `name`: canonical entity name |
| Entity type | `name`: vocabulary name |
| Predicate | `name`: vocabulary name; `object_kind`: vocabulary object kind |
| Knowledge item, both kinds | `kind`: `type_membership` or `fact`; `schema_version`: version recorded on the item; `subject`: entity link; `evidence`: zero or more supporting passage links |
| Type membership | `entity_type`: type link |
| Fact | `predicate`: predicate link; `object`: entity link or typed literal |
| Literal fact only | `literal_kind`: predicate object kind; `literal_json`: canonical SQLite literal JSON as a string |
| Passage | `revision`: revision link; `ordinal`, `start_byte`, `end_byte`, `text`: exact passage fields |
| Revision | `document`: document link; `revision_number`, `revision_digest`, `source_type`, `temporal_state`, `metadata_json`: exact revision fields; optional `title` and dated `occurred_at` |
| Document | `source_key`: canonical source key |

Text, names, IDs, kinds, digests, and canonical JSON fields use `xsd:string`.
Versions, ordinals, revision numbers, and byte offsets use `xsd:integer`.
Optional SQL NULL fields emit no triple, not an empty string. Source metadata
belongs to the cited revision, never implicitly to the latest revision.
The graph carries passage text, not a second copy of full revision text; `get`
remains authoritative for that full text.

Fact object conversion is exact: JSON string to `xsd:string`, signed i64 to
`xsd:integer`, boolean to `xsd:boolean`, and normalized UTC timestamp to
`xsd:dateTime`. Projected `occurred_at` also uses `xsd:dateTime`. Integer lexical
forms are decimal, boolean forms are `true`/`false`, and timestamps reuse the
canonical normalization in section 14. No conversion through floating point is
permitted. `literal_json` preserves the canonical authored JSON independently
of the engine's term representation. Arbitrary query-computed literals use the
product's RDF-term result contract, not the canonical SQLite JSON encoder.

A membership or fact is represented by its distinct knowledge subject and the
properties above, not by a bare `(entity, predicate, object)` triple or RDF 1.2
triple term. Do not add parallel shortcut relationships or synthetic value nodes.
Duplicate facts remain distinct resources; multiple evidence links do not
duplicate the item. Zero-evidence active items remain projected.

Only active items, their referenced entities/types/predicates, and their cited
passages/revisions/documents are emitted. An item keeps its recorded
`schema_version`; it is not rewritten to the latest vocabulary version.
Vocabulary descriptions, endpoint constraints, aliases, and identifiers remain
authoritative SQLite data outside this projection.

### 13.2 Native version metadata

The only application named graph is `urn:commonplace:metadata`. It contains:

```turtle
<urn:commonplace:store> <urn:commonplace:property:knowledge_version>
    "0"^^<http://www.w3.org/2001/XMLSchema#integer> .
```

Replace `0` with the SQLite snapshot's `knowledge_version`. Verification requires
exactly one value at this subject/predicate, typed as `xsd:integer`, within the
nonnegative signed-i64 range, and equal to SQLite. Missing, multiple, malformed,
wrong-datatype, negative, and unequal values are errors. An empty knowledge
projection still contains this metadata triple. Metadata is query-visible but
never mixed into the default graph or stored in an independent sidecar.

### 13.3 Layout and compatibility

The initialized configuration is:

```json
{
  "format": "commonplace-config/3",
  "database": "commonplace.sqlite3",
  "graph": "graph/current"
}
```

Graph paths relative to the store are fixed:

| Path | Lifetime |
| --- | --- |
| `graph/current/` | Complete current native Oxigraph/RocksDB database |
| `graph/candidate/` | Temporary complete replacement built from one SQLite snapshot |
| `graph/previous/` | Temporary pre-activation database retained through SQLite commit |
| `graph/publication.lock` | Stable application lock outside all renamed directories |

The store root may also contain `schema-freeze.json` and the transient
`.schema-freeze.pending` path defined in section 7.1. They are independent of
the derived graph layout and neither is opened by graph workflows.

`init` creates an empty default graph and version-zero metadata with matching
SQLite/configuration formats. Its JSON response field names remain unchanged;
its format value is `commonplace-store/3`. Version-1/2 configuration, the old
`graph/current.grafeo` layout, and unknown markers are rejected before mutation,
including by `init` and `graph rebuild`. No compatibility backend or migration
is provided. Further incompatible representation decisions still require a
new explicit format decision.

Normal reads never create a missing current database or select `previous` as a
fallback. Missing/corrupt current state or invalid metadata yields
`graph_unavailable` with explicit rebuild guidance for an otherwise compatible
store. SQLite-only operations need not open the graph.

Temporary paths are not retained generations. A normal graph publisher encountering
leftover candidate/previous state refuses to overwrite it and directs the caller
to explicit rebuild. After validating the store format and taking the writer
and exclusive publication locks, explicit rebuild may remove these two known
derived scratch paths and reconstruct from committed SQLite. It never chooses
a generation by directory name or metadata, scans for arbitrary orphans, or
deletes authoritative state. It preserves any pre-operation current directory
until a replacement is verified, then uses the activation/restore sequence in
[architecture section 7.2](architecture.md#72-knowledge-changes).
Rebuild also works when no current directory exists. Cleanup and restoration
failures are reported explicitly; they are not successful repairs.

### 13.4 User defaults are not store configuration

The optional user-level default file is a separate durable public format:

```json
{
  "format": "commonplace-user-config/1",
  "store": "/absolute/path/to/persistent/store",
  "model_cache": "/absolute/path/to/installed/pinned-models"
}
```

`format` is required. `store` and `model_cache` are independently optional
UTF-8 strings and, when present, must be absolute paths on the executing
platform. Unknown and duplicate fields, malformed JSON, unsupported formats,
and relative paths invalidate the complete file. The application does not
rewrite, migrate, repair, or preserve unknown data in this format.

This file is discovered outside every knowledge base and contains no
authoritative source, knowledge, graph, credential, profile, or provider state.
It must never be confused with `<store>/config.json`, whose fixed
`commonplace-config/3` contents describe the backend layout and are created and
validated only with that store. Selecting a user default does not initialize,
open, validate, migrate, or mutate the referenced store or model cache.

Commonplace is read-only with respect to `commonplace-user-config/1` in this
release, so it has no application write or atomic-replacement protocol. Package
and user instructions may show deliberate creation or replacement by the user,
but installation and runtime commands do not modify the real user home
implicitly.

## 14. Time and serialization

Operational timestamps are UTC RFC 3339 text. The workflow obtains one
timestamp immediately before opening its write transaction and binds that value
to every row created or changed by the operation.

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
"commonplace-revision/2\0"
length(text UTF-8)          || text UTF-8
presence + length(title)    || title UTF-8 when present
length(source_type)         || source_type UTF-8
length(temporal_state)      || temporal_state UTF-8
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

The explicitly approved balanced, target-overlapping passage preparation in
section 5 is a narrow exception: only newly created revisions use the new
deterministic boundaries;
persisted historical passages and unchanged-input revision identities remain
untouched. It requires neither a new format nor reingestion of existing sources.
