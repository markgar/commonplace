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
4. query the authored graph with read-only SPARQL SELECT; and
5. resolve every cited result back to exact stored evidence.

Deterministic processing belongs in the executable. Semantic interpretation
belongs to the calling agent.

## 2. Product principles

### 2.1 Local and personal

A knowledge base is a local directory containing authoritative SQLite state and
a derived Oxigraph graph. Anyone who can open the directory and run the CLI has
full access.

There are no tenants, accounts, principals, grants, user-managed namespaces, or
network service endpoints. Reserved RDF namespaces identify application data,
not authorization boundaries.

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
| `temporal_state` | Required explicit `dated`, `timeless`, or `unknown` |
| `occurred_at` | RFC3339 event timestamp required only for `dated` |
| `metadata` | Small JSON object |

SQLite maps `source_key` to an internal document ID. Examples include an email
message ID, an editable message ID, a meeting-summary key, or a canonical file
URI.

### 3.2 Adapters

The CLI supports:

```sh
commonplace ingest document.md --temporal-state timeless
commonplace ingest document-a.md document-b.md --temporal-state unknown
commonplace ingest ./notes --recursive --temporal-state unknown
commonplace ingest --stdin --source-key teams/message/123 --temporal-state dated --occurred-at 2026-09-28T10:00:00Z
commonplace ingest --jsonl documents.jsonl
producer | commonplace ingest --jsonl -
```

Direct file and directory inputs accept Markdown and plain UTF-8 text. Their
default source key is a `file:` URI derived from the normalized absolute path,
so direct and directory ingestion of the same file use the same identity and
different scan roots cannot conflate unrelated files. Moving a file creates a
new source unless streamed input supplies a stable key.
Directory scans support `.md` and `.txt`, recurse only when requested, skip
symlinks by default, and preserve Markdown as text rather than extracting
domain-specific records. Repeated include and exclude globs may narrow a scan.
Every input path is subject to the configured maximum document size.

Inputs must decode as valid UTF-8. The system preserves valid text exactly,
including empty content, CRLF line endings, byte-order marks, NUL characters,
combining marks, and non-BMP characters. It rejects undecodable or binary input
rather than normalizing it.

For the P3 file forms, explicit symlinks are rejected and discovered symlinks are
skipped. Scan globs are case-sensitive and relative to each input directory:
`*` stays within a path component, `**` may span components, include patterns are
ORed, and excludes take precedence. Filters apply to directory scans, not explicit
files. Roots retain argument order; each root's bounded scan is sorted by path.
Exceeding the document-enumeration limit rejects the command before publication.
An input that disappears during reading fails as an item, never as an implicit
deletion.

Every input requires explicit temporal intent: `dated` means a known event time
and requires valid RFC3339 `occurred_at`; `timeless` means event time does not
apply; `unknown` means event time applies but is unavailable. The latter two
prohibit a timestamp. There is no omitted/null/default state or inference from
text, metadata, filenames, frontmatter, or ingestion time.

File defaults are title equal to the filename, source type `file`, and empty
metadata. Required `--temporal-state` applies to every document in a file/scan
command; mixed-state batches use separate commands or self-contained JSONL.
`--title`, `--source-type`, `--occurred-at`, and `--metadata`
override those fields for every document in that command. The metadata argument
is a JSON object using persistence's canonical value set; timestamps normalize
to UTC RFC 3339. P3 establishes these file overrides; P9 adds generic streamed
inputs.

Acquisition and format extraction belong entirely to the caller. Commonplace
accepts already-normalized text through stdin and JSON Lines, not email, web,
PDF, service, or provider-specific connectors. Manifest input is deferred:
saved JSON Lines already serves the current agent workflow without another
batch envelope or path-reference mapping format.

Exactly one input mode is selected. `--stdin` accepts one exact UTF-8 document
and requires `--source-key` and `--temporal-state`; metadata flags apply to that document.
JSON Lines accepts a filename or `-` for stdin. Each physical line is a
self-contained object with required string `source_key`, `text`, and `temporal_state`, optional
`title`, `source_type`, `occurred_at`, and `metadata`, and no unknown or duplicate
object fields. Dated records require a string `occurred_at`; timeless/unknown
records omit it. Explicit JSON null is not a timestamp and is rejected.
JSON Lines rejects command-level metadata overrides, including temporal state.
Streamed defaults are null title, source type `text`, and empty metadata. Source
keys are opaque, nonempty, NUL-free strings in the existing shared namespace;
they are not trimmed, case-folded, or derived from input locations.

