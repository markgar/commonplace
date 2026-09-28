# Grafeo evaluation spike

The [P2 resolution follow-up](grafeo-resolution.md) now demonstrates passing
isolated prototypes for bounded Cypher, semantic result identity, and two durable
version carriers. It recommends an adoption decision but does not change the
unpatched gate below. The exact patch, regression harness, and recorded results
are preserved separately.

## P2 local graph gate (2026-09-28 UTC)

**GRAPH gate: BLOCKED.** Grafeo 0.5.43 remains the selected component
provisionally; P5a must not implement around the failures below without an
explicit decision. This report does not weaken the specification. The independent
[combined inference gate passes locally](rust-packaging.md) on macOS arm64.
All new evidence below is local; no Actions or remote platform checks ran.

The shared bounded probe lives in `spikes/grafeo-evaluation/src/preflight.rs`,
consumed by both spike binaries. Run the combined reproduction in the packaging
report, or invoke the graph-only preflight on a fresh directory:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
COMMONPLACE_GRAFEO_DATA_DIR="$(mktemp -d /tmp/commonplace-graph-p2.XXXXXX)" \
  cargo run --manifest-path spikes/grafeo-evaluation/Cargo.toml \
  --release --locked --offline -- preflight
```

The combined binary's equivalent `grafeo` invocation was executed. Its graph
diagnostic returns exit zero only when the expected evidence is reproduced,
including the blockers; inspect `gate: blocked`, not exit status, for gate
acceptance. The standalone shared-code entry point was compiled/linted, not
separately rerun as a duplicate platform test.

| Requirement | Expected | Observed in combined local execution |
| --- | --- | --- |
| Native `Role::ReadOnly` | Reject writes through engine authorization | PASS: CREATE, SET, DETACH DELETE, MERGE all return `permission denied: write operations require ReadWrite role` |
| Close/reopen | Retain graph fixture and query through read-only role | PASS |
| Native deadline | Return engine timeout, not arbitrary execution error | PASS: typed `QueryErrorKind::Timeout`, configured 1 ms, observed 204 ms |
| Candidate publication | Exclude readers; close all handles before replacement; reopen candidate | PASS locally, filesystem-only probe described below |
| Bounded result materialization | At most row_limit + 1 = 3 result rows | FAIL: 10,000 already materialized; consuming three changes nothing |
| Scalar/list/object conversion | Recursive JSON, deterministic map order, explicit unsupported errors | PASS for fixture; bytes and non-finite floats explicitly rejected |
| Node/relationship/path conversion | Tags, canonical endpoint IDs, sorted labels/properties, ordered path | Known-typed fixture passes; **generic query conversion BLOCKED** by erased node/object distinction |
| Durable knowledge version | Version travels with the published graph, including empty graphs | **Unresolved representation choice**; prepared-commit metadata does not persist |

### Engine gap 1: bounded role-scoped Cypher results

Probe query: `UNWIND range(1, 10000) AS x RETURN x`. Before any iteration,
`QueryResult::row_count()` returns 10,000. `iter().take(3).count()` returns three,
but the first result already retains all 10,000 rows. This is not a bound on
engine result retrieval.

Exact pinned-source evidence (paths relative to Cargo registry package roots):

| Package/file | Lines | Evidence |
| --- | --- | --- |
| `grafeo-engine-0.5.43/src/session/mod.rs` | 3208-3211, 3262-3274 | Authorization checks the optimized plan; ordinary Cypher uses `make_executor` then `Executor::execute` |
| `grafeo-engine-0.5.43/src/query/executor/mod.rs` | 104-125, 241-254 | Drains `operator.next()` and pushes every row into the result |
| `grafeo-engine-0.5.43/src/database/mod.rs` | 2925-2934, 3133-3134 | `QueryResult` owns a `Vec<Vec<Value>>`; `iter()` is `self.rows.iter()` |
| `grafeo-engine-0.5.43/src/session/mod.rs` | 2862-2903 | Public streaming uses `build_streaming_plan`, explicitly GQL translation, not Cypher |
| `grafeo-engine-0.5.43/src/query/executor/mod.rs` | 191-222 | `execute_with_limit` can cap collected result rows, but is not exposed through the Cypher session path |

No already-public API in the selected `lpg` profile was found that combines
Cypher semantics, role enforcement, native deadline and bounded result
materialization. Database Cypher delegates to the same session; parameterized
Cypher uses the materializing processor. Manually constructing a lower-level
planner/executor would duplicate session authorization/transaction behavior,
not establish a supported session API.

**Smallest concrete remedy to request upstream:** expose a bounded Cypher
session entry point accepting a result cap, keep translation/binding/cache,
native write/admin authorization, transaction context and `make_executor`
deadline unchanged, then call the existing `execute_with_limit` in the final
read-query path. Include parameterized Cypher and explicit EXPLAIN/PROFILE/SHOW
behavior so special paths cannot bypass the result bound. Tests must demonstrate
the cap before materialization, authorization rejection, timeout propagation,
aggregation/order/union semantics and truncation detection. No patch was made.

This caps **returned result materialization**, not overall operator memory:
sorting, aggregation, range construction and joins can still retain working
data. A row cap is not a general query memory guarantee. Query text was not
rewritten, and no `LIMIT` was injected. Explicit specification relaxation or an
approved engine patch/component decision are alternatives for the owner, not
implicit implementation choices.

### Engine gap 2: semantic value tags

The fixture returns a node as `Value::Map`, a relationship as `Value::Map`, and
the node/relationship/path result columns as `LogicalType::Any`. The node map
contains `_id`, `_labels` and ordinary properties. This legal object literal
produces an exactly equal value **and an equal column type**:

```cypher
MATCH (a:Entity {entity_id:1})
RETURN a,
  {_id:0, _labels:['Entity'], entity_id:1, id:'entity:1', name:'Atlas'} AS object
