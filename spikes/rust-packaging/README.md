# Rust packaging spike

This disposable harness tests the native dependency chain selected for
Commonplace. It is evidence for the technology gate, not the production
application architecture.

From this directory:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
export LBUG_VERSION=0.20.4

cargo build --release --locked
COMMONPLACE_SPIKE_DATA_DIR="$PWD/target/spike-data" \
  ./target/release/rust-packaging all
./package-macos.sh
./verify-macos.sh
```

The first model run downloads model artifacts. Later runs use the cache under
`target/spike-data/`; `verify-macos.sh` disables network access through an
unreachable proxy to prove the packaged cache is sufficient.

The generated release bundle and archive are ignored with `target/`.
