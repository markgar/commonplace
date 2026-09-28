# Disposable P2 resolution prototype

**Superseded, historical experiment.** The user chose stock Oxigraph, not a
maintained Grafeo patch or either experimental durable format. Nothing in this
directory is approved for production adoption. The retained patch and fixtures
document why the investigation moved to [Oxigraph](../../oxigraph-evaluation/README.md).

**Evidence only. Not a supported Grafeo release, production dependency, or
approved durable format.** The unpatched graph gate remains blocked.
The [resolution report](../../../docs/spikes/grafeo-resolution.md) separates
passing local probes from remaining adoption requirements.

This independent Cargo project patches exactly `grafeo-common`, `grafeo-core`,
and `grafeo-engine` 0.5.43 under its ignored `target/vendor/`. No shared Cargo
source, parent harness, root dependency, specification, or workflow is modified.
The patch demonstrates bounded native Cypher collection, semantic graph value
tags, and candidate-only catalog metadata. The executable also compares a
public-API snapshot-envelope carrier using synthetic disposable fixtures.

## Reproduce on local macOS arm64

Use Python 3.12+, the existing local Rust toolchain, `patch`, macOS
`/usr/bin/time`, and already-cached crates. Missing/incompatible archives are
errors, never downloads or skipped tests. Set `--cache` to the actual existing
registry cache, not a project checkout.

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
python3 spikes/grafeo-evaluation/resolution/prepare.py \
  --cache "$HOME/.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f"
cargo test --manifest-path spikes/grafeo-evaluation/resolution/Cargo.toml \
  --release --locked --offline
cargo clippy --manifest-path spikes/grafeo-evaluation/resolution/Cargo.toml \
  --release --locked --offline --all-targets -- -D warnings
sandbox-exec -p '(version 1)(allow default)(deny network*)' \
  spikes/grafeo-evaluation/resolution/target/release/grafeo-resolution-prototype \
  > spikes/grafeo-evaluation/resolution/target/probe.json
```

Preparation intentionally refuses an existing `target/vendor/`; do not rerun it
over working sources. The recorded reproduction preserved the working copy,
prepared fresh sources from verified `.crate` archives, verified every patched
file digest, and reran the tests. `--capture` is a maintainer-only command that
regenerates `engine.patch` and `patch-manifest.json` from an intentional local
prototype edit. Review both before committing.

One integration test runs all query, counted-executor, publication, process-exit,
and version regressions. It creates temporary directories and launches local
child processes; the children deliberately exit 73 during two crash scenarios.
The private `crash` and `measure` executable modes exist only for that harness.
Nothing downloads models, starts a service, dispatches Actions, or runs remotely.
No other platform is verified by this test.

The JSON results are preserved in
[`docs/spikes/grafeo-resolution-results.json`](../../../docs/spikes/grafeo-resolution-results.json).
Timings/RSS are host observations, not acceptance thresholds or capacity claims.

## Attribution

`engine.patch` contains clearly marked local modifications to
[Grafeo v0.5.43](https://github.com/GrafeoDB/grafeo/tree/v0.5.43),
licensed Apache-2.0. `UPSTREAM-LICENSE` is the upstream license from that tag.
The pristine crate archives and their original notices remain unmodified.
`patch-manifest.json` records the exact upstream package checksums, modified
paths, before/after SHA-256 values, and patch identity.
