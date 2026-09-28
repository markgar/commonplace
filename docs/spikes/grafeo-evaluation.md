# Grafeo evaluation spike

## Outcome

**Packaging passes on macOS arm64 and Windows x64, but Grafeo 0.5.43 does not
satisfy the complete Commonplace graph contract.**

The standalone Windows executable ran on an independent Windows 11 Pro x64
machine without Rust or any other dependency installed. Cypher projection,
canonical evidence properties, persistence, and role-scoped read-only sessions
passed. Database-level read-only enforcement and explicit external query
cancellation failed.

Grafeo therefore demonstrates that a small, self-contained, cross-platform
Rust graph package is practical, but it is not the production graph engine
unless the missing behaviors are added or the Commonplace requirements change.
The graph-engine technology gate remains open.

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
| Query deadline | Pass, with probe correction required | The GitHub Windows runner returned Grafeo's timeout error. The initial independent-machine query finished at the 50 ms boundary, showing that the original workload was not deterministic across machines. The harness now isolates this check with a 1 ms deadline and a larger workload. |
| Explicit external cancellation | Fail | Grafeo 0.5.43 exposes query deadlines but no public interrupt or cancellation handle that another task can invoke. |

Database-level read-only mode and role-scoped authorization are distinct
controls. The passing role check does not satisfy Commonplace's requirement
that the graph itself be opened in a mode that natively rejects mutations.

A deadline also does not replace explicit cancellation. Commonplace needs to
stop an executing graph query when its caller is cancelled, not only when a
preconfigured duration expires.

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

## macOS package evidence

The release build on macOS arm64 produced an approximately 6.9 MB executable.
Its dynamic dependencies were limited to macOS system libraries:

```text
/usr/lib/libiconv.2.dylib
/usr/lib/libSystem.B.dylib
```

This is materially simpler than the Ladybug package, which required bundled
OpenSSL libraries and a much larger native dependency chain.

## Decision

Do not select Grafeo 0.5.43 as the Commonplace graph engine under the current
requirements. Its packaging characteristics are strong, and its Cypher and
persistence behavior cover the core data model, but accepting mutations in
database-level read-only mode and lacking external cancellation are release
blockers.

Continue using the Commonplace graph abstraction while the technology gate is
open. A later Grafeo version can be reevaluated if it provides both native
database-level mutation rejection and a public cancellation mechanism. Avoid
shipping parallel graph backends or weakening these requirements solely to
adopt Grafeo.