Blank JSON Lines records are invalid; a final record without a newline is
accepted. LF/CRLF record delimiters are excluded from `--max-json-bytes`, which
bounds each encoded record as well as canonical metadata. Decoded text also
obeys `--max-source-bytes`. Oversized records stop immediately with a terminal
failed item, without draining remaining input. Each physical record consumes
one `--max-documents` slot; excess input yields one terminal failed item without
parsing or publishing another document. Prior publications remain successful.
Empty JSON Lines input returns an empty complete result.

### 3.3 Revisions

Submitting a new source key creates a document and its first immutable revision.
Submitting an existing key with identical text and user-visible metadata returns
`unchanged`. A change creates a new immutable revision.

The current revision is the greatest revision number for a document. All prior
revisions remain readable so existing citations remain resolvable.

The revision digest covers exact text and canonical user-visible metadata:
title, source type, temporal state, occurred-at time, and custom metadata.

### 3.4 Batch ingestion

One command accepts one or many documents. It:

1. validates command options before reading documents;
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

The ingestion result is `{summary, items}` inside the common envelope.
`summary` counts `added`, `updated`, `unchanged`, and `failed`. Each item has
`input`, `source_key`, `status`, `document_id`, `revision_id`, `passage_ids`, and
`error`. Successful items return canonical IDs and `error: null`; failed items
return null document/revision IDs, an empty passage-ID array, and
`error: {stage, code, message}`. An unresolved source key is null.
An attempted batch emits this result on stdout even when partial or all-failed.
Exit selection is deterministic: any runtime failure yields 1; otherwise any
conflict yields 3; otherwise failed items yield 2; otherwise 0. Command-level
validation errors use the common error envelope on stderr.

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

The argument is an exact opaque stored key, not a path to resolve. For a
file-ingested document, copy its canonical `file://` source key from `get`;
removal does not normalize keys or delete the original file.

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
3. split an oversized paragraph into balanced chunks with target overlap:
   102 bytes (10% of maximum passage size), preferring nearby whitespace
   boundaries and falling back to UTF-8 character boundaries, all within
   1024 bytes including overlap; and
4. assign stable ordinals within the revision.

Whole-paragraph packing remains nonoverlapping. Only adjacent chunks within an
oversized paragraph overlap; rounding and whitespace/UTF-8 adjustments can change
the achieved overlap. Their exact source slices cover the full paragraph with no
gaps, and both start and end offsets advance strictly. The algorithm reserves
target context to choose a bounded chunk count; it does not promise a
mathematically minimal count after boundary adjustments. Adapters cannot provide
custom passage boundaries.
The exact algorithm is owned by persistence section 5. Preparation applies only
to newly created revisions; unchanged inputs keep their persisted passages and
IDs, and historical citation boundaries are never rewritten.

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
- source title, type, temporal state, time, and metadata;
- exact byte offsets;
- score or rank information; and
- explicit truncation information.

Supported filters are optional `--since`/`--until`, `--scope FILE` (or `-` for
stdin), repeatable `--source-type`, and optional single `--must-contain '<phrase>'`. All
apply identically to lexical and vector candidate selection. Structured entity
or automatic graph-expansion filters are not part of hybrid search. Scope is an
explicit schema-neutral document allow-list, not entity interpretation.

`--must-contain` requires a literal contiguous substring of the **passage text**,
not the full source, title, or metadata. Both paths apply it in SQLite before
their candidate limits. Matching lowercases both whole strings using Rust
`str::to_lowercase` (Unicode lowercase) and compares the resulting substring;
it does not perform Unicode normalization or full case folding. For example,
`ÉCRAN` matches `écran`, but `Straße` does not match `STRASSE`, composed and
decomposed accents differ, and contextual Greek final sigma remains distinct
from ordinary sigma. Whitespace and punctuation are literal and retained,
including leading/trailing whitespace. There is no regex, FTS syntax, word
boundary, entity, or alias interpretation. The phrase must be nonblank, contain
no NUL, and fit 4096 original UTF-8 bytes and 64 whitespace-delimited terms.
Omitting the flag preserves ordinary hybrid retrieval.

Ordinary results include final rank and may include a lightweight final score.
They do not expose candidate-stage membership, model identities, fusion
contributions, or retained search explanations. A narrow debug response may be
added only when retrieval evaluation requires it.

