# Commonplace

A local, evidence-backed knowledge system.

Commonplace is a lean personal knowledge tool for batch document ingestion,
hybrid SQLite search, explicitly authored cited knowledge, and local graph
queries.

The Rust implementation supports store initialization and additive vocabulary:

```sh
cargo run --bin commonplace -- --store .commonplace init
```

The command creates the SQLite schema, FTS5 and sqlite-vec indexes, and the
derived Grafeo storage location. It is safe to rerun against a compatible
initialized store.

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
existing `init` response is unchanged. Schema operations use the same
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
nor open or rebuild Grafeo.

Development checks:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

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

The specifications now select stock Oxigraph and SPARQL SELECT. Production
replacement, including the version-2 store layout, is assigned to P5a; the
initialization behavior described above still uses Grafeo until that work lands.
