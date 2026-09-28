# Commonplace

A local, evidence-backed knowledge system.

Commonplace is a lean personal knowledge tool for batch document ingestion,
hybrid SQLite search, explicitly authored cited knowledge, and local graph
queries.

The Rust implementation supports store initialization, additive vocabulary,
file and generic stdin/JSONL ingestion, hybrid search, atomic cited entity/type/fact
authoring, authoritative reads, and read-only RDF graph queries and rebuild:

```sh
cargo run --bin commonplace -- --store .commonplace init
```

The command creates the SQLite schema, FTS5 and sqlite-vec indexes, and the
derived stock Oxigraph 0.5.11/RocksDB database. New stores use
`commonplace-store/2` and `commonplace-config/2`. It is safe to rerun against a
compatible initialized store with a valid, version-matching graph. Version-1
Grafeo stores and unknown layouts are rejected without migration, relabeling,
or automatic repair; use a fresh directory for a new store.

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
cargo run --bin commonplace -- schema apply --describe --json
```

JSON is the default output; `--json` explicitly requests the same format. The
`init` response field names are unchanged; its format is `commonplace-store/2`.
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

Stores are validated before writing, without migration or automatic repair.
Writers use a process-level lock with a default two-second wait; contention
returns `conflict` (exit 3). Invalid input and input limits return exit 2; internal
failures return exit 1. Execution failures are JSON on stderr; successes are JSON
on stdout. Argument syntax errors use the standard CLI usage diagnostics.
Checks and reads use consistent read-only SQLite snapshots and create no
application-owned lock state; SQLite may maintain its own WAL/SHM sidecars.
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
and last only for this request. Withdrawal and name changes are not supported
by this release.

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
cargo run --bin commonplace -- --store .commonplace ingest notes.md other.txt
cargo run --bin commonplace -- --store .commonplace ingest ./notes --recursive \
  --include '**/*.md' --exclude 'archive/**'
cargo run --bin commonplace -- --store .commonplace ingest notes.md \
  --title 'Planning notes' --source-type note \
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
its filename, source type is `file`, source time is null, and metadata is `{}`.
Metadata overrides apply to every document in the command. Source times are
normalized to UTC RFC 3339. Custom metadata is an object with recursively sorted
keys; floating-point and out-of-range integer numbers are rejected.

Every successful item is fully published in one SQLite transaction, including
its FTS and 384-dimensional vector rows. Rerunning exact text and metadata returns
`unchanged` and updates only `last_ingested_at`. Changing either creates an
immutable revision. Old revisions and passages remain readable; only the current
revision is indexed. A concurrent source change during embedding returns a
rerunnable `conflict` rather than overwriting the other writer's revision.
Ingestion does not rebuild or open the graph.

UTF-8 is retained exactly, including CRLF, BOM, NUL, combining marks, and non-BMP
characters. Paragraph-aware passages cover every source byte without overlap,
with a fixed 1024-byte target and UTF-8-safe boundaries. Empty files are valid and
have zero passages. `get` returns a complete document with all revision IDs, a
complete revision with its text/metadata and passage IDs, or an exact passage
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
  commonplace ingest --stdin --source-key notes/42 \
    --title 'Draft decision' --metadata '{"project":"riley"}'
commonplace ingest --jsonl batch.jsonl
producer | commonplace ingest --jsonl -
```

`--stdin` ingests one document with a required opaque `--source-key`. Existing
metadata flags apply to that document. Streamed defaults are null title/time,
source type `text`, and metadata `{}`. Empty text is valid. Keys are nonempty
and NUL-free, are not trimmed or normalized, and share the same namespace as
file keys. Reusing a key with identical text/metadata returns `unchanged`;
changing metadata or text creates an immutable revision of the same document.
Changing the key creates a different document.

Each JSONL physical line is a self-contained record:

```json
{"source_key":"notes/42","text":"Riley approved the draft.\n","title":"Draft decision","metadata":{"project":"riley"}}
{"source_key":"notes/43","text":"Review on Friday.\n","source_type":"note","occurred_at":"2026-09-28T10:00:00-05:00"}
```

`source_key` and `text` are required strings. Optional fields are `title`,
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
ingest-only JSONL `record_schema`/`record_example` fields, using the execution
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
the document/revision/passage IDs, source key/title/type/time/metadata, ordinal,
and half-open byte offsets from `get`, plus 1-based `rank`. The common `search`
envelope has `result: {items, truncated}`. Empty results are
`{"items":[],"truncated":false}`. There is no score threshold: unrelated queries
can return nearest passages rather than an empty list.

The query is plain text, not FTS syntax. Whitespace-separated terms are safely
quoted and ORed for FTS5; the original query goes to the pinned embedding and
reranker. Queries must be nonblank, contain no NUL, and fit 4096 UTF-8 bytes and
64 terms. `--limit` defaults to **10**, accepts **0–50**, and never disables model
validation. `--since` is inclusive RFC3339 source time, with nanosecond precision;
it excludes unknown times. Source type filters are exact and case-sensitive,
ORed when repeated, with at most 32 supplied values and 4096 aggregate UTF-8 bytes.
Files ingested without metadata overrides have source type `file` and null time;
dates written in Markdown are not automatically extracted.

Both paths apply the same filters before retaining 64 candidates each. Vector
distance is evaluated by sqlite-vec inside SQLite, not by an application vector
scan; its native exhaustive work scales with the eligible corpus. BM25 and
distance ties use passage IDs. Equal-weight reciprocal-rank fusion (constant 60)
deduplicates candidates, then the pinned local reranker processes at most 64
passages in batches of eight. Fusion ties use passage IDs; final-score ties use
fusion order. Both retrieval paths and hydration share one SQLite read snapshot.

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

## Development checks

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
FTS/vector IDs on a version-2 store. It verifies ingestion leaves graph files
unchanged, then records a multiply-typed entity, cited relationship and literal
decision through the public CLI, compares old/new passage citations with exact
reads, and repeats after rebuild/reopen.
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
