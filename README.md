# Commonplace

A local, evidence-backed knowledge system.

Commonplace is a lean personal knowledge tool for batch document ingestion,
hybrid SQLite search, explicitly authored cited knowledge, and local graph
queries.

The Rust implementation supports store initialization, additive vocabulary, and
read-only RDF graph queries and rebuild:

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
Descriptions never open the store. Vocabulary operations neither load inference
nor open or rebuild Oxigraph, and remain available when derived graph state is
missing or corrupt.

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
currently projects active type memberships, their referenced entities/types, and
complete cited passage/revision/document fields from SQLite. There is no public
authoring command yet. Active facts fail explicitly rather than being omitted;
fact authoring/projection belongs to P6. Unused vocabulary and bare entities do
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

## Development

Development checks:

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
