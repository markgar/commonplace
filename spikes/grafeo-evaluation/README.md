# Grafeo evaluation spike

This disposable harness evaluates Grafeo as a cross-platform replacement for
LadybugDB. It tests the Commonplace graph shape, Cypher queries, persistence,
read-only enforcement, and query deadlines.

It intentionally reports unsupported requirements instead of hiding them.
The P2 `preflight` mode proves the selected role/deadline/publication behavior
and reproduces bounded-materialization and semantic-value-tag blockers. The
durable version carrier still needs an approved representation decision.
Current CLI cancellation is process termination, not an external engine handle.

The canonical findings and platform evidence are recorded in
[the Grafeo evaluation report](../../docs/spikes/grafeo-evaluation.md).

## macOS or Linux

```sh
COMMONPLACE_GRAFEO_DATA_DIR="$(mktemp -d /tmp/commonplace-graph-p2.XXXXXX)" \
  cargo run --manifest-path spikes/grafeo-evaluation/Cargo.toml \
  --release --locked --offline -- preflight
```

The JSON `gate: blocked` is intentional even when diagnostics exit zero.
`spikes/rust-packaging` runs this exact source in the combined executable.
The old no-argument mode remains historical platform-probe evidence.
New P2 execution is verified only on macOS arm64.

## Windows PowerShell

Historical Windows artifacts included the following verifier. Do not dispatch
or enable GitHub Actions; current validation policy is local-only. No new Windows
run is claimed by P2. From an already available extracted historical artifact:

```powershell
.\verify-windows.ps1
```

Unix artifacts include `verify-unix.sh`, which runs the adjacent executable
with an empty environment and reports its hash and dynamic dependencies:

```sh
./verify-unix.sh
```