The positional query is plain text, not raw FTS syntax. Whitespace-delimited
terms are individually quoted/escaped and ORed for lexical matching; both models
receive the original query. Blank/NUL queries are rejected. A query is bounded
to 4096 UTF-8 bytes and 64 terms. Repeatable source types are case-sensitive
exact alternatives, bounded to 32 supplied values and 4096 aggregate UTF-8 bytes;
empty/NUL values are rejected. `--since` is an inclusive RFC3339 instant normalized
to UTC without discarding fractional seconds. It admits current dated revisions
at/after that instant **plus timeless context**, and excludes older dated and
unknown sources. `--until` supplies an inclusive upper bound with the same
normalization and precision. Either bound includes timeless and excludes unknown;
equal bounds admit the exact instant, reversed windows fail `invalid_input`.
Both native candidate paths apply this predicate before their
limits; ingestion/creation time never substitutes for event time. Without
either date bound, all three states remain eligible.

`--limit` defaults to 10 and accepts 0 through 50. Search returns the common
envelope with operation `search`, status `complete`, and result
`{items, truncated, temporal_filter}`.
Each item has 1-based `rank` plus the same complete evidence fields as passage
`get`. Empty eligible results return `items: []`. Zero limit retains no items,
but still checks required models and indexes.

`temporal_filter` is null without either date bound. With bounds, the object contains
normalized nullable `since` and `until`, `includes_timeless: true`, `coverage`, and
`diagnostics`. Coverage counts current sources after scope and source-type filters,
including empty text, not historical
revisions, query matches, passages, candidates, or retained results:
`eligible_dated`, `timeless`, `older_dated`, `newer_dated`, and `excluded_unknown`. Counts use the
same SQLite read snapshot as search. Diagnostics contains `unknown_dates_excluded`
with an explicit coverage warning whenever excluded_unknown is nonzero, even
with nonempty results, empty results, or zero limit; otherwise it is empty.
Timeless inclusion is a filter semantic, not a claim that timeless sources match.
Coverage does not establish exhaustive relevance or certainty that no evidence exists.

#### Explicit document scope and grouped evidence

`graph query QUERY --document-scope COLUMN` takes a SELECT variable name without
`?` and returns `{document_ids, truncated}` inside the common envelope instead
of RDF rows. Every retained binding must be a canonical `urn:commonplace:doc:N`
IRI. Missing columns, unbound/non-document terms and noncanonical spellings fail
`invalid_input`; native graph evaluation/version/limit failures remain unchanged.
No query recipes or vocabulary-specific behavior are built into the bridge.

`search --scope FILE` reads precisely that result object (pipe `jq '.result'`
from graph output); `--scope -` reads stdin. `search --describe-scope` returns
the generated input schema and example without store/model access. Scope JSON
rejects unknown/duplicate fields, requires `document_ids` and boolean `truncated`,
and is bounded to 64 KiB and 1024 supplied IDs including duplicates. IDs use exact
canonical positive tagged spelling; malformed/overflow IDs fail `invalid_input`,
bounds fail `limit_exceeded`. Duplicate IDs are counted and deterministically
deduplicated. Well-formed missing/removed IDs are excluded and listed in diagnostics,
not substituted. Absent scope means all documents; explicit empty means none.
Scope intersects all existing filters before both candidate limits. Required
indexes/models are still validated even with empty scope, no results or zero limit.

Scoped results additionally contain `scope` with `supplied_ids`, `duplicate_ids`,
`selected_sources` (unique supplied IDs, including missing), `missing_document_ids`,
`existing_sources`, `excluded_source_type`, `eligible_sources`, `eligible_passages`,
`selection_truncated`, `lexical_truncated`, `vector_truncated`, `fusion_truncated`,
`result_truncated`, and diagnostics. Eligible sources count current sources after
scope/type/date; eligible passages additionally apply phrase. Retrieval-stage flags
refer to collection cutoffs, not selection completeness or text length.
Diagnostics always warn `selected_scope_only`; conditionally report
`selection_truncated`, `missing_documents`, `empty_scope`, `no_eligible_passages`
or `no_candidates_in_scope`. Zero retained-result limit does not imply no matches.
A partial graph scope is permitted but explicitly warned; scope never establishes
corpus-wide absence or exhaustive relevance.

`--grouped` returns `{groups,truncated,temporal_filter}` plus scope when supplied,
instead of passage items. Limit remains retained passage count (0..50). Groups
follow first retained rank; group passages preserve global ranks. Each group
shares canonical document/revision IDs, source key/title/type/state/time. Each
passage carries canonical passage ID, rank, original half-open byte span and full
exact text (already bounded to 1024 UTF-8 bytes). Compactness comes from shared
source fields and omitted metadata, not shortening relevant text. No source text
is reconstructed or stitched. Full metadata and historical revisions remain
accessible via get.
Grouping changes no ranking, collection truncation or coverage semantics.

