#!/bin/sh
# Build the browser version into web/pkg.
#
# Requires: rustup target add wasm32-unknown-unknown
#           cargo install wasm-bindgen-cli --version 0.2.126
#           zig (to cross-compile the C core; Apple clang has no wasm backend)
set -e

cd "$(dirname "$0")/.."

PROFILE="${1:-release}"
case "$PROFILE" in
  release) CARGO_FLAGS="--release"; OUT_DIR="target/wasm32-unknown-unknown/release" ;;
  debug)   CARGO_FLAGS="";          OUT_DIR="target/wasm32-unknown-unknown/debug" ;;
  *) echo "usage: $0 [release|debug]" >&2; exit 2 ;;
esac

# --lib only: the CLI binary would collide with the cdylib on the same output name.
cargo build --lib $CARGO_FLAGS --target wasm32-unknown-unknown

wasm-bindgen "$OUT_DIR/opentim.wasm" --out-dir web/pkg --target web

echo
echo "built web/pkg from the $PROFILE profile"
echo "serve it with:  python3 -m http.server -d web 8080"
echo "then open:      http://localhost:8080/"
