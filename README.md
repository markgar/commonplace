# Commonplace

A local, evidence-backed knowledge system.

Commonplace is a lean personal knowledge tool for batch document ingestion,
hybrid SQLite search, explicitly authored cited knowledge, and local graph
queries.

The Rust implementation supports store initialization, additive vocabulary,
permanent store-local vocabulary freezing,
file and generic stdin/JSONL ingestion, hybrid search, atomic cited entity/type/fact
authoring and withdrawal, explicit source removal, authoritative reads, and
read-only RDF graph queries and rebuild:

```sh
cargo run --bin commonplace -- --store .commonplace init
```

The command creates the SQLite schema, FTS5 and sqlite-vec indexes, and the
derived stock Oxigraph 0.5.11/RocksDB database. New stores use
`commonplace-store/3` and `commonplace-config/3`. It is safe to rerun against a
compatible initialized store with a valid, version-matching graph. All older
stores, including /2, are rejected without modification, migration, relabeling,
or automatic repair. Use a fresh directory and explicitly reingest sources.
The old store remains intact but this binary cannot read it; source reingestion
does not transfer old citation IDs or authored knowledge.

Create `schema.json` with the vocabulary you want to add:

```json
{
  "entity_types": [{"name": "person"}, {"name": "company"}],
  "identifier_schemes": [{"name": "email"}],
  "predicates": [{
    "name": "works_at",
    "object_kind": "entity",
    "subject_types": ["person"],
    "object_types": ["company"]
  }]
}
```

```sh
cargo run --bin commonplace -- --store .commonplace schema apply schema.json --check --json
cargo run --bin commonplace -- --store .commonplace schema apply schema.json --json
cargo run --bin commonplace -- --store .commonplace schema show --json
cargo run --bin commonplace -- --store .commonplace schema freeze --json
cargo run --bin commonplace -- schema apply --describe --json
```

JSON is the default output; `--json` explicitly requests the same format. The
`init` response field names are unchanged; its format is `commonplace-store/3`.
Schema and graph operations use the same
`operation`, `contract_version`, `status`, and `result` envelope.

`schema apply` and `schema show` manage **your vocabulary**: entity types,
identifier schemes, and predicates. `graph schema` describes **the RDF mapping**:
how Commonplace represents authored knowledge and citations for SPARQL queries.

Input collections may be omitted. Names match `[a-z][a-z0-9_]*`; terms may include
an optional string `description`. Predicate `object_kind` is `entity`, `string`,
`integer`, `boolean`, or `timestamp`. Every predicate needs nonempty
`subject_types`; entity-valued predicates also need nonempty `object_types`.
Literal-valued predicates omit `object_types` or use an empty array. Endpoints
can reference types added in the same request. Duplicate declarations, unknown
fields, unknown endpoints, and changes to existing kinds or descriptions fail
atomically. Omitting an existing description preserves it. Omitting existing
terms or endpoints never removes them.

An effective application increments `schema_version` once, including endpoint-only
additions; a no-op returns `unchanged`. The apply result includes `changed`,
`checked`, current/resulting `schema_version`, `projected_schema_version`, and a
`summary` counting added entity types, identifier schemes, predicates, and
endpoint rows. `--check` reports status `checked`, preserves the current version,
and reports the prospective version separately. `schema show` returns the
complete vocabulary sorted by name, with tagged IDs, introduction versions,
descriptions, predicate kinds, and sorted endpoint names.

Schema input is limited to **1 MiB (1,048,576 bytes)**, read with a bound; larger
files return `limit_exceeded`. Generated descriptions contain Draft 2020-12 JSON
Schema and a valid example. The generated schema is also used to validate
execution input, without remote schema resolution. Stateful additive checks run
identically for `--check` and apply.

`schema freeze` permanently freezes the initialized store's current vocabulary.
Its first success returns status `complete`; repeats return `unchanged`. Both
return `{"schema_version":N,"frozen":true}` without changing
`schema_version`, `knowledge_version`, or any graph file. The durable marker is
the inspectable store-root file `schema-freeze.json`:

```json
{"format":"commonplace-schema-freeze/1","schema_version":1}
```

The actual file uses compact UTF-8 JSON followed by LF. A valid frozen store
rejects every effective non-check `schema apply` with `conflict`; a no-op remains
`unchanged`, and `schema show` plus `schema apply --check` remain available.
Malformed, inconsistent, or interrupted marker state fails all non-check schema
applications closed. Rerun `schema freeze` to replace its named pending scratch
or confirm an uncertain successful delivery. A different vocabulary requires a
fresh store: Commonplace has no unfreeze, bypass, schema removal, replacement,
or migration command.

Freeze is an accidental-change guardrail, not an authorization boundary against
arbitrary filesystem or database edits. Future operating automation must treat
the final marker as permanent and must not remove or modify it. Freezing the
vocabulary does not limit entity or fact authoring under that vocabulary.

Stores are validated before writing, without migration or automatic repair.
Concurrent SQLite-backed reads use consistent read-only snapshots. A read may
wait up to two seconds for transient SQLite WAL recovery or lock setup, but
Commonplace does not replay the command or retry any mutation. Read-snapshot
setup can therefore add its own bounded wait before a write command reaches the
separate process-level writer lock. That writer lock has a default two-second
wait; contention returns `conflict` (exit 3). Agents should still serialize
mutations and avoid unbounded read fan-out.

SQLite reads create no application-owned lock state; SQLite may maintain its own
WAL/SHM sidecars. Graph queries additionally hold the shared graph publication
lease described below, separately from SQLite and the writer lock. Invalid input
and input limits return exit 2; internal failures return exit 1. Execution
failures are JSON on stderr; successes are JSON on stdout. Argument syntax errors
use the standard CLI usage diagnostics.
Schema/ingest descriptions never open the store; record descriptions read current
vocabulary. Vocabulary operations neither load inference
nor open or rebuild Oxigraph, and remain available when derived graph state is
missing or corrupt.

## Cited entities, types and facts

After ingesting evidence and applying your vocabulary, create `record.json`:

```json
{
  "created_by": "manual",
  "items": [
    {"kind": "entity", "ref": "riley", "name": "Riley",
     "aliases": ["R"], "identifiers": [{"scheme": "email", "value": "riley@example.test"}]},
    {"kind": "type_membership", "entity": {"ref": "riley"},
     "entity_type": "person", "support": [{"passage_id": "passage:1"}]}
  ]
}
```

Use your actual passage IDs and declared type/scheme names:

```sh
cargo run --bin commonplace -- --store .commonplace record --describe --json
cargo run --bin commonplace -- --store .commonplace record record.json --json
cargo run --bin commonplace -- --store .commonplace get entity:1
cargo run --bin commonplace -- --store .commonplace get knowledge:1
```

One JSON request is atomic, including all entities, metadata, memberships, facts, and
graph publication. Repeat membership items to assign multiple types; successful
repeated writes receive distinct knowledge IDs. Entity creation is explicit and
never merges by name. Memberships resolve exactly one selector: `{"id":"entity:1"}`,
`{"identifier":{"scheme":"email","value":"riley@example.test"}}`, `{"name":"R"}`,
or `{"ref":"riley"}`. Exact case-sensitive name/alias matches must be unambiguous.
Local refs match `[A-Za-z][A-Za-z0-9_]{0,63}`, refer only to preceding entity items,
and last only for this request. Name changes are not supported by this release.

Add directed facts using a declared predicate and matching subject/object types:

```json
{"created_by":"manual","items":[
  {"kind":"entity","ref":"acme","name":"Acme"},
  {"kind":"type_membership","entity":{"ref":"acme"},"entity_type":"company"},
  {"kind":"fact","subject":{"id":"entity:1"},"predicate":"works_at",
   "object":{"entity":{"ref":"acme"}},"support":[{"passage_id":"passage:1"}]}
]}
```

Fact subjects and entity objects accept the same selectors as memberships.
Every required endpoint needs at least one permitted active type in the
**resulting** state; memberships may appear after facts within the request.
For a literal predicate, use `"object":{"literal":"approved"}` (string),
`{"literal":42}` (integer), `{"literal":true}` (boolean), or
`{"literal":"2026-09-28T12:25:23.123-05:00"}` (timestamp).
Declare the corresponding predicate kind and subject types with `schema apply`.
Timestamps normalize to UTC (`2026-09-28T17:25:23.123Z` here). Integer limits are
exactly -9223372036854775808 through 9223372036854775807; floats/exponent tokens,
including `1.0` and `1e0`, are rejected. Null, arrays and objects are not literals.
Empty strings are valid string facts. Repeated successful facts get distinct
knowledge IDs, including when the subject, predicate, object and citations match.

`record --jsonl facts.jsonl --json` accepts one request-envelope fragment per
line and flattens **all** fragments into one atomic request, not ingestion's
per-document writes:

```jsonl
{"created_by":"manual","items":[{"kind":"entity","ref":"riley","name":"Riley"}]}
{"created_by":"manual","items":[{"kind":"type_membership","entity":{"ref":"riley"},"entity_type":"person"}]}
```

Refs cross preceding fragments. Every fragment must have the same decoded
optional `created_by`, including omission; there is no inheritance or per-line
creator. Existing label validation is unchanged (explicit null is not accepted
by the CLI schema). LF/CRLF and an optional final newline are accepted.
Empty files, blank lines, BOMs, invalid UTF-8, multiple values per line, and
unknown/duplicate fields fail the complete request; `{"items":[]}` is an explicit
no-op. Decode errors name physical lines; item indices use flattened input order.
The file, --jsonl and --describe modes are exclusive; record has no stdin mode.

`entity_metadata` items require `entity_id` and accept `add_aliases`,
`remove_aliases`, `add_identifiers`, and `remove_identifiers`. Removals must belong
to that entity. Duplicates/conflicting additions and removals fail; adding an
already-owned value is a no-op. Identifiers cannot belong to two entities.
Aliases/identifiers retain creation time and optional request `created_by`.
Neither metadata correction nor a bare entity/empty request rebuilds the graph
or increments `knowledge_version`; they do not repair an unavailable graph.

Every membership/fact result and knowledge read includes `support`, including `[]`.
Support always cites a **whole canonical passage**, not an arbitrary snippet.
Optional `quote` must equal the entire text exactly. Optional `start_byte` and
`end_byte` must both be supplied and match that passage's zero-based half-open
UTF-8 byte range in its immutable revision. Old revision evidence stays valid
after new ingestion. Duplicate support IDs, bad quotes/offsets, undefined refs,
unknown vocabulary, or a late invalid item roll back the entire request.

Input is bounded to 1 MiB aggregate UTF-8 bytes (including JSONL separators),
1000 aggregate items, and 1000 entries in each
metadata/support array. Names, aliases, and identifier values contain 1-1024
Unicode scalar values and no NUL; supplied `created_by` contains 1-128 Unicode
scalar values. Unknown and duplicate JSON fields are rejected. `--describe`
generates the execution JSON Schema, a minimal example, current vocabulary,
JSONL examples/framing rules, and stateful/lexical validation rules.
`record` returns `schema_version`, `knowledge_version`, summary counts, and one
indexed result per input item. Entity reads include complete metadata and active
type/membership/fact IDs. Knowledge reads include subtype, schema version,
creation/withdrawal state, and hydrated support; retained withdrawn memberships
remain readable. Summary adds `facts_created`. A fact result has
`kind`, `index`, `knowledge_id`, `subject_entity_id`, `predicate_id`, `object`,
and `support`. Relationship object output is `{"entity_id":"entity:2"}`; literal
output is, for example,
`{"literal_kind":"string","literal":"approved","literal_json":"\"approved\""}`.
Fact get returns `kind: knowledge`, `subtype: fact`, common provenance/withdrawal/
support fields and the same subject, predicate and object. Membership fields are
unchanged. Literal JSON is the exact canonical SQLite representation.

Membership/fact publication builds from the pending SQLite transaction, then activates
the verified graph and commits under the publication lock. A reported commit
failure restores the previous graph; crashes in the activation/commit window
fail closed until explicit `graph rebuild`. **A nonzero exit can follow a
successful commit:** `post_commit_cleanup` means previous-directory removal or
final directory sync failed. Response-delivery errors use `internal_error`.
Their messages identify committed IDs/version and say not to retry `record`;
inspect those IDs with `get`, and rebuild for cleanup failures. If the process or
output stream dies, receipt delivery cannot be guaranteed: inspect canonical
state before retrying a non-idempotent request.

