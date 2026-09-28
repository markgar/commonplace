#!/bin/bash
set -euo pipefail

cache="${1:?Pass the already-prepared immutable model cache directory}"
spike="$(cd "$(dirname "$0")" && pwd)"
test -d "$cache"
test -x "$spike/target/release/oxigraph-evaluation"
run="$(mktemp -d "$spike/target/relocation.XXXXXX")"
mkdir -p "$run/assembly/bin" "$run/empty-home"
cp "$spike/target/release/oxigraph-evaluation" "$run/assembly/bin/"
cp -R "$cache" "$run/assembly/models"
mv "$run/assembly" "$run/relocated"

sandbox-exec -p '(version 1)(allow default)(deny network*)' /usr/bin/env -i \
  PATH=/usr/bin:/bin \
  HOME="$run/empty-home" \
  COMMONPLACE_MODEL_CACHE="$run/relocated/models" \
  COMMONPLACE_SPIKE_DATA_DIR="$run/data" \
  "$run/relocated/bin/oxigraph-evaluation" all > "$run/results.json"

otool -L "$run/relocated/bin/oxigraph-evaluation"
shasum -a 256 "$run/relocated/bin/oxigraph-evaluation"
printf 'Evidence: %s/results.json\n' "$run"
