# Grafeo evaluation spike

This disposable harness evaluates Grafeo as a cross-platform replacement for
LadybugDB. It tests the Commonplace graph shape, Cypher queries, persistence,
read-only enforcement, and query deadlines.

It intentionally reports unsupported requirements instead of hiding them.
Grafeo 0.5.43 exposes query timeouts but no public external interruption or
cancellation handle, so the result cannot be a full pass under the current
Commonplace graph contract.

The canonical findings and platform evidence are recorded in
[the Grafeo evaluation report](../../docs/spikes/grafeo-evaluation.md).

## macOS or Linux

```sh
cargo run --manifest-path spikes/grafeo-evaluation/Cargo.toml --release
```

## Windows PowerShell

Download and extract the `grafeo-windows-x64` artifact produced by the
`Grafeo platform spike` GitHub Actions workflow. No Rust toolchain is required
on the test machine. From the extracted directory:

```powershell
.\verify-windows.ps1
```

Unix artifacts include `verify-unix.sh`, which runs the adjacent executable
with an empty environment and reports its hash and dynamic dependencies:

```sh
./verify-unix.sh
```
