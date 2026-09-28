#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
target="$root/target/release"
dist="$root/target/dist/commonplace-rust-spike-macos-arm64"
archive="$root/target/dist/commonplace-rust-spike-macos-arm64.tar.gz"
cache="$root/target/spike-data/fastembed-cache-jina"
openssl_prefix=${OPENSSL_PREFIX:-/opt/homebrew/opt/openssl@3}
embedding_revision=8f518e882455312b086101e60691f5e6e2f05c3c
reranker_revision=b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2

cd "$root"

if [ ! -d "$cache" ]; then
    echo "model cache is missing; run './target/release/rust-packaging models' first" >&2
    exit 1
fi

actual_embedding_revision=$(cat "$cache/models--Qdrant--all-MiniLM-L6-v2-onnx/refs/main")
actual_reranker_revision=$(cat "$cache/models--jinaai--jina-reranker-v1-turbo-en/refs/main")
if [ "$actual_embedding_revision" != "$embedding_revision" ]; then
    echo "unexpected embedding model revision: $actual_embedding_revision" >&2
    exit 1
fi
if [ "$actual_reranker_revision" != "$reranker_revision" ]; then
    echo "unexpected reranker model revision: $actual_reranker_revision" >&2
    exit 1
fi

LBUG_VERSION=0.20.4 cargo build --release --locked

rm -rf "$dist"
mkdir -p "$dist/lib" "$dist/data"
cp "$target/rust-packaging" "$dist/"
cp "$openssl_prefix/lib/libssl.3.dylib" "$dist/lib/"
cp "$openssl_prefix/lib/libcrypto.3.dylib" "$dist/lib/"
cp -R "$cache" "$dist/data/fastembed-cache-jina"

binary_ssl_dep=$(otool -L "$dist/rust-packaging" | awk '/libssl\.3\.dylib/{print $1; exit}')
binary_crypto_dep=$(otool -L "$dist/rust-packaging" | awk '/libcrypto\.3\.dylib/{print $1; exit}')
ssl_crypto_dep=$(otool -L "$dist/lib/libssl.3.dylib" | awk '/libcrypto\.3\.dylib/{print $1; exit}')

install_name_tool \
    -change "$binary_ssl_dep" \
    "@executable_path/lib/libssl.3.dylib" \
    -change "$binary_crypto_dep" \
    "@executable_path/lib/libcrypto.3.dylib" \
    "$dist/rust-packaging"
install_name_tool \
    -id "@loader_path/libssl.3.dylib" \
    -change "$ssl_crypto_dep" \
    "@loader_path/libcrypto.3.dylib" \
    "$dist/lib/libssl.3.dylib"
install_name_tool \
    -id "@loader_path/libcrypto.3.dylib" \
    "$dist/lib/libcrypto.3.dylib"

codesign --force --sign - "$dist/lib/libcrypto.3.dylib"
codesign --force --sign - "$dist/lib/libssl.3.dylib"
codesign --force --sign - "$dist/rust-packaging"

rm -f "$archive"
tar -C "$(dirname "$dist")" -czf "$archive" "$(basename "$dist")"

echo "$dist"
echo "$archive"
