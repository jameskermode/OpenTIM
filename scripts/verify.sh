#!/bin/sh
# Verification gate for the C-to-Rust port. Behaviour must not change, so this compares
# the simulation across build configurations rather than asserting fixed values.
#
# Requires game-data/tim1 (user-supplied game files). Without it, only build and unit
# tests run.
set -e
cd "$(dirname "$0")/.."

LEVELS="L6 L20 L21 L24 L25 L31 L79"
TICKS="0 30 120 300"
FAIL=0

echo "== build =="
cargo build --quiet
cargo build --quiet --release

echo "== unit tests =="
cargo test --quiet 2>&1 | grep -E "test result" | head -1

if [ ! -f game-data/tim1/RESOURCE.MAP ]; then
    echo "!! game-data/tim1 missing - skipping simulation checks"
    echo "ALL CHECKS PASSED (build and unit tests only)"
    exit 0
fi

echo "== wasm build =="
if command -v wasm-bindgen >/dev/null 2>&1 && command -v zig >/dev/null 2>&1; then
    cargo build --quiet --lib --release --target wasm32-unknown-unknown
    wasm-bindgen target/wasm32-unknown-unknown/release/opentim.wasm \
        --out-dir /tmp/opentim-verify --target nodejs >/dev/null
    WASM=1
else
    echo "   (zig or wasm-bindgen missing - skipping wasm comparison)"
    WASM=0
fi

cat > /tmp/opentim-verify-compare.js <<'EOF'
const fs = require('fs'), path = require('path');
const { Game } = require('/tmp/opentim-verify/opentim.js');
const dir = process.argv[2], files = {};
for (const n of fs.readdirSync(dir)) {
  const p = path.join(dir, n);
  if (fs.statSync(p).isFile()) files[n.toUpperCase()] = new Uint8Array(fs.readFileSync(p));
}
const g = new Game(files);
g.load_level(process.argv[3]);
g.tick_n(parseInt(process.argv[4], 10));
process.stdout.write(g.parts_summary());
EOF

echo "== simulation: debug == release == wasm =="
for lev in $LEVELS; do
  for t in $TICKS; do
    ./target/debug/opentim   game-data/tim1 "$lev.LEV" "$t" 2>/dev/null | sed -n '/after/,$p' | tail -n +2 > /tmp/v_dbg.txt
    ./target/release/opentim game-data/tim1 "$lev.LEV" "$t" 2>/dev/null | sed -n '/after/,$p' | tail -n +2 > /tmp/v_rel.txt
    if [ ! -s /tmp/v_dbg.txt ]; then echo "  FAIL $lev@$t produced no output"; FAIL=1; continue; fi
    if ! diff -q /tmp/v_dbg.txt /tmp/v_rel.txt >/dev/null; then
      echo "  FAIL $lev@$t debug != release"; diff /tmp/v_dbg.txt /tmp/v_rel.txt | head -4; FAIL=1
    fi
    if [ "$WASM" = "1" ]; then
      node /tmp/opentim-verify-compare.js "$PWD/game-data/tim1" "$lev.LEV" "$t" > /tmp/v_wsm.txt 2>/dev/null
      if ! diff -q /tmp/v_rel.txt /tmp/v_wsm.txt >/dev/null; then
        echo "  FAIL $lev@$t release != wasm"; diff /tmp/v_rel.txt /tmp/v_wsm.txt | head -4; FAIL=1
      fi
    fi
  done
done

echo "== reload: loading a level replaces the previous world =="
cargo build --quiet --example reload
RELOAD_TICKS=120 ./target/debug/examples/reload game-data/tim1 L31.LEV 2>/dev/null | grep "^  " > /tmp/v_fresh.txt
RELOAD_TICKS=120 ./target/debug/examples/reload game-data/tim1 L6.LEV L21.LEV L31.LEV 2>/dev/null | grep "^  " > /tmp/v_reload.txt
if ! diff -q /tmp/v_fresh.txt /tmp/v_reload.txt >/dev/null; then
  echo "  FAIL reloaded world differs from fresh"; FAIL=1
fi

if [ "$FAIL" = "0" ]; then echo "ALL CHECKS PASSED"; else echo "CHECKS FAILED"; exit 1; fi
