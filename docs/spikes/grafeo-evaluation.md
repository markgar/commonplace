# Grafeo evaluation spike

## Outcome

**Packaging passes on every intended target: macOS arm64, Windows x64 and
arm64, and Linux x64 and arm64. Grafeo 0.5.43 does not yet satisfy the complete
Commonplace graph contract.**

The standalone Windows x64 executable ran on an independent Windows 11 Pro x64
machine without Rust or any other dependency installed. Native GitHub-hosted
runners then built and executed Windows arm64, Linux x64, and Linux arm64
artifacts. Cypher projection, canonical evidence properties, persistence, and
role-scoped read-only sessions passed on every tested target.

Grafeo therefore demonstrates that a small, self-contained, cross-platform
Rust graph package is practical. Database-level read-only enforcement remains
broken, though the native role-scoped control used by public queries works.
Query deadlines produce timeout errors on every target where the corrected
probe exceeded its deadline, but cancellation latency varied substantially.
Grafeo has no public external cancellation handle.

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

Before final selection, Commonplace must make one bounded decision about query
cancellation. For the synchronous CLI, native deadlines plus whole-process
termination may be accepted, with the observed deadline overshoot documented.
If a strict duration bound or in-process cancellation token remains mandatory,
Grafeo needs an upstream fix, a fork, or process-isolated graph queries. No
second graph backend should be introduced.
