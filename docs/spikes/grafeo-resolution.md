# P2 graph resolution follow-up

> **Historical experiment; recommendation superseded.** The user chose unmodified
> Oxigraph rather than maintaining engine patches. The patches, native metadata
> format, and snapshot envelope below are preserved only as investigation evidence,
> never production dependencies or approved persistence formats. Follow the
> [owning specifications](../specification/README.md) and
> [stock Oxigraph evidence](oxigraph-evaluation.md), not the former recommendation.

## Decision summary

**Unpatched Grafeo 0.5.43 GRAPH gate: BLOCKED. Patched local prototype:
PASS for the explicitly enumerated regression suite, not universal Cypher or
production qualification. INFERENCE gate: remains PASS on local macOS arm64**
from the [independent combined proof](rust-packaging.md); inference was not
rerun to manufacture another platform result.

All three original gaps now have positive isolated evidence. The smallest
coherent engine route is to expose existing bounded collection through the
native authorized Cypher session, preserve node/edge identity in recursive
values, and persist candidate metadata with the graph. No supported release
found during this investigation already supplies all three. This work adopts
neither a production fork nor a durable representation.

**Recommendation:** retain Grafeo provisionally and approve completing a narrow,
pinned Grafeo patch/upstream change with **native candidate metadata**, rather
than building a query-rewriting layer or silently introducing an envelope.
Native metadata best preserves the intended native-file/open path. The
envelope is a demonstrated alternative if avoiding a native storage patch is
worth taking ownership of a new application format and snapshot-import lifecycle.
It does not remove the separate bounded-query and semantic-tag engine changes.

The precise next user decision is: **authorize maintained Grafeo patch work and
the native metadata carrier direction, including explicit incompatible-store
rejection, before changing production dependencies or persistence contracts.**
That is approval to finish the production acceptance work listed below, not
approval to ship this experimental patch as-is. If maintaining an engine patch
is unacceptable, only upstream adoption of equivalent capabilities resolves the
current contract without reopening the selected-provider decision. No external
submission, merge, release, root manifest change, or specification change was made.

## Scope, source identities, and reproducibility

Local host: macOS 27.0 build 26A428, arm64; rustc 1.98.1
(`48a229cea 2026-09-01`), Cargo 1.98.1. Synthetic fixtures only.
The original P2 commit is `4de610e2056df51c000cc569252929d767ec1095`.
This follow-up lives entirely under `spikes/` and `docs/spikes/`.

The exact patch is
[`spikes/grafeo-evaluation/resolution/engine.patch`](../../spikes/grafeo-evaluation/resolution/engine.patch).
It modifies **12 Rust files in three 0.5.43 crates, +132/-154 lines**, including
replacement of two projection assertions. It is not a new backend.

| Patched crate | Pristine crates.io archive SHA-256 | Scope |
| --- | --- | --- |
| grafeo-common 0.5.43 | `780d7fb460139e5ba9d6062d679893b24c724129a174710bd62fd45dc51157d4` | Two semantic value variants; type names, display, hashing, size/order handling |
| grafeo-core 0.5.43 | `2c746c01145ab3c1a3a368c15012e1fdfc608c5d6cd27e158330c12d1f2d730e` | Typed/generic vectors, projection, mutation ID consumers, path construction/functions, join keys |
| grafeo-engine 0.5.43 | `d6beb48085a410e875eb52dec6c7a18ad6e5e1e650ea21c457ccf40624120ca5` | Authorized bounded session entry point, catalog metadata, native/snapshot serialization |

The facade, adapters, and storage crates remain 0.5.43; their dependencies are
fixed by the isolated Cargo.lock. The patch SHA-256 is
`a6f37c48cad6e65a43d56cf11611e5179f70a94c3c87696093837ff9c58aa3a2`.
The [manifest](../../spikes/grafeo-evaluation/resolution/patch-manifest.json)
also records every modified file's before/after digest. Preparation verifies
already-cached crate archives before extraction, applies a dry-run-checked
patch, and verifies the resulting source bytes. It never downloads dependencies,
modifies registry sources, or overwrites an existing vendor directory.