## Atomic withdrawal and retained history

Create `withdraw.json` using actual knowledge IDs returned by `record` or `get`:

```json
{"withdrawn_by":"manual","knowledge_ids":["knowledge:4","knowledge:5"]}
```

```sh
cargo run --bin commonplace -- withdraw --describe --json
cargo run --bin commonplace -- --store .commonplace withdraw withdraw.json --json
cargo run --bin commonplace -- --store .commonplace get knowledge:4
```

One JSON-file request withdraws **all** selected active memberships/facts or none.
The graph and entity active type/membership/fact IDs exclude withdrawn items.
`get` retains the exact subtype, canonical literal, evidence, schema version and
creation provenance, adding `withdrawn_at` and optional `withdrawn_by`. Withdrawal
is final; record a new corrected assertion rather than restoring or rewriting one.
Withdrawal does not delete sources, change search, detach citations, or cascade.
Later explicit source removal still detaches its evidence from retained history.

The complete proposed batch is validated **before** changing lifecycle columns.
Removing the last permitted type on either endpoint of a remaining active fact
fails. Retain another permitted active type or include every dependent fact in the
same withdrawal request; input order does not affect validity. Another active
membership of the same permitted type also suffices.

Input accepts 1-1000 unique canonical `knowledge:<positive i64>` IDs and at most
1 MiB of JSON bytes. Empty batches, duplicates, wrong ID tags, unknown/duplicate
fields, bad labels and already-withdrawn IDs yield `invalid_input`; unknown IDs
yield `not_found`. Supplied `withdrawn_by` contains 1-128 Unicode scalar values;
omit it for no label (explicit null is rejected). Oversize input yields
`limit_exceeded`. All failures reject the whole batch. The writer/publication
timeout is two seconds. There is no stdin, JSONL, positional-ID or check mode.
`--describe` is exclusive with the file argument, requires no store or models,
and returns the generated execution schema, input example, typed output example
and stateful validation rules.