`truncated` reports collection omissions: a candidate sentinel proves more
candidates, fusion exceeds the rerank bound, or final results exceed `--limit`.
It does not report tokenizer windows: the pinned embedding uses 256 tokens and
the reranker uses 512 tokens per query/passage pair. Model windows may shorten
inference inputs; returned citations always contain the exact full passage.
There is no relevance threshold or promise of exhaustive relevant results;
an unrelated query can return nearest passages. Missing/corrupt required models
fail even for an empty corpus or zero limit. Invalid values use `invalid_input`,
input bounds use `limit_exceeded`, and incompatible indexes use `conflict`,
following the common stderr error envelope and exit conventions.

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
commonplace schema freeze --json
```

Schema application changes SQLite only. It does not rebuild Oxigraph because the
graph projects only vocabulary referenced by active knowledge.

`schema freeze` permanently freezes the current vocabulary for that initialized
store. The first successful freeze and every repeat leave `schema_version` and
`knowledge_version` unchanged and do not open or modify Oxigraph. A valid frozen
store rejects every effective non-check schema application with `conflict` and
guidance to create a fresh store for a different vocabulary. A non-check no-op
remains `unchanged`; `schema show` and `schema apply --check` remain available.
Invalid or uncertain freeze-marker state rejects every non-check schema
application, including a no-op, until `schema freeze` can confirm the permanent
state.

Freeze is a one-way accidental-change guardrail enforced by Commonplace, not an
authorization boundary against a process that can edit the database or store
files directly. Commonplace provides no unfreeze, bypass, vocabulary removal,
replacement, or migration operation.

The physical SQLite schema and RDF mapping remain generic. User vocabulary is data,
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

Read-only `entity list` returns complete existing entity metadata together
(canonical ID/name, aliases, identifiers, active type/membership/fact IDs).
Optional `--type` filters an existing schema type by active membership;
`--missing-identifier` filters an existing identifier scheme. Unknown terms fail
`invalid_input`; untyped entities remain listed without type filtering.
Ascending canonical ID order and exclusive `--after entity:N` keyset paging are
deterministic. `--limit` defaults to 100, accepts 0..1000, and uses one sentinel
to report `{items,truncated,next_after}`. Cursor is last retained ID only when
truncated; zero has no cursor. Each item equals entity get metadata without `kind`.

`entity resolve` takes exactly one of `--name`, `--scheme` plus `--value`, or
`--id`, using the existing record selector validation/resolution. Exact opaque
case-sensitive names/aliases/identifier values are not normalized. Ambiguity
fails `conflict`; absent matches fail `not_found`. Result equals entity get
metadata without `kind`. Both commands are authoritative SQLite read-only
operations: no inference, graph access, metadata replay, or writes.

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

#### Record input and output

JSON input is `{"created_by":"manual","items":[...]}`; `created_by` may be omitted.
Entity, entity_metadata and type_membership items retain their existing forms.
A directed relationship and literal fact use:

```json
{"created_by":"manual","items":[
  {"kind":"entity","ref":"riley","name":"Riley"},
  {"kind":"entity","ref":"acme","name":"Acme"},
  {"kind":"type_membership","entity":{"ref":"riley"},"entity_type":"person"},
  {"kind":"type_membership","entity":{"ref":"acme"},"entity_type":"company"},
  {"kind":"fact","subject":{"ref":"riley"},"predicate":"works_at","object":{"entity":{"ref":"acme"}},"support":[{"passage_id":"passage:1"}]},
  {"kind":"fact","subject":{"ref":"riley"},"predicate":"decision","object":{"literal":"approved"},"support":[{"passage_id":"passage:1"}]}
]}
```

The example requires declared person/company types, works_at with those entity
endpoints, decision with person subjects and string objects, and an existing
passage:1. Fact subject and entity object accept exactly one selector: canonical
`id`, exact `identifier: {scheme,value}`, unambiguous `name`, or request-local
`ref`. Local refs match `[A-Za-z][A-Za-z0-9_]{0,63}`, name preceding entity
declarations, and last only for the request. Endpoint eligibility uses the
resulting active state: a required membership may occur after a fact. At least
one permitted active type must exist for each required endpoint.

Fact object is exactly `{"entity": SELECTOR}` or `{"literal": VALUE}`. The
predicate determines the literal kind. Strings (including empty strings),
signed-i64 integer tokens and booleans are accepted for matching predicates;
timestamp predicates require RFC3339 strings normalized to UTC. For example,
`2026-09-28T12:25:23.123-05:00` becomes `2026-09-28T17:25:23.123Z`.
Reject null, arrays, objects, out-of-i64 integers, and floating-point/exponent
tokens including `1.0` and `1e0`. No integer conversion through floating point.
Persistence section 14 owns canonical JSON and time encoding.

`record --jsonl FILE` accepts one complete JSON request envelope per physical
line, then concatenates all items into one request before opening a writer:

```jsonl
{"created_by":"manual","items":[{"kind":"entity","ref":"riley","name":"Riley"}]}
{"created_by":"manual","items":[{"kind":"type_membership","entity":{"ref":"riley"},"entity_type":"person"},{"kind":"fact","subject":{"ref":"riley"},"predicate":"decision","object":{"literal":"approved"}}]}
```

Every fragment must have the same decoded optional `created_by`, including
omission; there is no per-line creator or implicit inheritance. Existing creator
validation is unchanged: supplied labels contain 1-128 Unicode scalar values;
the CLI schema rejects explicit null. Refs may cross preceding fragments.
Accept LF/CRLF and an optional final newline. Reject empty files, blank lines,
BOMs, invalid UTF-8, multiple values per line, and unknown/duplicate fields.
`{"items":[]}` is an explicit no-op. Decode errors identify the physical line;
stateful item errors and successful result indices use flattened zero-based
positions. There are no per-line publications or partial-success results.
FILE, --jsonl FILE and --describe are mutually exclusive; record adds no stdin
mode.

Both inputs are bounded to 1 MiB aggregate UTF-8 bytes (including JSONL framing),
read with at most one overflow byte; 1000 aggregate items; and 1000 entries per
metadata/support array. These reuse the record limits, not ingestion's
per-document publication semantics. A late invalid item or fragment rejects
the entire request. Support asserts whole canonical passages: optional quote
equals the full text, optional start_byte/end_byte are paired and equal its
zero-based half-open UTF-8 revision range, and passage IDs cannot repeat.

Record keeps the common envelope with operation `record`, contract version `1`
and status `complete`. Result includes schema_version, knowledge_version,
summary (`items`, `entities_created`, `metadata_changed`, `memberships_created`,
`facts_created`) and one indexed item per input. A fact item includes
`kind: fact`, index, knowledge_id, subject_entity_id, predicate_id, object and
hydrated support. Entity object output is `{"entity_id":"entity:2"}`. Literal
object output includes its kind, typed JSON value and exact canonical JSON:
`{"literal_kind":"string","literal":"approved","literal_json":"\"approved\""}`.
Knowledge get keeps common provenance/withdrawal/support fields; fact subtype
adds `subtype: fact`, subject_entity_id, predicate_id and the same object.
Membership output is unchanged. Every knowledge result includes support, even [].

Any membership or fact creation increments knowledge_version once and drives
the same coordinated publication branch, including fact-only follow-up requests.
Metadata-only and empty requests do neither. Errors preserve the common stderr
envelope: invalid_input for validation/count/framing/creator failures,
limit_exceeded for total input bytes, not_found for missing records and conflict
for ambiguity or locks. For example, an invalid endpoint returns exit 2 with
`{"operation":"record","contract_version":"1","status":"failed","error":{"code":"invalid_input","message":"...items[5]...permitted active type...endpoint..."}}`
and no successful stdout result. Post-commit cleanup or response-delivery errors
retain the committed IDs/version and do-not-retry guidance; they are not rollback.

`record --describe` returns the generated execution input_schema, a valid
example, current vocabulary, JSONL framing/creator/limit metadata with valid
example fragments (using the same input_schema), and stateful/lexical validation
rules beyond JSON Schema value validation.

### 7.4 Withdrawal and history

`commonplace withdraw` marks one or more active knowledge items withdrawn and
republishes the graph. Withdrawn items remain readable from SQLite history but are
absent from the active graph.

Withdrawal is final. There is no restore lifecycle; a corrected assertion is a
new knowledge item.

A withdrawal request accepts only knowledge-item IDs and validates the complete
resulting active state before mutation. It is rejected if any fact that would
remain active loses every permitted type on a required subject or object
endpoint, unless that dependent fact is included in the same withdrawal request.
Any invalid or already withdrawn ID rejects the complete batch.

The JSON-file input is:

```sh
commonplace withdraw withdraw.json --json
commonplace withdraw --describe --json
```

```json
{"withdrawn_by":"manual","knowledge_ids":["knowledge:4","knowledge:5"]}
```

`knowledge_ids` contains 1-1000 unique canonical `knowledge:<positive i64>` IDs.
Empty batches and duplicate IDs are rejected, not no-ops or deduplicated writes.
The optional actor label contains 1-128 Unicode scalar values when supplied;
explicit null is rejected. Unknown and duplicate fields are rejected. Input is
bounded to 1 MiB; the writer/publication timeout is two seconds. There is no
stdin, JSONL, positional-ID alias or check mode.

Success has `operation: withdraw`, `contract_version: "1"`, `status: complete`,
and a result with `knowledge_version`, `summary: {"items":N,"withdrawn":N}` and
`items` in input order. Each item contains `index` and the complete retained
Knowledge fields: knowledge_id, schema_version, creation/withdrawal provenance,
subtype/detail and support (including `[]`). It has no outer `kind: knowledge`
tag. All items share one application timestamp and the version advances once.
Stored canonical literals and creation/schema provenance do not change.

Invalid shape, count, label, ID, duplicates, already-withdrawn items or invalid
remaining endpoints yield `invalid_input`; missing IDs yield `not_found`.
Byte-limit errors yield `limit_exceeded`, and lock contention uses `conflict`.
Every error rejects the complete batch unless explicitly identified as
post-commit. Committed cleanup (`post_commit_cleanup`) or response-delivery
(`internal_error`) errors retain the committed IDs/version and instruct callers
not to retry withdrawal, but to inspect those IDs with `get`. Only cleanup
failures additionally instruct `graph rebuild`. Uncertain delivery requires
inspection of the submitted IDs; there are no retained request IDs or replays.

`withdraw --describe` is exclusive with the input file and needs no store or
inference. It returns the generated execution `input_schema`, a validated
`example`, a typed complete-envelope `output_example`, and `validation` metadata
for stateful and lexical rules beyond JSON Schema.

## 8. Graph projection and queries

Stock Oxigraph is the required query engine and a derived store. Its ordinary
RDF 1.1 projection contains all active knowledge and enough source data to return
citation-ready graph results. No engine fork, alternate backend, RDF 1.2 triple
terms, or inference rules are required.

Every authored membership or fact has its own canonical knowledge-item IRI.
Memberships link that item to an entity and entity type; facts link it to a
subject, predicate, and entity or typed literal object. Evidence links the
knowledge item to passages, their exact revisions, and documents. Equal facts
remain distinct authored items even though RDF triples have set semantics.
Literal assertions do not need separate value identities. Aliases and identifiers
remain SQLite identity metadata and are not projected.

[Persistence section 13](persistence.md#13-graph-snapshot-reads) owns the exact
IRI, property, datatype, and source-field mapping. RDF IRIs reversibly identify
canonical SQLite records; they do not change the public IDs accepted by `get`.

Only entities participating in active knowledge are projected. Only passages
supporting active knowledge and their documents are projected. Uncited corpus
passages remain searchable in SQLite. Ordinary document ingestion does not
rebuild Oxigraph; source removal does because it can remove projected evidence.
Only entity types and predicates referenced by active knowledge are projected;
the complete vocabulary remains available through `commonplace schema show`.

Knowledge and evidence occupy the default graph. The reserved named graph
`urn:commonplace:metadata` stores the graph version, including for an empty
knowledge base. It is deliberately visible to explicit `GRAPH` queries and graph
discovery; it is neither hidden data nor a user vocabulary term.

Every knowledge-changing operation builds and verifies a complete candidate
graph before publishing it. Graph reads compare the graph's
`knowledge_version` with SQLite and fail closed on mismatch.

A crash in the narrow interval between graph activation and SQLite commit can
leave a version mismatch; a crash between directory renames can leave the current
graph absent. Missing, corrupt, or invalid-version graphs also fail closed.
`commonplace graph rebuild` acquires the writer and graph
publication locks, builds a complete candidate from committed SQLite state,
verifies it, and activates it without changing `knowledge_version`. Failure
preserves the pre-operation current graph, when present. Rebuild repairs derived
state only in a compatible store; it never converts an old store format.
There is no automatic repair, retained
graph-generation system, or general transaction coordinator.

`commonplace graph query` accepts SPARQL 1.1 SELECT queries through Oxigraph's
native query parser and a native read-only store. SPARQL Update and the ASK,
CONSTRUCT, and DESCRIBE query forms are rejected as `invalid_input` in the first
release. This is a product boundary, not an engine limitation. No mutation API is
exposed and read-only enforcement is not based on string filtering.

The query text is a positional argument. `--row-limit` defaults to 1000 and
permits zero: a zero limit retains no rows and consumes at most one sentinel
solution to distinguish empty from truncated. `--timeout-ms` defaults to 5000
and must be positive. These are operational options, not stored representation
fields; the initialized configuration retains the exact shape in persistence
section 13.3. Limits must be representable without arithmetic overflow and must
not cause eager result-capacity allocation.

Queries are local: HTTP support and remote service handlers are disabled.
`SERVICE` cannot retrieve remote data. `SERVICE SILENT` retains SPARQL's error
suppression semantics without enabling network access. Dataset clauses select
only locally stored graphs; they do not fetch IRIs.

An operation-scoped timer requests native cancellation when the configured
evaluation budget expires. Cancellation is cooperative, may overshoot, and is
not a hard wall-clock or memory limit. It covers execution and result iteration,
not a promise of preemptible parsing or every engine operator. A cancelled query
returns `limit_exceeded`, not partial success. Ctrl+C terminates the synchronous
CLI process rather than keeping it alive with a cancelled query.

The command consumes at most `row_limit + 1` solutions from the engine's lazy
iterator, retains at most `row_limit` output rows, and reports `truncated` when
the extra solution exists. This bounds final result retention, not internal
sorting/join/aggregate memory or individual value size. Queries are not rewritten
to append LIMIT. Without ORDER BY, row order is not guaranteed across runs.

The graph-query success payload (inside the common CLI success envelope) has
`kind`, `columns`, `rows`, and `truncated` fields. For example, an empty result
projecting `?knowledge` is
`{"kind":"select","columns":["knowledge"],"rows":[],"truncated":false}`.
Columns are variable names without `?`, in native projection order. Each row is an array
aligned with those columns. Unbound variables are JSON `null`; bound RDF terms
use these exact shapes:

| Term | JSON shape |
| --- | --- |
| IRI | `{"type":"uri","value":"urn:commonplace:knowledge:3"}` |
| Blank node | `{"type":"bnode","value":"label"}` |
| Literal | `{"type":"literal","value":"42","datatype":"http://www.w3.org/2001/XMLSchema#integer","language":null}` |

Literal lexical values remain strings, including integers, booleans, timestamps,
and query-computed datatypes. Language-tagged literals retain their language tag
and datatype; untagged literals have `language:null`. Blank-node labels are
result/store-scoped, not canonical Commonplace IDs. Unsupported native term
variants fail explicitly, never by stringification or shape heuristics.
SPARQL expression errors retain their native unbound semantics. In particular,
stock Oxigraph's bare minimum-i64 expression may be unbound; the explicit
`"-9223372036854775808"^^<http://www.w3.org/2001/XMLSchema#integer>` literal and
canonical stored integer preserve the exact value without query rewriting.

