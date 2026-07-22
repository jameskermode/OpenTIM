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

# All scratch files for this run live under one per-invocation directory so two
# concurrent runs of this script cannot clobber each other's temp files.
VERIFY_TMP="$(mktemp -d "${TMPDIR:-/tmp}/opentim-verify.XXXXXX")"
cleanup() { rm -rf "$VERIFY_TMP"; }
trap cleanup EXIT INT TERM

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

echo "== C compiler diagnostics (implicit declarations / incompatible pointer types) =="
# These two warning classes indicate real C type errors that cc-rs happily compiles
# anyway (they don't fail `cargo build`), so a normal build can pass with broken C:
#   -Wimplicit-function-declaration : a call with no visible prototype -- C assumes the
#       function returns `int`, which silently truncates a returned pointer on a 64-bit
#       target.
#   -Wincompatible-pointer-types    : e.g. a struct tag first named inside a function
#       prototype gets function-prototype scope, making it a distinct incomplete type from
#       the real file-scope struct of the same name -- callers passing the real struct
#       pointer then get this warning instead of a clean compile.
# Both are otherwise easy to miss (the binary still links and often still runs), so check
# for them explicitly here rather than relying on someone reading `cargo build` warnings.
# Compiled directly with the system compiler (not through cargo/cc-rs) so this check does
# not depend on incremental build caching -- it always re-parses every C file.
C_SOURCES="c_src/foo.c c_src/globals.c c_src/main.c c_src/part_defs.c c_src/draw_rope.c"
C_DIAG="$VERIFY_TMP/c_diagnostics.txt"
# Use the same compiler the real build uses: the `cc` crate honours a `CC` env var override,
# so hardcoding `cc` here would let this check silently diagnose a different compiler than
# the one that actually built the binaries being verified.
"${CC:-cc}" -Wall -Wextra -fsyntax-only $C_SOURCES > "$C_DIAG" 2>&1 || true
C_BAD_DIAGS="$(grep -E '\[-W(implicit-function-declaration|incompatible-pointer-types)\]' "$C_DIAG" || true)"
if [ -n "$C_BAD_DIAGS" ]; then
    echo "  FAIL C compiler emitted implicit-function-declaration or incompatible-pointer-types warnings:"
    echo "$C_BAD_DIAGS" | sed 's/^/    /'
    FAIL=1
fi

echo "== TIMWIN provenance tags on ported functions (src/tim_c.rs, src/parts/mod.rs, src/wasm_libc.rs) =="
# Every ported function carries a `TIMWIN: segment:offset` doc-comment tag -- the ONLY
# cross-reference from the Rust back to the original disassembled binary. Losing that tag
# loses the function's provenance. This has happened silently twice: a new function's doc
# comment was written directly beneath the previous function's doc comment with no blank
# line between them, so rustdoc merges both `///` blocks onto the second function, leaving
# the first with no doc comment and no tag at all. This check catches that (and any other
# untagged export) mechanically instead of relying on someone noticing during review.
#
# Ported code is not confined to src/tim_c.rs -- e.g. src/parts/mod.rs already holds several
# ported functions, and more files will gain them as the port continues -- so every Rust
# source that has ever exported a `#[no_mangle] pub extern "C" fn` is listed explicitly here.
# When porting introduces a new module with such exports, add it to this list; an unlisted
# file with untagged exports would defeat the whole check.
TIMWIN_SOURCES="src/tim_c.rs src/parts/mod.rs src/wasm_libc.rs"
#
# A handful of exported functions are project infrastructure rather than ports of original
# TIM code, so they legitimately have no TIMWIN tag. That set was established by manually
# auditing every `#[no_mangle] pub extern "C" fn` in TIMWIN_SOURCES as of 2026-07-22 (see the
# task that added this check); it must only grow for the same reason -- a NEW omission
# should fail, not get silently added here.
TIMWIN_ALLOWLIST="unimplemented output_c output_part_c output_int_c arctan_c sine_c \
cosine_c rotate_point_c calculate_line_intersection calculate_line_intersection_helper \
belt_data_alloc rope_data_alloc debug_part_size part_image_size part_density part_mass \
part_bounciness part_friction part_order part_data30_flags1 part_data30_flags3 \
part_data30_size_something2 part_data30_size part_data31_render_pos_offset \
part_explicit_size part_run part_reset part_bounce part_flip part_resize part_rope \
part_create_func abs"