Success uses `operation: withdraw`, `contract_version: "1"`, `status: complete`.
The result contains `knowledge_version`, `summary: {"items":N,"withdrawn":N}`, and
one indexed item per input ID in request order. Items carry the complete existing
Knowledge fields (without `get`'s outer `kind: knowledge` tag), including `support`
even when empty. All selected items share one withdrawal timestamp; the
knowledge version advances once. `withdraw --describe` shows a complete example.

The existing coordinated publication and rollback rules apply. **A nonzero exit
can follow a successful commit:** cleanup errors use `post_commit_cleanup`,
response-delivery errors use `internal_error`. Both report the committed version
and IDs with **do not retry withdraw** guidance; inspect those IDs using `get`.
Cleanup failures additionally require `graph rebuild`. If delivery is uncertain,
inspect the submitted IDs before retrying; responses are not replayed.

## Explicit source removal

```sh
cargo run --bin commonplace -- remove --describe --json
cargo run --bin commonplace -- --store .commonplace get doc:1
cargo run --bin commonplace -- --store .commonplace remove --source-key 'opaque:key' --json
```

Use the **exact stored source key** returned by `get`. File ingestion uses a
canonical `file://` URL; removal never resolves paths, normalizes URLs, trims
whitespace, or expands patterns. Keys must be nonempty and contain no NUL.
`--describe` generates a schema/example for the CLI argument object and a typed
output example without opening a store or loading models. It is **not a JSON
input mode** and cannot be combined with `--source-key`.

Removal permanently deletes that document, **all** revisions and passages,
lexical/vector index rows, and evidence links. It does not delete original source
files or remove/withdraw authored knowledge, entities, or vocabulary. Affected
active knowledge remains readable and projected, possibly with `support: []`;
unrelated evidence remains unchanged. Directory scan absence never removes a
source. There is no batch removal, restore, or tombstone. Reingesting the same
key creates a new document identity and does not reattach old citations.

```json
{
  "operation": "remove",
  "contract_version": "1",
  "status": "complete",
  "result": {
    "source_key": "opaque:key",
    "document_id": "doc:1",
    "deleted_revisions": 2,
    "deleted_passages": 3,
    "detached_evidence": 4,
    "affected_knowledge_ids": ["knowledge:1", "knowledge:2"],
    "knowledge_version": 2
  }
}
```

`detached_evidence` counts deleted knowledge/passage link rows across all
revisions. Affected knowledge IDs are distinct and numerically sorted, including
already-withdrawn items whose links were removed; withdrawal state is unchanged.
Each successful removal increments `knowledge_version` once, including removal
of an uncited or empty source. Unknown or already-removed keys return `not_found`
(exit 2) without mutation. Deleted document/revision/passage IDs return
`not_found` from `get` and cannot appear in search.

Removal uses the same coordinated publication as record. Before-commit failures
roll back the deletion; reported commit failure also restores the previous graph.
Restoration failure is explicit, and crash-window graph mismatches fail closed
until `graph rebuild`. **A nonzero exit may follow a successful commit:** cleanup
or final-sync errors use `post_commit_cleanup`, and response delivery errors use
`internal_error`. Their receipt identifies the removed document/key, affected
knowledge IDs, and committed version. Do not blindly retry `remove`: verify the
removed document is `not_found` and inspect affected knowledge with `get`; run
`graph rebuild` for cleanup failures. Process/output-stream death can prevent
receipt delivery, so inspect canonical state before retrying.

## Graph queries and explicit rebuild

```sh
cargo run --bin commonplace -- graph schema --json
cargo run --bin commonplace -- --store .commonplace graph query \
  'SELECT ?knowledge WHERE { ?knowledge ?p ?o }' --row-limit 1000 --timeout-ms 5000
cargo run --bin commonplace -- --store .commonplace graph query \
  'SELECT ?version WHERE { GRAPH <urn:commonplace:metadata> { <urn:commonplace:store> <urn:commonplace:property:knowledge_version> ?version } }'
cargo run --bin commonplace -- --store .commonplace graph rebuild --json
```

`graph schema` describes the canonical RDF 1.1 mapping, ID namespaces, property
datatypes, evidence fields, and SELECT examples without opening a store. Rebuild
projects active type memberships and facts, their referenced entities/types/predicates, and
complete cited passage/revision/document fields from SQLite, including pending
`record` writes. Facts use distinct knowledge IRIs with predicate/object links
and canonical typed literals, not shortcut triples or synthetic value nodes.
Unused vocabulary and bare entities do
not appear in the default graph. Empty stores retain version-zero metadata in
the query-visible reserved named graph `urn:commonplace:metadata`.

Queries accept a positional SPARQL SELECT through the native parser and a native
read-only store. Update, ASK, CONSTRUCT, and DESCRIBE return `invalid_input`.
HTTP and remote service handlers are disabled. `SERVICE` fails explicitly;
`SERVICE SILENT` preserves native error suppression without fetching data.
`FROM` clauses select local stored graphs only. No inference loads for graph
commands.

The result has exactly `kind`, `columns`, `rows`, and `truncated`:

```json
{"kind":"select","columns":["knowledge"],"rows":[],"truncated":false}
```

Columns preserve native projection order, and rows contain aligned RDF terms or
`null` for unbound variables. IRIs use `{"type":"uri","value":"urn:commonplace:knowledge:3"}`;
blank nodes use `{"type":"bnode","value":"label"}`. Literals use
`{"type":"literal","value":"42","datatype":"http://www.w3.org/2001/XMLSchema#integer","language":null}`.
Lexical values remain strings; language tags and datatypes are retained.
Native expression errors remain unbound: for the exact minimum i64, use
`"-9223372036854775808"^^<http://www.w3.org/2001/XMLSchema#integer>` rather than
the stock engine's bare numeric expression.

`--row-limit` defaults to **1000**. Zero is valid and returns no rows while probing
one solution to determine truncation. The lazy iterator consumes at most
`row_limit + 1` solutions, retaining at most `row_limit` rows without rewriting
the query. This does **not** bound internal sort/join/aggregate memory or individual
values. Use `ORDER BY` when ordering matters.

`--timeout-ms` defaults to **5000** and must be positive. An operation-scoped timer
requests native cancellation during execution and result iteration; it wakes
and joins promptly on completion, error, or truncation. Cancellation is
cooperative and may overshoot: this is not a hard wall-clock or memory limit,
and parsing is not preemptible. Budget expiry returns `limit_exceeded` (exit 2)
without partial results. Ctrl+C terminates the synchronous process.

Graph reads hold a shared `graph/publication.lock` lease through closing both
results and native storage, and compare metadata with a consistent SQLite
snapshot under the same lease. Missing/corrupt graphs or invalid/mismatched
metadata return `graph_unavailable` (exit 1), never a fallback or automatic repair.
Rebuild takes the writer and exclusive publication locks (two-second bounded
wait), reads committed SQLite, builds in bounded batches, flushes/closes/verifies
`graph/candidate`, and publishes via `graph/previous`. It returns
`{"knowledge_version":0}` for an empty store without incrementing that version.
Build/activation failure preserves the prior current state; restoration and
post-publication cleanup failures are explicit. Rebuild can recover missing
current state and removes only the known `candidate`/`previous` scratch paths,
not arbitrary files or authoritative SQLite state. The publication lock itself
is never renamed or removed.

## Ingestion and exact reads

The [Riley notebook fixture](fixtures/meeting-notes/README.md) provides separate
collections of personal Obsidian notes and shared Teams AI recaps. Its 36
documents cover Project Lantern, onboarding, escalation handoffs, manager 1:1s,
community volunteering, and Riley's planning, reflection, and people map.

```sh
cargo run --bin commonplace -- --store .commonplace ingest notes.md other.txt --temporal-state unknown
cargo run --bin commonplace -- --store .commonplace ingest ./notes --recursive \
  --include '**/*.md' --exclude 'archive/**' --temporal-state unknown
cargo run --bin commonplace -- --store .commonplace ingest notes.md \
  --title 'Planning notes' --source-type note --temporal-state dated \
  --occurred-at '2026-09-28T09:00:00-05:00' --metadata '{"project":"commonplace"}'
cargo run --bin commonplace -- ingest --describe --json
cargo run --bin commonplace -- --store .commonplace get doc:1
cargo run --bin commonplace -- --store .commonplace get revision:1
cargo run --bin commonplace -- --store .commonplace get passage:1
```

Use the IDs returned by your own ingestion. Direct inputs accept regular UTF-8
files. Directory scans select lowercase `.md` and `.txt` extensions; recursion
is opt-in. Explicit symlinks fail, discovered symlinks are skipped. Include and
exclude globs apply only to directory scans, match case-sensitive paths relative
to each scan root, and use `/` separators (`*` does not cross a separator,
`**` can). Includes are ORed; excludes win. Explicit roots retain argument order;
matching files within each root are sorted by path after bounded enumeration.
Enumeration holds paths, not file bodies, and exceeding the document bound
rejects the command before any publication.

The default source key is a canonical absolute file URI, identical for direct
and scanned input. Moving a file gives it a new identity. The default title is
its filename, source type is `file`, and metadata is `{}`.
Every file/scan/stdin command requires **`--temporal-state`**:
`dated` requires `--occurred-at` as an RFC3339 event timestamp;
`timeless` means event time does not apply; `unknown` means it applies but is
unavailable. The latter two prohibit `--occurred-at`. No state is inferred or
defaulted. Shared intent/metadata options apply to every document in the command;
mixed-state collections use separate commands or self-contained JSONL. Source times are
normalized to UTC RFC 3339. Custom metadata is an object with recursively sorted
keys; floating-point and out-of-range integer numbers are rejected.

Every successful item is fully published in one SQLite transaction, including
its FTS and 384-dimensional vector rows. Rerunning exact text and metadata returns
`unchanged` and updates only `last_ingested_at`. Changing either creates an
immutable revision, including changes to temporal state alone. Old revisions and
passages remain readable; only the current
revision is indexed. A concurrent source change during embedding returns a
rerunnable `conflict` rather than overwriting the other writer's revision.
Ingestion does not rebuild or open the graph.

UTF-8 is retained exactly, including CRLF, BOM, NUL, combining marks, and non-BMP
characters. Paragraph-aware passages cover every source byte without gaps,
with a fixed 1024-byte ceiling **including overlap** and UTF-8-safe boundaries.
Whole paragraphs are packed without overlap as before. Oversized paragraphs use
approximately balanced lengths with **target overlap: 102 bytes (10% of maximum
passage size)**, not 10% of each smaller chunk. Nearby whitespace/UTF-8 boundaries
can adjust the achieved overlap; both start and end offsets advance. The chunk
count conservatively reserves target context, not a mathematically minimal count
after adjustments. A whitespace-free 1100-byte ASCII paragraph yields
`[0,601)` and `[499,1100)`: 601/601 bytes with 102 repeated source bytes, not a
tiny rigid-window tail. Each citation remains an exact contiguous source slice.
Only newly created revisions use this preparation. Unchanged inputs retain
their revision/passage IDs and persisted historical boundaries; old citations
are never rewritten or reindexed. Empty files are valid and
have zero passages. `get` returns a complete document with all revision IDs, a
complete revision with its text/metadata/temporal state and passage IDs, or an exact passage
citation with source metadata and half-open byte offsets. It does not load
models or truncate records. Entity and knowledge reads are described above.

Ingestion uses the common envelope with `complete`, `partial`, or `failed` status.
Its `result` contains `summary` counts (`added`, `updated`, `unchanged`, `failed`)
and `items`. Every item has `input`, `source_key`, `status`, `document_id`,
`revision_id`, `passage_ids`, and `error`. Failed items have null IDs, an empty
passage-ID list, and `error: {stage, code, message}`; successful items have
`error: null`. Repeated source keys fail that item. Failures do not undo successful
items. Valid attempted batches, including all-failed batches, return their result
on stdout. Exit precedence is runtime failure **1**, otherwise conflict **3**,
otherwise any failed item **2**, otherwise **0**. Command-level errors retain the
stderr error envelope.

For automation, capture stdout, stderr, and the exit code separately. A nonzero
exit does **not** mean stdout is empty or that no files were saved:

| Outcome | Where to read it | What the caller should do |
| --- | --- | --- |
| Completed batch (exit 0) | stdout JSON | Read `result.summary` and returned IDs. |
| Partial or all-failed batch (nonzero exit) | stdout JSON | Inspect `result.items[].error`; successful items remain saved. |
| Command-level failure (nonzero exit) | stderr JSON | Inspect `error.code` and `error.message`. |
| Invalid CLI syntax (exit 2) | stderr usage text | Correct the arguments; do not assume stderr is always JSON. |

Do not discard stdout on a nonzero exit or merge stderr into stdout before
parsing JSON. Use the envelope's `status` and per-item errors for details; the
exit code alone cannot distinguish a failed batch from a command-level failure.

All limits below must be positive. They are operational bounds, not model or
passage-representation settings:

| Option | Default |
| --- | --- |
| `--max-source-bytes` | 10,485,760 (10 MiB) |
| `--max-documents` | 1,000 |
| `--max-passages` | 10,000 per document |
| `--embedding-batch-size` | 32 |
| `--max-json-bytes` | 1,048,576 (metadata argument/canonical metadata and each encoded JSONL record) |
| `--writer-lock-timeout-ms` | 2,000 |

### Caller-prepared text and JSON Lines

Agents and other callers own acquisition and extraction. Commonplace accepts
their exact normalized UTF-8 text; it does not fetch URLs, connect to services,
or extract email/PDF contents.

```sh
printf 'Riley approved the draft.\n' |
  commonplace ingest --stdin --source-key notes/42 --temporal-state unknown \
    --title 'Draft decision' --metadata '{"project":"riley"}'
commonplace ingest --jsonl batch.jsonl
producer | commonplace ingest --jsonl -
```

`--stdin` ingests one document with required opaque `--source-key` and explicit
`--temporal-state`. Metadata flags apply to that document. Streamed defaults are null title,
source type `text`, and metadata `{}`. Empty text is valid. Keys are nonempty
and NUL-free, are not trimmed or normalized, and share the same namespace as
file keys. Reusing a key with identical text/metadata returns `unchanged`;
changing metadata or text creates an immutable revision of the same document.
Changing the key creates a different document.

Each JSONL physical line is a self-contained record:

```json
{"source_key":"notes/42","text":"Riley approved the draft.\n","temporal_state":"unknown","title":"Draft decision","metadata":{"project":"riley"}}
{"source_key":"notes/43","text":"Review on Friday.\n","temporal_state":"dated","source_type":"note","occurred_at":"2026-09-28T10:00:00-05:00"}
{"source_key":"notes/44","text":"Project reference material.\n","temporal_state":"timeless"}
```

`source_key`, `text` and `temporal_state` are required strings. Dated records
require valid string `occurred_at`; timeless/unknown records omit it. Explicit
null state/time is rejected. Optional fields are `title`,
`source_type`, `occurred_at`, and `metadata`. Unknown fields and duplicate object
keys (including inside metadata) fail the item. JSONL does not accept global
metadata overrides or directory scan options. Exactly one input mode is allowed;
`--stdin` and `--jsonl -` cannot be selected together.

LF/CRLF delimiters are framing, not source text. Escaped text inside a record
is preserved exactly. A final line without a delimiter is valid; blank lines
and invalid UTF-8/JSON fail individually and processing continues. Empty input
returns an empty complete result. Input labels are `stdin`, `stdin:1`, or
`batch.jsonl:1` (1-based physical line).

The JSON limit bounds each encoded line excluding LF/CRLF, independently of the
decoded source-byte limit and canonical metadata limit. A document near the
10 MiB source limit requires raising the JSON limit, including escape overhead.
An oversized line stops with one terminal `limit_exceeded` item without draining
the rest of the input. Every record, including failed records, consumes a
document slot; excess input produces one terminal failed item without parsing
or publishing another record. Stream read failures likewise terminate with an
`internal_error` item. Prior successful publications remain saved. Existing
file requests still reject excessive known document counts before publication.
Readers buffer one bounded record; retained identities/results grow with the
configured document and record limits rather than holding all document bodies.

All modes use the existing stdout result and exit precedence above; raw stdin
body failures are item errors, not command-level errors. Invalid command options
and failure opening a JSONL file use the existing command-error path.
`ingest --describe` provides generated command `input_schema`/`example` and
ingest-only JSONL `record_schema`/`record_example`/`record_examples` fields, using the execution
validators. Command `source_type: null` means resolve the mode default, not
store a null source type. Semantic checks also enforce canonical metadata,
timestamps, byte limits, and duplicate fields.

Manifest input is explicitly deferred; saved JSONL supplies the current generic
batch workflow.

## Hybrid search

```sh
cargo run --bin commonplace -- --store .commonplace search 'Lantern pilot schedule' --limit 5
cargo run --bin commonplace -- --store .commonplace search 'escalation acknowledgement' \
  --since '2026-09-21T00:00:00Z' --source-type note --source-type recap --limit 10 --json
```

Search returns ranked current-revision passages with complete exact citations:
the document/revision/passage IDs, source key/title/type/temporal state/time/metadata, ordinal,
and half-open byte offsets from `get`, plus 1-based `rank`. The common `search`
envelope has `result: {items, truncated, temporal_filter}` (plus `scope` when supplied). Without date bounds,
`temporal_filter` is null. There is no score threshold: unrelated queries
can return nearest passages rather than an empty list.

The query is plain text, not FTS syntax. Whitespace-separated terms are safely
quoted and ORed for FTS5; the original query goes to the pinned embedding and
reranker. Queries must be nonblank, contain no NUL, and fit 4096 UTF-8 bytes and
64 terms. `--limit` defaults to **10**, accepts **0–50**, and never disables model
validation. `--since` is an inclusive RFC3339 dated-source cutoff, with nanosecond
precision, **plus timeless context**; it excludes unknown and older dated sources.
Ingestion time never substitutes for event time. Source type filters are exact and case-sensitive,
ORed when repeated, with at most 32 supplied values and 4096 aggregate UTF-8 bytes.
Files use source type `file` unless overridden; temporal intent is always explicit.
Dates written in Markdown/frontmatter or filenames are not automatically extracted.

With either date bound, `temporal_filter` contains normalized nullable `since`/`until`,
`includes_timeless: true`, `coverage: {eligible_dated, timeless, older_dated, newer_dated, excluded_unknown}`, and
`diagnostics`. Counts concern **current sources after scope and source-type filters**, not
query matches, passage phrase constraints, overlapping passages, historical
revisions or candidates; empty sources count.
When any unknown dates were excluded, diagnostics contains
`{"code":"unknown_dates_excluded","message":"Sources with unknown event dates are excluded by the date window; results do not cover those sources."}`,
even with nonempty/empty results or zero limit. Otherwise diagnostics is empty.
These counts describe collection coverage, not exhaustive relevance or certainty
that no evidence exists. Returned items and authoritative revision/passage/
knowledge-support reads retain their immutable cited `temporal_state`.

### Graph-selected scope, date windows and compact evidence

Select canonical document IRIs in any schema-neutral graph SELECT, then search
only that selection:

```sh
commonplace graph query "$SELECT_DOCUMENTS" --document-scope document \
  | jq '.result' > scope.json
commonplace search 'modernization risks and next actions' --scope scope.json \
  --since '2026-09-14T00:00:00Z' --until '2026-10-05T23:59:59.999999999Z' \
  --grouped --limit 10
# Or pipe jq '.result' directly into search ... --scope -.
commonplace search --describe-scope
```

`--document-scope` takes a projected variable name **without `?`**. Every retained
binding must be a canonical `urn:commonplace:doc:N` IRI; unbound values, literals,
other resource kinds and noncanonical spellings fail explicitly. Its result is
`{"document_ids":["doc:1"],"truncated":false}`. Search reads **that result object**,
not the whole command envelope: use `jq '.result'`. Scope input is strict JSON,
bounded to 64 KiB and 1024 supplied canonical IDs (including duplicates).
Duplicates are deduplicated and counted; missing/removed IDs are reported and
excluded. An explicit empty array never becomes a corpus-wide search. Both
lexical and vector SQL apply scope, type, date and passage-phrase constraints
**before** candidate selection, fusion and reranking. Historical citations still
resolve through `get`; only current revisions are searchable.

Scoped output adds `scope`: supplied/duplicate/selected/existing source counts,
`missing_document_ids`, `excluded_source_type`, `eligible_sources`,
`eligible_passages`, selection and individual retrieval-stage truncation flags,
and diagnostics. Eligible sources include empty sources after type/date filters;
eligible passages additionally satisfy the phrase constraint. Temporal buckets
count current sources after scope and type filters, independently of query and
phrase. A truncated graph selection remains searchable but is explicitly warned
as partial. `empty_scope` and `no_eligible_passages` distinguish scoped emptiness
from corpus-wide absence. Zero retained-result limit is not a no-match claim.
Graph-selected documents need not include every relevant source.

`--until` is an **inclusive** upper RFC3339 event bound, with the same exact UTC
fractional-second handling as `--since`. Either bound includes timeless context
and excludes unknown dates. Equal bounds admit that exact instant; reversed
windows fail `invalid_input`. With either date bound, the uniform
`temporal_filter` now contains nullable `since` and `until`,
`includes_timeless: true`, five coverage buckets (`eligible_dated`, `timeless`,
`older_dated`, `newer_dated`, `excluded_unknown`) and diagnostics. Without bounds
it remains null. **JSON contract change:** since-only responses now also include
`until: null` and `newer_dated: 0`; the unknown-date warning refers to the date
window. Store/config format remains **/3**, with no reload or migration needed.

`--grouped` replaces `items` with `groups`, sharing document/revision IDs, source
key/title/type/state/time once per source. Groups follow first retained rank;
passages within each group retain global rank. `--limit` still bounds **passages**,
not groups. Each independent passage contains canonical passage ID, rank, original
half-open `start_byte`/`end_byte`, and full exact `text`.
Passages are never shortened or
stitched into reconstructed document text: compactness comes from sharing source
fields and omitting metadata, not discarding relevant text. Each passage is already
bounded to 1024 UTF-8 bytes. Full metadata and historical revisions remain available
via `get`. `truncated` describes collection cutoffs only, never text shortening.

### Read-only entity discovery

```sh
commonplace entity list --type company --missing-identifier email --limit 100
commonplace entity list --after entity:100 --limit 100
commonplace entity resolve --name 'Exact name or alias'
commonplace entity resolve --scheme email --value 'opaque@example.test'
commonplace entity resolve --id entity:1
```

`entity list` returns `{items,truncated,next_after}` in canonical entity-ID
order. Items are the same complete entity metadata as `get`, without its `kind`
tag: canonical ID/name, aliases, scheme/value identifiers and active knowledge
IDs together. Type filters use **active memberships**, not withdrawn history;
untyped entities are included without a type filter. Missing-identifier filters
use an existing scheme. Unknown types/schemes fail `invalid_input`. Limit defaults
to 100 and accepts 0–1000; `--after` is exclusive keyset paging, and `next_after`
is the last retained ID only when more results exist. Zero returns no items with
an honest truncation flag and null cursor.

Resolution reuses record's exact, case-sensitive name/alias/identifier semantics:
ambiguity fails `conflict`, no match fails `not_found`. Schemes and values are not
normalized or interpreted. These commands read SQLite only, load no models,
do not open/rebuild RDF and never replay metadata writes for verification.

Use optional single `--must-contain 'ACME Corp'` to require a **literal contiguous
phrase in each returned passage**, not elsewhere in the source, title, or
metadata:

```sh
commonplace search 'MACC commitment' --must-contain 'ACME Corp' --json
```

Both SQL candidate paths apply this constraint **before** their limits.
Matching lowercases both whole strings with Rust's Unicode `str::to_lowercase`
and checks substring containment. This is not full Unicode case folding or
normalization: `ÉCRAN` matches `écran`, `Straße` differs from `STRASSE`,
composed/decomposed accents differ, and contextual Greek final sigma differs from
ordinary sigma. Whitespace (including leading/trailing spaces) and punctuation
are literal. There is no regex, raw FTS, word-boundary, entity, or alias matching.
The phrase must be nonblank, contain no NUL, and fit 4096 original UTF-8 bytes and
64 whitespace-delimited terms. Without the flag, normal search is unchanged.
Even with it, results are bounded retrieval, not an exhaustive relevant-evidence
or no-evidence guarantee.

Both paths apply the same filters before retaining 64 candidates each. Vector
distance is evaluated by sqlite-vec inside SQLite, not by an application vector
scan; its native exhaustive work scales with the eligible corpus. BM25 and
distance ties use passage IDs. Equal-weight reciprocal-rank fusion (constant 60)
deduplicates candidates, then the pinned local reranker processes at most 64
passages in batches of eight. Fusion ties use passage IDs; final-score ties use
fusion order. Both retrieval paths and hydration share one SQLite read snapshot.
Adjacent overlapping passages remain distinct canonical passage IDs, so repeated
context can appear in multiple results. Existing passage-ID deduplication,
ranking and truncation behavior is unchanged; there is no content-level
deduplication.

`truncated: true` means a candidate sentinel proved additional candidates, the
fusion set exceeded the rerank cap, or final results exceeded `--limit`. It is
not an exhaustive relevance claim and does not describe tokenizer truncation:
embedding inputs use a 256-token window; reranker query/passage pairs use 512.
Inference may shorten inputs, but returned evidence always contains the full
exact passage. Zero limit returns no items and reports whether retrieval omitted
any. Required-model errors still fail even for empty results or zero limit;
search never falls back to lexical-only. Missing/extra index rows or incompatible
vector declarations fail without repair or changes to the store.

### Local model cache

Changed nonempty documents lazily load one local FastEmbed 7.1.0/ONNX Runtime
session per command. The embedding is
`Qdrant/all-MiniLM-L6-v2-onnx@8f518e882455312b086101e60691f5e6e2f05c3c`,
mean pooled, maximum token length 256, unit-normalized, 384 dimensions. Model
configuration, tokenizer files, and weights are hash-verified against the pinned
[artifact identities](spikes/rust-packaging/models.json) before inference.
Source text and metadata never enter a network request.

Without an override, the stock synchronous `hf-hub` 0.5.0 cache is used:
`$HF_HOME/hub`, or `~/.cache/huggingface/hub` if `HF_HOME` is unset. This client
does not use `HF_HUB_CACHE`. Cache hits are verified without a network probe;
missing artifacts are acquired from the exact immutable revision at
`https://huggingface.co`. Ingestion acquires only the embedding; search also loads
`jinaai/jina-reranker-v1-turbo-en@b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2`
through the same verified cache path. Public artifact requests use no
credentials. Network/policy failures are reported as `model_unavailable`; no
alternate provider or revision is tried.

Set `COMMONPLACE_MODEL_CACHE` for a **strict offline** deployment or test cache
using `<cache>/<revision>/<filename>` (the P2 cache layout). All five embedding
artifacts must be present for embedding, and search also requires all five pinned
reranker artifacts. Missing or corrupt files fail explicitly; this override
never downloads or replaces anything. Corrupt ordinary cache hits also fail
without automatic replacement. Unchanged and empty documents require no model.

### Persistent user path defaults

Commonplace can read one optional user file containing non-secret defaults for
the persistent store and strict pinned-model cache:

```json
{
  "format": "commonplace-user-config/1",
  "store": "/absolute/path/to/persistent/store",
  "model_cache": "/absolute/path/to/installed/pinned-models"
}
```

The file is:

- macOS: `$HOME/Library/Application Support/commonplace/config.json`
- Linux/other Unix: `$XDG_CONFIG_HOME/commonplace/config.json`, or
  `$HOME/.config/commonplace/config.json` when `XDG_CONFIG_HOME` is unset or
  relative
- Windows: `%APPDATA%\commonplace\config.json`

`format` is required; `store` and `model_cache` are independently optional and
must be absolute. Unknown or duplicate fields, malformed JSON, unsupported
formats, relative paths, and unreadable files fail executing commands with
`configuration_error` (exit 2), even when command-line or environment overrides
are present. A missing file preserves existing behavior.

Store precedence is `--store`, `COMMONPLACE_STORE`, user configuration, then
`.commonplace`. Strict model-cache precedence is `--model-cache`,
`COMMONPLACE_MODEL_CACHE`, user configuration, then the stock `hf-hub` cache.
CLI/environment paths may remain relative to that invocation. Inspect the
selection without opening a store or loading models:

```sh
commonplace config show
```

For example, a loaded file produces the common JSON envelope with:

```json
{
  "config_file": {
    "path": "/Users/example/Library/Application Support/commonplace/config.json",
    "status": "loaded"
  },
  "store": {
    "path": "/Users/example/.local/share/commonplace/stores/personal",
    "source": "user_config"
  },
  "model_cache": {
    "path": "/Users/example/.local/share/commonplace/releases/commonplace-0.1.0-0123456789ab-macos-arm64/pinned-models",
    "source": "user_config"
  }
}
```

File status is `unavailable`, `absent`, or `loaded`. Value source is
`command_line`, `environment`, `user_config`, `default`, or `runtime_default`;
the last reports a null strict override so the stock model cache applies.

This user file is not `<store>/config.json`. The latter is the backend-managed
`commonplace-config/3` layout marker created by `init`. Commonplace does not
write, migrate, repair, or profile user configuration. `config show` does not
check path health. Only explicit `init` may create the selected store; a selected
strict cache never downloads or falls back when artifacts are missing or corrupt.

## Development checks

The manual [local macOS arm64 packaging procedure](docs/release-local.md) builds
and exercises an extracted production binary with copied pinned models. Its
test-only `COMMONPLACE_TEST_BINARY` selector emits an in-process path/hash receipt;
package acceptance must verify that receipt, not just a passing test result.
This remains developer-host evidence, not independent clean-target certification.

Install an already-produced verified archive through the repository-owned
macOS arm64 installer:

```sh
python3 scripts/install-local-macos.py \
  /absolute/path/commonplace-<version>-<commit>-macos-arm64.tar.gz
```

It verifies the archive sidecar and bundle, publishes immutable contents under
`~/.local/share/commonplace/releases/`, atomically updates
`~/.local/bin/commonplace`, and atomically writes
`~/.local/share/commonplace/installed-release.json`. Exact reinstall is
idempotent; verified updates preserve stores, configuration, reports and earlier
releases. The installer neither downloads nor edits shell profiles, so put
`~/.local/bin` on `PATH` yourself or use the absolute executable path. See the
local release guide for receipt inspection and partial-install retry rules.
It also narrowly adopts the existing strict `commonplace-manual-install/1`
current-main installation on its first verified archive update; other legacy or
mismatched manual state is rejected.
Interrupted activation is pinned to its exact archive by
`~/.local/share/commonplace/installed-release.pending.json`; retry that archive
to complete receipt publication and clear the marker.

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Use `-j2` for the first native RocksDB build on memory-constrained hosts.
The graph integration is exercised locally on macOS arm64. Independent
clean-target and other supported-platform execution remain P10 release gates.
There is also a concrete Windows portability gap, not just missing test evidence:
directory publication currently uses `File::open(directory).sync_all()`.
Windows-compatible directory synchronization for initialization, activation,
restoration, and cleanup still needs implementation/verification in P10 before
release. Sync errors remain explicit; there is no no-op or fallback.

Ordinary tests use deterministic substitutes only for expensive inference; they
use real SQLite, FTS5, and sqlite-vec and never acquire model weights. The separate
real-model integration target is ignored by default. Explicit invocation requires
the prepared cache and fails, rather than skipping, if it is absent or corrupt:

```sh
COMMONPLACE_MODEL_CACHE=/absolute/path/to/prepared/pinned-models \
  cargo test --locked --offline --test real_models -- \
  --ignored --exact real_model_ingest_get_offline --nocapture
COMMONPLACE_MODEL_CACHE=/absolute/path/to/prepared/pinned-models \
  cargo test --locked --offline --test real_models -- \
  --ignored --exact real_model_search_offline --nocapture
COMMONPLACE_MODEL_CACHE=/absolute/path/to/prepared/pinned-models \
  cargo test --locked --offline --test real_models -- \
  --ignored --exact real_model_stream_ingest_get_offline --nocapture
```

The file check runs the public binary over multiple files, repeats ingestion,
makes metadata and content revisions, reads exact old/new evidence, and checks current
FTS/vector IDs on a version-3 store. It verifies ingestion leaves graph files
unchanged, then records a multiply-typed entity, cited relationship and literal
decision through the public CLI, compares old/new passage citations with exact
reads, and repeats after rebuild/reopen.
It then removes that multiply revised source by its exact stored file key,
checks full canonical/index/evidence deletion, retained empty/mixed membership,
relationship and literal-fact
support and unrelated citations, public search exclusion, graph/reopen parity,
and new document identity on reingestion. All sources used are fresh temporary
test files. It then withdraws cited and retained empty-support facts atomically,
checks exact historical fields, active IDs/RDF absence, unchanged sources/search,
rebuild/reopen parity, and withdrawal lifecycle preservation through later removal.
The stream check runs real
nonempty stdin/JSONL submissions, unchanged and metadata-only reruns, partial
failures, exact old reads, and current index/unchanged graph checks. The file
check also verifies offline hits through the standard `HF_HOME` cache.
On macOS, prefix `cargo` with
`sandbox-exec -p '(version 1)(allow default)(deny network*)'` to enforce network
denial for the test and its binary children. This is local runtime evidence,
not other-platform or clean-machine release validation.

The search invocation ingests only the 36 Riley source documents under
`Obsidian Notes` and `AI Summaries`, excluding the fixture guides. It evaluates
four checked-in top-five source/quote expectations in
[`fixtures/search-relevance.json`](fixtures/search-relevance.json), repeat order,
exact `get` citation parity, filtered/empty/zero results, and required reranker
failures. These are bounded retrieval expectations, not support for arbitrary
natural-language questions or generated answers.

Run development checks locally. GitHub Actions CI is disabled to conserve
Actions usage; enabling or dispatching workflows requires explicit approval.

For packet work and build status, use the repo-local
[`commonplace-progress` skill](.github/skills/commonplace-progress/SKILL.md).
The execution plan links each packet's GitHub progress issue.

## Design documents

The standalone design specification is organized under
[`docs/specification/`](docs/specification/README.md):

- [Product design](docs/specification/product.md)
- [Software architecture](docs/specification/architecture.md)
- [Persistence design](docs/specification/persistence.md)
- [Implementation and acceptance](docs/specification/implementation.md)