Exact commands are in the
[harness README](../../spikes/grafeo-evaluation/resolution/README.md).
Validation used fresh reconstruction from the persistent patch, release
integration tests, Clippy with warnings denied for the harness, formatting, and
a release execution under OS network denial. The ordinary test is deterministic
about behavior; elapsed times/RSS are observations only. No model weights are
needed by this follow-up. The original real-model ignored/cache invocation
convention is unchanged.

Full structured evidence:
[`grafeo-resolution-results.json`](grafeo-resolution-results.json).
Recorded release executable SHA-256:
`202660bc064aa28e840eab1bfd52f7d09b2987967d78abb75203b349631122c8`.

## Existing API and upstream investigation

The [original report](grafeo-evaluation.md) retains the unpatched reproduction
and exact source locations. The current published release observed was
[v0.5.43](https://github.com/GrafeoDB/grafeo/releases/tag/v0.5.43).
Inspected upstream main was
[`8295a5447c8bfa6a7cd0069194c5c0306112672b`](https://github.com/GrafeoDB/grafeo/tree/8295a5447c8bfa6a7cd0069194c5c0306112672b).
The open release work, GrafeoDB/grafeo#496, was inspected at
[`e970e21c7fce212631e0a865783defb1b1c01452`](https://github.com/GrafeoDB/grafeo/tree/e970e21c7fce212631e0a865783defb1b1c01452).
Its path-property/persistence fixes did not expose bounded role-scoped Cypher or
restore erased semantic value tags. These are point-in-time observations,
not a claim about future releases.

`Executor::execute_with_limit` already exists in
`grafeo-engine/src/query/executor/mod.rs:191-222`. Calling it directly does not
provide the required native session authorization. The public streaming route
uses the GQL translator, not Cypher. The prototype therefore exposes a small
session method that uses the native Cypher translator, native permissions,
parameter substitution, binder, optimizer, active graph store, transaction
context, physical planner, and deadline-configured executor. No string
rewriting or injected Cypher `LIMIT` is involved.

**Canonical projection is useful for a known fixed query, not a generic fix.**
Explicit `labels(a)`/`properties(a)` projections can construct a chosen output
object where the caller already knows `a` is a node. However, the existing
node/object collision proves that the public materialized value plus column
type is not an injective representation of semantic identity. A serializer
cannot recover information that was erased. Mixed lists/maps, CASE branches,
UNWIND values, aliases, and UNION arms require per-value provenance, not a
column-name heuristic. Rewriting arbitrary supported Cypher into canonical
projections would require its own semantic/type-aware transformation and
coverage of nested expressions. No supported public API found supplies that
generic transformation. The demonstrated native tags address the actual loss
point instead.

## Bounded result and query probes

`resolution/src/bounds.rs` drives the same existing bounded executor with a
counted source that fails if pulled past the expected budget:

| Chunk supplied by operator | Final cap | `next()` calls | Retained final rows |
| --- | --- | --- | --- |
| 1 row | 3 | 3 | 3 |
| 1,024 rows | 3 | 1 | 3 |
| 1,024 rows | 0 | 0 | 0 |

**The 1,024-row case computes/supplies a 1,024-row chunk, not just three rows.**
Only final row collection is capped at three. Sort, aggregate, UNWIND range
construction, join, and other operators may still retain larger intermediate
state. This is evidence of bounded final result retention, not a whole-query
memory or maximum-rows-computed guarantee. The result iterator still traverses
an eagerly materialized `Vec`; the change is that the Vec is populated with at
most the requested cap, not truncated after exhaustive execution.

Through `Role::ReadOnly`, six bounded-vs-unbounded result-prefix comparisons
pass: 10,000-row range, descending ORDER BY, DISTINCT, aggregate sum,
UNION ALL, and parameterized ORDER BY/SKIP/LIMIT. Cap is three (the fixture's
row limit two plus one). Literal/default parameters and repeated bindings 7/19
work without cache contamination. CREATE, SET, DETACH DELETE, and MERGE are
rejected by native permission checks. A parameterized expensive aggregate
returns native `Error::Query` with `QueryErrorKind::Timeout` at a configured
1 ms deadline; this is not a promise of 1 ms wall-clock termination.

EXPLAIN/PROFILE are explicitly rejected by this prototype; the assertions
verify that rejection instead of claiming semantic equivalence. Non-plan
commands, including SHOW/schema commands, are also not a complete bounded
implementation. Those must be wired through their proper native permission
and bounded-result paths before general production use. The prototype bypasses
the query-plan cache deliberately; upstream integration should share existing
execution plumbing rather than retain a duplicate session path.

## Semantic result probes

`resolution/src/query.rs` verifies actual `Value::Node`/`Value::Edge` variants
before recursive encoding. These are distinct from Int64 and ordinary Map
values. The serializer resolves properties while the graph handle/publication
lease is alive; labels are sorted, maps ordered, endpoint canonical IDs required,
and unsupported/non-finite values are explicit errors.
The fixture codec's `kind` fields demonstrate semantic preservation; they do
not introduce a new production JSON contract.

Passing assertions cover direct nodes/edges, aliases, mixed nested lists and
maps, an ordinary node-lookalike object left untouched, a path's ordered tagged
components, `nodes(p)`, `relationships(p)`, `startNode`/`endNode`, WITH/property
access, collect, DISTINCT over mixed nodes/edges/scalars/maps, node/edge/scalar
equality, mixed CASE branches, and a node/edge UNION ALL. Date and NaN conversion
are rejected rather than serialized misleadingly.

The regression work caught and fixed two concrete propagation defects:
mutation operators still expected Int64 after vectors gained tags, and path
construction/functions still emitted integer IDs. This is why changing only
the final `node_to_map` helper was insufficient. Hash join keys also preserve
node-versus-edge identity rather than falling through to null keys.

This is a positive but finite suite. It does not certify every language feature,
every expression consumer, graph-valued ORDER BY, nested comprehensions,
procedures, named graph/transaction snapshot resolution, shortest/all-path
variants, push/parallel execution, or other language bindings. The generic
`Value` enum participates in persisted values and other crate APIs; adopting
tags requires upstream-wide enum/serialization and graph-property validation
review. Legacy internal Int64-ID paths remain in the prototype; they are not a
new application compatibility layer or permission to infer semantic tags from
integers in the output codec. Two directly affected projection unit assertions
were updated, but the upstream all-feature test matrix was not run.

## Version carrier comparison and crash behavior

Both fixtures keep `current.grafeo`, `candidate.grafeo`, and a separate stable
`publication.lock` in the same temporary directory. SQLite has the authoritative
version. This is a disposable layout experiment, not approval to replace the
initialized production artifact. No sentinel nodes, hidden entity namespaces,
metadata sidecars, custom reserved sections, or overloaded epoch/header fields
are used.

| Property | Public-API snapshot envelope | Patched native catalog metadata |
| --- | --- | --- |
| Representation | Magic + u64 version + snapshot length + exported bytes + SHA-256 over header/payload | Catalog BTreeMap carried by native catalog section and exported snapshot |
| New engine persistence code | None for the envelope mechanism | Four engine files; catalog section v1 to experimental v2, snapshot v4 to experimental v5 |
| Read lifecycle | Read/checksum entire envelope, import complete snapshot into a fresh graph | `GrafeoDB::open_read_only` on the native file |
| Memory implications | Holds encoded input while importing; import constructs decoded snapshot/store; each reader has an independent imported graph | Normal engine loader also builds graph state in this configuration; avoids the application envelope buffer/import path |
| Write lifecycle | Export complete snapshot; sync complete candidate file; close/drop; atomic rename | Set candidate metadata; save native candidate; close/drop; sync; atomic rename |
| Maintenance owner | Commonplace owns envelope format, validation, size policy, and import lifecycle | Engine owns catalog/snapshot format; application validates required knowledge_version |
| Reader authorization | Imported DB is writable internally, so public query access must remain through native ReadOnly session | Native read-only database plus ReadOnly session; metadata setter refuses read-only DB |
| Missing version | No valid envelope accepted without complete header/checksum | Getter returns None; adapter errors instead of supplying a version |

The envelope implementation uses public export/import APIs, but this comparison
binary links the patched engine for **both** carriers. Its payload is therefore
the experimental v5 snapshot, not proof that unpatched Grafeo understands that
payload. The envelope mechanism does not depend on the metadata patch; its
version is in the checksummed envelope header.

Both carriers pass: empty graph version 1/reopen; two simultaneously open
shared-reader handles excluding an exclusive publisher; handles closed and
dropped before replacement; native read-only rejection; complete candidate
version 2 with three nodes; a successful graph/SQLite publication; and rejection
of a truncated candidate without changing the current graph. Native
export/import also preserves the metadata. An old snapshot marker is rejected.

Each carrier runs two local subprocess crash probes. The child takes the
exclusive publication lock, begins a SQLite transaction setting version 2, and
validates the closed candidate. It then exits 73 without Rust destructors:
before activation the old complete graph/version 1 survives; after graph
rename/directory sync but before SQLite commit, the graph is version 2 while
SQLite rolls back to version 1. The mismatch is detectable and an **explicit**
rebuild from authoritative SQLite restores parity. No silent repair occurs on
open. A second normal transaction activates a validated candidate and commits
SQLite, leaving both at version 2.

This demonstrates process-exit behavior and same-filesystem rename on this
host, **not** atomic two-store commit or hardware-power-loss correctness.
It does not test a retained-old-candidate rollback protocol, every crash window,
fsync fault injection, lock fairness, large-reader pressure, or other filesystems.
The shared-reader probe uses independently opened handles in one process;
crash writers and memory measurements run in separate local processes.

Native metadata is candidate-only in this patch: the setter is not integrated
with session commit/WAL, transactional rollback, or live checkpoint dirty
tracking. Close/save complete candidate publication is the demonstrated scope.
The catalog decoder in stock 0.5.43 ignores the decoded version/trailing bytes,
and its file-open path does not compare the catalog entry version before
deserializing (`database/catalog_section.rs:184-217`,
`database/mod.rs:1573-1582`). Consequently an experimental section-version bump
alone is **not a safe old-reader rejection policy**. Production work needs an
explicit native format/compatibility decision, potentially an engine file-header
version change, and old/new-reader regression probes. Neither silently rewriting
an old artifact nor shipping this prototype format is approved.

### Small local measurements

One recorded run, 2,000 synthetic single-property nodes:

| Measurement | Envelope | Native metadata |
| --- | --- | --- |
| Artifact bytes | 58,463 | 70,715 |
| Snapshot bytes | 58,407 | 58,427 |
| Fresh child open/import time | 1,057 us | 1,123 us |
| Child maximum resident size | 6,651,904 bytes | 6,848,512 bytes |
| Build time | 39.07 ms | 29.99 ms |

RSS comes from local macOS `/usr/bin/time -l`; the measured child opens the
graph, exports one snapshot to count bytes, and closes it. It includes process,
engine, and export allocations, not just graph storage. This is a tiny warm-cache
fixture, no benchmark distribution, and no evidence that native is faster or
uses less memory. The five-iteration `open_verify_ms` in the JSON additionally
performs query/auth checks; the native branch also checks metadata round-trip,
so that metric must not be treated as a like-for-like loader benchmark.
The architectural difference is the envelope's mandatory whole-snapshot
buffer/import path and application format ownership, not these noisy timings.

## Gate and target matrix

| Target | Unpatched combined inference | Unpatched graph | Patched resolution suite |
| --- | --- | --- | --- |
| macOS arm64, this local host | PASS, original P2 evidence | BLOCKED, unchanged | PASS for cases above |
| macOS x64 | UNVERIFIED | UNVERIFIED | UNVERIFIED |
| Linux x64 / arm64 | UNVERIFIED | UNVERIFIED | UNVERIFIED |
| Windows x64 / arm64 | UNVERIFIED | UNVERIFIED | UNVERIFIED |

Historical standalone runs do not count as combined evidence. Local passing
inference can unblock local P3/P4; this prototype does not silently unblock
production P5a. There were no Actions runs/changes, remote runners, other
machines, production/provider substitutions, or external upstream submissions.
