#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
archive="$root/target/dist/commonplace-rust-spike-macos-arm64.tar.gz"
work=$(mktemp -d "${TMPDIR:-/tmp}/commonplace-spike.XXXXXX")
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/home"
tar -C "$work" -xzf "$archive"
bundle="$work/commonplace-rust-spike-macos-arm64"

codesign --verify --deep --strict "$bundle/rust-packaging"

if otool -L "$bundle/rust-packaging" "$bundle/lib/"*.dylib | grep -q /opt/homebrew; then
    echo "bundle still contains a Homebrew dynamic-library path" >&2
    exit 1
fi

mkdir -p "$work/non-inference-data"
env -i \
    PATH=/usr/bin:/bin \
    HOME="$work/home" \
    TMPDIR="$work" \
    HF_HUB_OFFLINE=1 \
    HTTP_PROXY=http://127.0.0.1:9 \
    HTTPS_PROXY=http://127.0.0.1:9 \
    ALL_PROXY=http://127.0.0.1:9 \
    COMMONPLACE_SPIKE_DATA_DIR="$work/non-inference-data" \
    "$bundle/rust-packaging" sqlite >/dev/null
env -i \
    PATH=/usr/bin:/bin \
    HOME="$work/home" \
    TMPDIR="$work" \
    HF_HUB_OFFLINE=1 \
    HTTP_PROXY=http://127.0.0.1:9 \
    HTTPS_PROXY=http://127.0.0.1:9 \
    ALL_PROXY=http://127.0.0.1:9 \
    COMMONPLACE_SPIKE_DATA_DIR="$work/non-inference-data" \
    "$bundle/rust-packaging" ladybug >/dev/null

env -i \
    PATH=/usr/bin:/bin \
    HOME="$work/home" \
    TMPDIR="$work" \
    HF_HUB_OFFLINE=1 \
    HTTP_PROXY=http://127.0.0.1:9 \
    HTTPS_PROXY=http://127.0.0.1:9 \
    ALL_PROXY=http://127.0.0.1:9 \
    "$bundle/rust-packaging" all