```

The executable asserts both equalities. Therefore generic conversion cannot
decide whether to emit a tagged node or a plain JSON object from the returned
value/type. Key-name heuristics would silently misserialize valid objects.
Source: `grafeo-core-0.5.43/src/execution/operators/project.rs:429-444`
(`node_to_map`) and `451-472` (`edge_to_map`). Both erase graph identity into maps.

`serialization.rs` intentionally converts only the known positions of the
literal fixture. It demonstrates hydration through graph IDs, sorted labels
and properties, relationship endpoints `entity:1`/`entity:2`, and ordered path
arrays. It is **not** a production codec or an arbitrary-Cypher workaround.
Unsupported bytes/non-finite floats fail instead of being stringified; debug
strings in the probe report are diagnostic evidence only.

**Smallest concrete remedy:** preserve unambiguous graph value tags through
projection, including elements nested in lists/maps and paths, and expose them
to Rust callers. Merely giving an outer column a node type is insufficient for
mixed/nested values. No provider change, query parser, reserved map keys or
fallback serializer was added.

### Version carrier and publication compatibility

No compliant carrier has been selected. `PreparedCommit::set_metadata` looks
promising but is only an in-memory `HashMap`: pinned
`grafeo-engine-0.5.43/src/transaction/prepared.rs:104-127` stores the map on the
prepared object and `commit()` calls `session.commit()` without persisting it.
The probe sets `knowledge_version` to a distinctive sentinel, commits, closes,
reopens, and confirms the exported snapshot does not contain the sentinel.

Native file metadata is engine state, not a safe application field:
`grafeo-storage-0.5.43/src/file/format.rs` defines iteration/checksum/snapshot
length/epoch/transaction ID/counts/timestamp. Snapshot fields in
`grafeo-engine-0.5.43/src/database/persistence.rs:27-48` and catalog section
fields in `database/catalog_section.rs:25-40` contain schema/index/epoch state,
not arbitrary durable application metadata. Do not overload engine epoch or
transaction IDs as SQLite `knowledge_version`.

This is an **approved representation decision still needed**, not necessarily a
requirement to add an upstream API. Concrete choices to assess are native
application metadata; an explicitly specified graph-internal carrier; or an
explicitly specified external envelope/carrier published atomically with the
graph. A reserved node changes wildcard query results and the specified complete
projection, including an otherwise empty graph. An independent sidecar plus two
renames does not atomically couple graph and version. Neither was implemented
or presented as passing. Any external carrier needs a crash/rollback publication
design and owner approval before it is used.

The demonstrated **probe layout** preserves today's configured current path:

```text
graph/current.grafeo
graph/candidate.grafeo
graph/publication.lock
```

Candidate construction happens before exclusive activation. A shared lock held
over open/query/result-close rejects an exclusive acquisition by another open
file handle; after dropping results and sessions, calling `close()`, dropping
the database and releasing the shared lock, exclusive acquisition succeeds.
Both closed graph files have no WAL sidecar. `rename(candidate, current)`,
directory sync, lock release and fresh shared-open return the candidate's data.
The publication lock is a separate stable file, never the graph inode being
replaced. This is a same-host handle-lifetime/filesystem proof, **not** proof of
SQLite-coordinated rollback, metadata parity, process-crash recovery, or Windows
replacement semantics.

The extra temporary path and lock alone do not require changing `config.json`.
However existing initialized stores have no demonstrated version carrier;
opening one cannot silently infer `knowledge_version = 0`, mutate it, migrate
it, or delete/rebuild it. Compatibility remains unresolved until the carrier
and explicit rejection/rebuild behavior are approved. P2 changes no real store.

### Recommendation and handoff

Keep local inference work moving after P1. Keep P5a blocked on the two engine/API
gaps and the separately approved durable version representation. Prefer an
upstream bounded read API and semantic value tags before considering an
explicitly approved narrow engine change; do not introduce a second backend.
The specification owner must decide any relaxation. This bounded investigation
stops here; no upstream work, production fixes or normative edits are included.

## Historical standalone platform evaluation

The following records earlier standalone packaging, not the P2 graph gate or
combined inference acceptance. References to former database-level read-only
and external-cancellation requirements are historical; current specifications
require `Role::ReadOnly` and process-level CLI cancellation.

## Outcome

**Selected for Commonplace.** Packaging passes on every intended target:
macOS arm64, Windows x64 and arm64, and Linux x64 and arm64.

The standalone Windows x64 executable ran on an independent Windows 11 Pro x64
machine without Rust or any other dependency installed. Native GitHub-hosted
runners then built and executed Windows arm64, Linux x64, and Linux arm64
artifacts. Cypher projection, canonical evidence properties, persistence, and
role-scoped read-only sessions passed on every tested target.

Grafeo therefore demonstrates that a small, self-contained, cross-platform
Rust graph package is practical. Commonplace uses native role-scoped read-only
sessions rather than Grafeo's ineffective database-level read-only setting.
Query deadlines produce timeout errors on every target where the corrected
probe exceeded its deadline, although cancellation latency varies
substantially. The synchronous CLI uses process termination for user
cancellation and does not promise cancellation of one query while keeping the
process alive.

## Evaluated component

| Component | Selection |
|---|---|
| Grafeo | 0.5.43 |
| Cargo feature profile | `lpg` with default features disabled |
| Rust | 1.98.1, edition 2024 |
| Query language | Cypher |
| Storage | Embedded persistent Grafeo database |

The spike source, lockfile, platform verifier, and build workflow live under
`spikes/grafeo-evaluation/` and `.github/workflows/grafeo-windows-spike.yml`.

## Capability results

| Capability | Result | Evidence |
|---|---|---|
| Cypher node and relationship projection | Pass | A relationship query returned both canonical entity IDs, the knowledge item ID, predicate, and evidence passage ID. |
| Persistent close and reopen | Pass | Reopening the graph returned both stored entities on macOS and Windows. |
| Role-scoped read-only session | Pass | `Role::ReadOnly` rejected the tested Cypher `SET` mutation. |
| Database-level read-only configuration | Fail | A database opened with `Config::read_only` accepted the same Cypher `SET` mutation without returning an error. |
| Query deadline | Partial | The corrected probe returned Grafeo's timeout error, but elapsed time ranged from 199 ms on macOS arm64 to almost 9 seconds on Linux x64 for a 1 ms deadline. |
| Explicit external cancellation | Fail | Grafeo 0.5.43 exposes query deadlines but no public interrupt or cancellation handle that another task can invoke. |

Database-level read-only mode and role-scoped authorization are distinct
controls. The passing role check does not satisfy Commonplace's requirement
that the graph itself be opened in a mode that natively rejects mutations.

A deadline also does not replace explicit cancellation. A cancellation token
in Commonplace cannot interrupt `execute_cypher()` because Grafeo exposes no
handle for it. For the synchronous CLI, process termination can stop a
user-cancelled command, but a future long-running server would need an upstream
API, a fork, or process-isolated queries.

## Windows package evidence

GitHub Actions run
[`36367904936`](https://github.com/markgar/commonplace/actions/runs/36367904936)
built and smoke-tested the `x86_64-pc-windows-msvc` artifact. The downloaded
artifact was then run independently on:

```text
Windows 11 Pro
Version 10.0.26300
x64
```

Observed executable:

```text
Bytes: 6749184
SHA-256: CAA5E6D0596B1CF811F1C1F6ADEDB105C045A1396F91FA45ECC71F770A430F9A
```

The executable launched and completed without Rust, Cargo, Visual Studio,
OpenSSL, Python, or another runtime being installed for the test. No
SmartScreen warning, missing-DLL error, permission error, persistence error, or
temporary-file cleanup error occurred. `dumpbin.exe` was not available, so the
independent machine did not produce a DLL dependency listing.

The GitHub-hosted Windows run reported a timeout after 65 ms. The independent
machine completed the original stress query in 49 ms, one millisecond below
the configured deadline, and therefore correctly reported no timeout. This is
a test-workload variance rather than evidence that the executable failed to
launch or package correctly.

## Additional platform evidence

Manual GitHub Actions run
[`36369182949`](https://github.com/markgar/commonplace/actions/runs/36369182949)
built and executed the remaining supported targets once. The completed
artifacts were:

| Target | Bytes | SHA-256 | Packaging result |
|---|---:|---|---|
| Windows arm64 | 5,680,128 | `019145C6BCE982E6E99F5109C944100FB7694651472ABBAF795DA9ACE2B352DA` | Pass |
| Linux x64 | 8,966,272 | `b08cef73d39fce4145b3e3372dbe19b0b4f879674a6de23b6fc17bc9a6a7f6e0` | Pass |
| Linux arm64 | 8,209,840 | `37eb46f1dae2a53b885c07fb678e8400e1a2fea48efed85a68b49dd1d8efd2d7` | Pass |

Windows arm64 completed the full probe natively on
`windows-11-vs2026-arm`. Both Linux binaries completed the full probe with an
empty environment. Their only dynamic dependencies were the platform loader,
glibc, `libm`, `libgcc_s`, and the virtual dynamic shared object.

The corrected 1 ms timeout probe returned a Grafeo timeout error on all three
targets, but with materially different elapsed times:

| Target | Elapsed time |
|---|---:|
| Windows arm64 | 821 ms |
| Linux arm64 | 4,090 ms |
| Linux x64 | 8,975 ms |

The Linux runs therefore prove eventual interruption, not close adherence to
the configured deadline. The spike should not represent query duration as a
hard upper bound without another containment mechanism.

## macOS arm64 package evidence

The release build on macOS arm64 produced an approximately 6.9 MB executable.
Its dynamic dependencies were limited to macOS system libraries:

```text
/usr/lib/libiconv.2.dylib
/usr/lib/libSystem.B.dylib
```

This is materially simpler than the Ladybug package, which required bundled
OpenSSL libraries and a much larger native dependency chain.

## Decision

Grafeo is the strongest current candidate for Commonplace because it preserves
embedded Cypher while packaging cleanly on every intended OS and architecture.
Public Cypher must always use `Role::ReadOnly`; `Config::read_only` cannot be
relied on for mutation rejection in version 0.5.43.

Select Grafeo 0.5.43 for the current synchronous CLI with these boundaries:

- public Cypher always executes through `Role::ReadOnly`;
- configured deadlines are operational budgets, not precise wall-clock limits;
- Ctrl+C terminates the CLI process rather than cancelling an individual query;
  and
- a future long-running service must reevaluate cancellation before reusing the
  embedded query path.

No second graph backend is introduced.
