#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cache=${COMMONPLACE_MODEL_CACHE:?"set COMMONPLACE_MODEL_CACHE to a prepared pinned cache"}
dist="$root/target/dist/commonplace-rust-spike-macos-arm64"
archive="$root/target/dist/commonplace-rust-spike-macos-arm64.tar.gz"

test "$(uname -m)" = arm64
test "$(uname -s)" = Darwin
test ! -e "$dist" || { echo "refusing to overwrite $dist" >&2; exit 1; }
test ! -e "$archive" || { echo "refusing to overwrite $archive" >&2; exit 1; }
cd "$root"
cargo build --release --locked --offline
mkdir -p "$dist"
cp target/release/rust-packaging "$dist/"
cp -R "$cache" "$dist/pinned-models"
codesign --force --sign - "$dist/rust-packaging"
tar -C "$(dirname "$dist")" -czf "$archive" "$(basename "$dist")"
echo "$archive"