TIMWIN_MISSING="$(awk -v allow="$TIMWIN_ALLOWLIST" '
BEGIN {
    n = split(allow, arr, " ")
    for (i = 1; i <= n; i++) {
        name = arr[i]
        gsub(/^[ \t]+|[ \t]+$/, "", name)
        if (name != "") allowed[name] = 1
    }
    doc_tag = 0
    pending = 0
}
FNR == 1 {
    # Reset per-file: a dangling #[no_mangle] or open doc block at the end of one file must
    # never be carried over and matched against the first lines of the next file.
    doc_tag = 0
    pending = 0
}
{
    t = $0
    gsub(/^[ \t]+|[ \t]+$/, "", t)

    if (t ~ /^\/\/\//) {
        if (t ~ /TIMWIN/) doc_tag = 1
        next
    }

    if (t == "#[no_mangle]") {
        pending = 1
        pending_tag = doc_tag
        doc_tag = 0
        next
    }

    if (pending == 1 && t ~ /^pub extern "C" fn /) {
        name = t
        sub(/^pub extern "C" fn[ \t]+/, "", name)
        sub(/[ \t(].*/, "", name)
        if (pending_tag == 0 && !(name in allowed)) {
            print name
        }
        pending = 0
        doc_tag = 0
        next
    }

    # Any other line (including a blank one) breaks doc-comment contiguity and cancels a
    # dangling #[no_mangle] that was not immediately followed by the fn signature.
    doc_tag = 0
    pending = 0
}
' $TIMWIN_SOURCES)"

if [ -n "$TIMWIN_MISSING" ]; then
    echo "  FAIL these exported functions have no TIMWIN: tag in the doc"
    echo "  comment immediately above them, and are not in the allowlist:"
    echo "$TIMWIN_MISSING" | sed 's/^/    /'
    FAIL=1
fi

echo "== build =="
cargo build --quiet
cargo build --quiet --release

echo "== unit tests =="
# Capture output and exit status separately from any filtering: piping straight into
# `grep | head` would make the pipeline's exit status head's (always 0), so a failing
# test run would never fail the gate. Instead run cargo test as the `if` condition
# (exempt from `set -e`), and only inspect the exit code to decide pass/fail.
if cargo test --quiet > "$VERIFY_TMP/v_test.txt" 2>&1; then
    grep -E "test result" "$VERIFY_TMP/v_test.txt" | head -1
else
    echo "  FAIL unit tests failed"
    grep -E "test result" "$VERIFY_TMP/v_test.txt" | head -5
    echo "  -- last 40 lines of cargo test output --"
    tail -n 40 "$VERIFY_TMP/v_test.txt"
    FAIL=1
fi

if [ ! -f game-data/tim1/RESOURCE.MAP ]; then
    if [ "$BLESS" = "1" ]; then
        echo "!! game-data/tim1 missing - cannot bless baselines without it"
        exit 1
    fi
    echo "!! game-data/tim1 missing - skipping simulation checks"
    if [ "$FAIL" = "0" ]; then
        echo "ALL CHECKS PASSED (build and unit tests only)"
        exit 0
    else
        echo "CHECKS FAILED (unit tests failed; build and unit tests only)"
        exit 1
    fi
fi

echo "== wasm build =="
if command -v wasm-bindgen >/dev/null 2>&1 && command -v zig >/dev/null 2>&1; then
    cargo build --quiet --lib --release --target wasm32-unknown-unknown
    wasm-bindgen target/wasm32-unknown-unknown/release/opentim.wasm \
        --out-dir "$VERIFY_TMP/wasm-out" --target nodejs >/dev/null
    WASM=1
else
    echo "   (zig or wasm-bindgen missing - skipping wasm comparison)"
    WASM=0
fi

cat > "$VERIFY_TMP/compare.js" <<EOF
const fs = require('fs'), path = require('path');
const { Game } = require('$VERIFY_TMP/wasm-out/opentim.js');
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
    ./target/debug/opentim   game-data/tim1 "$lev.LEV" "$t" 2>/dev/null | sed -n '/after/,$p' | tail -n +2 > "$VERIFY_TMP/v_dbg.txt"
    ./target/release/opentim game-data/tim1 "$lev.LEV" "$t" 2>/dev/null | sed -n '/after/,$p' | tail -n +2 > "$VERIFY_TMP/v_rel.txt"
    if [ ! -s "$VERIFY_TMP/v_dbg.txt" ]; then echo "  FAIL $lev@$t produced no output"; FAIL=1; continue; fi
    if [ ! -s "$VERIFY_TMP/v_rel.txt" ]; then echo "  FAIL $lev@$t release produced no output"; FAIL=1; continue; fi
    if [ ! -f "$base" ]; then
      echo "  FAIL $lev@$t no baseline at $base (run ./scripts/verify.sh --bless to create it deliberately)"
      FAIL=1
    elif ! diff -q "$base" "$VERIFY_TMP/v_rel.txt" >/dev/null; then
      echo "  FAIL $lev@$t release != baseline $base"; diff "$base" "$VERIFY_TMP/v_rel.txt" | head -4; FAIL=1
    fi
    if ! diff -q "$VERIFY_TMP/v_dbg.txt" "$VERIFY_TMP/v_rel.txt" >/dev/null; then
      echo "  FAIL $lev@$t debug != release"; diff "$VERIFY_TMP/v_dbg.txt" "$VERIFY_TMP/v_rel.txt" | head -4; FAIL=1
    fi
    if [ "$WASM" = "1" ]; then
      if ! node "$VERIFY_TMP/compare.js" "$PWD/game-data/tim1" "$lev.LEV" "$t" > "$VERIFY_TMP/v_wsm.txt" 2> "$VERIFY_TMP/v_wsm_err.txt"; then
        echo "  FAIL $lev@$t wasm crashed"; tail -n 20 "$VERIFY_TMP/v_wsm_err.txt"; FAIL=1
      elif ! diff -q "$VERIFY_TMP/v_rel.txt" "$VERIFY_TMP/v_wsm.txt" >/dev/null; then
        echo "  FAIL $lev@$t release != wasm"; diff "$VERIFY_TMP/v_rel.txt" "$VERIFY_TMP/v_wsm.txt" | head -4; FAIL=1
      fi
    fi
  done
done

echo "== reload: loading a level replaces the previous world =="
cargo build --quiet --example reload
RELOAD_OK=1
if ! RELOAD_TICKS=120 ./target/debug/examples/reload game-data/tim1 L31.LEV > "$VERIFY_TMP/v_fresh_raw.txt" 2>&1; then
  echo "  FAIL reload (fresh L31) crashed"; tail -n 20 "$VERIFY_TMP/v_fresh_raw.txt"; FAIL=1; RELOAD_OK=0
fi
if ! RELOAD_TICKS=120 ./target/debug/examples/reload game-data/tim1 L6.LEV L21.LEV L31.LEV > "$VERIFY_TMP/v_reload_raw.txt" 2>&1; then
  echo "  FAIL reload (L6,L21,L31) crashed"; tail -n 20 "$VERIFY_TMP/v_reload_raw.txt"; FAIL=1; RELOAD_OK=0
fi
if [ "$RELOAD_OK" = "1" ]; then
  grep "^  " "$VERIFY_TMP/v_fresh_raw.txt" > "$VERIFY_TMP/v_fresh.txt" || true
  grep "^  " "$VERIFY_TMP/v_reload_raw.txt" > "$VERIFY_TMP/v_reload.txt" || true
  if [ ! -s "$VERIFY_TMP/v_fresh.txt" ] || [ ! -s "$VERIFY_TMP/v_reload.txt" ]; then
    echo "  FAIL reload check produced no parsable output"; FAIL=1
  elif ! diff -q "$VERIFY_TMP/v_fresh.txt" "$VERIFY_TMP/v_reload.txt" >/dev/null; then
    echo "  FAIL reloaded world differs from fresh"; diff "$VERIFY_TMP/v_fresh.txt" "$VERIFY_TMP/v_reload.txt" | head -4; FAIL=1
  fi
fi

if [ "$FAIL" = "0" ]; then echo "ALL CHECKS PASSED"; else echo "CHECKS FAILED"; exit 1; fi
