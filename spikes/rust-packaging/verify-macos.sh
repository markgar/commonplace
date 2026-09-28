#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
archive="$root/target/dist/commonplace-rust-spike-macos-arm64.tar.gz"
work=$(mktemp -d "${TMPDIR:-/tmp}/commonplace-spike.XXXXXX")
trap 'rm -rf "$work"' EXIT
tar -C "$work" -xzf "$archive"
bundle="$work/commonplace-rust-spike-macos-arm64"
codesign --verify --strict "$bundle/rust-packaging"
if otool -L "$bundle/rust-packaging" | grep -q /opt/homebrew; then
    echo "bundle still contains a Homebrew dynamic-library path" >&2
    exit 1
fi
mkdir "$work/home"
for command in sqlite grafeo all; do
    env -i PATH=/usr/bin:/bin HOME="$work/home" TMPDIR="$work" \
        COMMONPLACE_SPIKE_DATA_DIR="$work/$command" \
        COMMONPLACE_MODEL_CACHE="$bundle/pinned-models" \
        /usr/bin/sandbox-exec -p '(version 1)(allow default)(deny network*)' \
        "$bundle/rust-packaging" "$command"
done
