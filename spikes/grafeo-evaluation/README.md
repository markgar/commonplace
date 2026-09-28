# Grafeo evaluation spike

This disposable harness evaluates Grafeo as a cross-platform replacement for
LadybugDB. It tests the Commonplace graph shape, Cypher queries, persistence,
read-only enforcement, and query deadlines.

It intentionally reports unsupported requirements instead of hiding them.
Grafeo 0.5.43 exposes query timeouts but no public external interruption or
cancellation handle, so the result cannot be a full pass under the current
Commonplace graph contract.

## macOS or Linux

```sh
cargo run --manifest-path spikes/grafeo-evaluation/Cargo.toml --release
```

## Windows PowerShell

Download and extract the `grafeo-windows-x64` artifact produced by the
`Grafeo Windows spike` GitHub Actions workflow. No Rust toolchain is required
on the test machine. From the extracted directory:

```powershell
.\verify-windows.ps1
```
