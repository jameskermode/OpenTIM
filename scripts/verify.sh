#!/bin/sh
# Verification gate for the C-to-Rust port. Behaviour must not change, so this compares
# the simulation two ways:
#   1. against committed golden baselines captured from known-good code (tests/baselines/)
#      - this is what catches a transliteration that computes the wrong answer identically
#        in every build configuration, which a config-vs-config diff alone cannot see.
#   2. across build configurations (debug == release == wasm)
#      - this is what catches platform-specific divergence (e.g. an ABI bug), which a
#        golden baseline alone cannot see.
#
# Requires game-data/tim1 (user-supplied game files). Without it, only build and unit
# tests run.
#
# Usage:
#   ./scripts/verify.sh          run the gate
#   ./scripts/verify.sh --bless  rewrite tests/baselines/ from the current build (release)
set -e
cd "$(dirname "$0")/.."

BLESS=0
if [ "$1" = "--bless" ]; then
    BLESS=1
fi

LEVELS="L6 L20 L21 L24 L25 L31 L79"
TICKS="0 30 120 300"
FAIL=0

if [ "$BLESS" = "1" ]; then
    echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"
    echo "!! --bless: rewriting tests/baselines/ from the CURRENT build."
    echo "!!"
    echo "!! If you are running this during a C-to-Rust port task, the baselines are"
    echo "!! supposed to be bit-identical before and after the port. Needing to bless them"
    echo "!! means the port CHANGED BEHAVIOUR -- i.e. the port is WRONG. Blessing away a"
    echo "!! real regression just deletes the evidence; it does not fix anything."
    echo "!! Only bless when a behaviour change is genuinely intended, and say so in the"
    echo "!! commit message that updates these fixtures."
    echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"
fi

echo "== build =="
cargo build --quiet
cargo build --quiet --release

echo "== unit tests =="
cargo test --quiet 2>&1 | grep -E "test result" | head -1

if [ ! -f game-data/tim1/RESOURCE.MAP ]; then
    if [ "$BLESS" = "1" ]; then
        echo "!! game-data/tim1 missing - cannot bless baselines without it"
        exit 1
    fi
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

if [ "$BLESS" = "1" ]; then
    echo "== blessing tests/baselines/ from release build =="
    mkdir -p tests/baselines
    for lev in $LEVELS; do
      for t in $TICKS; do
        base="tests/baselines/$lev.LEV.$t.txt"
        ./target/release/opentim game-data/tim1 "$lev.LEV" "$t" 2>/dev/null | sed -n '/after/,$p' | tail -n +2 > "$base"
        if [ ! -s "$base" ]; then echo "  FAIL $lev@$t produced no output while blessing"; FAIL=1; fi
      done
    done
    if [ "$FAIL" != "0" ]; then echo "BLESS FAILED"; exit 1; fi
    echo "   wrote $(ls tests/baselines | wc -l | tr -d ' ') baseline files"
fi

echo "== simulation: baseline == debug == release == wasm =="
for lev in $LEVELS; do
  for t in $TICKS; do
    base="tests/baselines/$lev.LEV.$t.txt"
    ./target/debug/opentim   game-data/tim1 "$lev.LEV" "$t" 2>/dev/null | sed -n '/after/,$p' | tail -n +2 > /tmp/v_dbg.txt
    ./target/release/opentim game-data/tim1 "$lev.LEV" "$t" 2>/dev/null | sed -n '/after/,$p' | tail -n +2 > /tmp/v_rel.txt
    if [ ! -s /tmp/v_dbg.txt ]; then echo "  FAIL $lev@$t produced no output"; FAIL=1; continue; fi
    if [ ! -f "$base" ]; then
      echo "  FAIL $lev@$t no baseline at $base (run ./scripts/verify.sh --bless to create it deliberately)"
      FAIL=1
    elif ! diff -q "$base" /tmp/v_rel.txt >/dev/null; then
      echo "  FAIL $lev@$t release != baseline $base"; diff "$base" /tmp/v_rel.txt | head -4; FAIL=1
    fi
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