For example, an ordered query for an authored fact and its exact evidence is:

```sparql
PREFIX c: <urn:commonplace:property:>
SELECT ?knowledge ?passage ?revision ?document ?start ?end ?quote
WHERE {
  ?knowledge c:kind "fact"; c:predicate <urn:commonplace:predicate:1>;
             c:evidence ?passage .
  ?passage c:revision ?revision; c:start_byte ?start;
           c:end_byte ?end; c:text ?quote .
  ?revision c:document ?document .
}
ORDER BY ?knowledge ?passage
```

A zero-evidence item still exists in the graph; callers use OPTIONAL evidence
patterns when such items should be included. Joins may return multiple rows per
knowledge item without changing its identity.

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
commonplace config show
commonplace ingest
commonplace remove
commonplace search
commonplace schema show
commonplace schema apply
commonplace schema freeze
commonplace record
commonplace withdraw
commonplace get
commonplace entity list
commonplace entity resolve
commonplace graph schema
commonplace graph query
commonplace graph rebuild
```

`commonplace init` creates the SQLite database, Oxigraph storage location, and local
configuration for one knowledge base. It does not create model profiles,
approval state, cache profiles, or a separate model-preparation workflow.
Pinned models load on first use and may be obtained through the selected
runtime's standard cache mechanism.

Commonplace may read one optional user configuration file containing only
non-secret default paths for the knowledge base and strict offline model cache.
It is not the store-local backend configuration created by `init`. The effective
knowledge-base path is selected by `--store`, then `COMMONPLACE_STORE`, then the
user file, then `.commonplace`. The effective strict model-cache path is selected
by `--model-cache`, then `COMMONPLACE_MODEL_CACHE`, then the user file; without
one, the selected runtime's existing stock cache/acquisition behavior remains.

`commonplace config show` returns the discovered file status and each effective
value and source without opening a store, loading models, checking path health,
downloading artifacts, or writing configuration. A discovered malformed,
unreadable, unknown-field, relative-path, or unsupported-version file fails
every executing command explicitly. Standard `--help` and `--version` remain
available before configuration loading. Explicit overrides do not hide a broken
discovered file.

The `config.show` result contains `config_file.path` (null when platform
discovery is unavailable), `config_file.status` (`unavailable`, `absent`, or
`loaded`), and `store`/`model_cache` objects with `path` and `source`.
Value sources are `command_line`, `environment`, `user_config`, `default`, or
`runtime_default`; the last is used only for a null model-cache override, where
the existing model runtime selects its stock cache.

The user file is optional and has one standard platform location; there is no
`--config` location override, named profile, configuration writer, or implicit
store/model preparation. A selected missing store is created only by explicit
`init`; other commands retain their existing failure. A selected model cache is
strictly offline and never falls back or downloads when files are missing or
corrupt.

`commonplace get ID` is the complete direct record read command. It accepts tagged
document, revision, passage, entity, and knowledge-item IDs. Document results
include revision IDs; revision results include complete stored text and metadata;
passage results include their exact citation; entity results include active type
and fact IDs; and knowledge-item results include their subtype, evidence, and
withdrawal state. Withdrawn knowledge remains readable by ID. There is no
generic history mode, centered-context read, semantic revision comparison, or
separate history read command. Entity discovery/resolution are defined in section 7.1.

`commonplace get` returns the complete requested record, including all referenced IDs,
and does not silently truncate authoritative state. Existing source-size and
per-request write limits bound individual stored values; cumulative revision
and entity histories may grow with use.

`get` results have `kind: document | revision | passage | entity | knowledge`
plus the complete record fields. A document
includes all `revision_ids` in revision-number order and `current_revision_id`;
a revision includes all `passage_ids` in ordinal order. A passage includes source
key, revision number, ordinal, exact text and byte offsets, and the revision's
title/type/time/metadata. An entity includes aliases, identifiers, and active
type, membership, and fact IDs. A knowledge item includes its membership or fact
subtype, schema version, creation/withdrawal provenance, and exact support,
including an empty support array. Missing supported IDs yield `not_found`.

`commonplace schema show` returns the complete user vocabulary from SQLite.
`commonplace schema freeze` returns
`{"schema_version":N,"frozen":true}` in the common envelope. Its first success
has status `complete`; a repeated confirmed freeze has status `unchanged`.
If delivery is uncertain, callers rerun the same idempotent command. A retry
confirms the existing final marker and completes required directory durability
before returning unchanged.
`commonplace graph schema` returns the stable RDF mapping, reserved namespaces
and metadata graph, canonical ID mapping, property names, datatypes, evidence
fields, and example read-only SPARQL SELECT patterns. Standard
`--help`, these two schema commands, and generated input descriptions replace a
general `commonplace capabilities` command.

`graph schema` is a static description and does not open the store. It identifies
supported membership/fact projection and its evidence mapping. Rebuild rejects
invalid active state instead of silently publishing an incomplete graph.
The `graph rebuild` success payload is `{"knowledge_version":N}`, inside
the common envelope; `N` is the unchanged committed SQLite version.

Complex JSON boundaries describe themselves:

```sh
commonplace ingest --describe --json
commonplace schema apply --describe --json
commonplace record --describe --json
commonplace withdraw --describe --json
commonplace graph schema --json
```

Descriptions are generated from the validators used by execution and include an
input JSON Schema plus a minimal valid example. `ingest --describe` additionally
provides `record_schema` and `record_example` for self-contained JSON Lines
records, without changing other commands' description envelopes. A null command
`source_type` selects the input mode's default; record source types remain
non-null strings. Duplicate fields and canonical metadata/time/byte-limit rules
are also checked during execution. `commonplace record --describe` also
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
- schema unfreeze, bypass, removal, replacement, retirement, or migration;
- competing type claims or classification-selection workflows;
- record-supersession graphs;
- generated tables for user vocabulary;
- historical schema reconstruction;
- automatic semantic deduplication;
- source deletion inferred from synchronization;
- multiple selectable model profiles;
- configuration profiles, credentials, or executable-path settings;
- configuration migration or implicit configuration writes;
- background workers or job recovery;
- incremental graph projection;
- historical or withdrawn knowledge in Oxigraph;
- retained graph sessions or query handles;
- a general query-plan language, continuation tokens, or retained result sets;
- fixed relationship, decision, count, or proof-inspection query APIs;
- a general capability-discovery command;
- generalized diagnostics or resource accounting; or
- a stable library SDK, compatibility adapters, or migration machinery.
