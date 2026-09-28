#!/usr/bin/env sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
executable="$script_dir/grafeo-evaluation"

if [ ! -x "$executable" ]; then
  echo "Missing executable: $executable" >&2
  exit 1
fi

data_dir=$(mktemp -d "${TMPDIR:-/tmp}/commonplace-grafeo.XXXXXX")
trap 'rm -rf "$data_dir"' EXIT INT TERM

env -i \
  PATH="/usr/bin:/bin" \
  COMMONPLACE_GRAFEO_DATA_DIR="$data_dir" \
  "$executable"

echo
echo "Executable metadata:"
wc -c "$executable"

if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$executable"
else
  shasum -a 256 "$executable"
fi

file "$executable"

if command -v ldd >/dev/null 2>&1; then
  echo
  echo "Dynamic dependencies:"
  ldd "$executable"
elif command -v otool >/dev/null 2>&1; then
  echo
  echo "Dynamic dependencies:"
  otool -L "$executable"
fi
