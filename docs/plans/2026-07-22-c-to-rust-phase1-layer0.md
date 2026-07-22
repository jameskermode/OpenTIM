# C to Rust Port — Phase 1, Foundation and Layer 0

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the verification gate, move the globals into Rust, and port all 45
leaf functions (819 lines) out of the C, leaving a working build at every commit.

**Architecture:** Each C function is rewritten in Rust as `#[no_mangle] pub extern "C" fn`
with an identical name and signature, then deleted from the C. Remaining C callers still
link against the Rust symbol, so the program builds and runs after every single function
moves. Ports are ordered bottom-up by the call graph so the FFI surface only ever shrinks.

**Tech Stack:** Rust 2018, `cc` crate + `zig cc` for the C half (both eventually deleted),
`wasm-bindgen` for the browser build, DOSBox-X and Ghidra as reverse-engineering oracles.

**Spec:** `docs/specs/2026-07-22-c-to-rust-port-design.md`

## Global Constraints

- **Behaviour must not change.** This is a transliteration. Every port is verified
  bit-identical against committed golden baselines captured before the port began; a
  divergence means the port is wrong. Never update a baseline to make the gate pass.
- **Exported functions must be `pub extern "C" fn`,** never bare `pub fn`. The Rust ABI is
  unspecified and C calling a Rust-ABI function caused a real optimisation-level-dependent
  bug (see `28c6ba5`).
- **Ported functions take raw pointers (`*mut Part`), never `&mut Part`.** While C still
  holds pointers to the same objects, a `&mut` asserts exclusivity that does not hold.
- **Preserve 16-bit arithmetic exactly.** Use `wrapping_add`/`wrapping_sub`/`wrapping_mul`
  and explicit casts. Do not "fix" overflow, truncation or sign behaviour.
- **Keep `/* TIMWIN: seg:off */` comments** on every ported function — they are the only
  cross-reference to the original disassembly.
- **Keep `stub_XXXX_XXXX` names** until a function is actually identified.
- **Never commit anything under `game-data/`.**

## The port recipe

Every function port follows this recipe. It is complete in itself; no task needs to refer
to another task to know what to do.

1. Read the C function in `c_src/`, including any `static inline` helpers it uses.
2. Add the Rust translation to `src/tim_c.rs` (engine functions) or `src/parts/mod.rs`
   (per-part behaviour), as `#[no_mangle] pub extern "C" fn <same_name>(...)`, keeping the
   original parameter order and integer widths, and copying the `TIMWIN:` comment.
3. Delete the C function body from `c_src/`. Leave its prototype in `c_src/tim.h` (or add
   one) so remaining C callers still compile against it.
4. Remove the now-duplicate `extern` declaration for that function from the `extern` block
   at the top of `src/tim_c.rs`, if one exists — Rust must not declare an import for a
   symbol it now defines.
5. Run `./scripts/verify.sh`. It must print `ALL CHECKS PASSED`.
6. Commit with `port: move <name> to Rust`.

If verify fails, the port is wrong. Do not adjust the expected values. Use
`cargo run --example trace -- game-data/tim1 <level> <ticks> <part-type>` in both the
previous commit and the current one, and diff the traces to find the first differing tick
and field.

## Type mapping

| C | Rust |
|---|---|
| `struct Part *` | `*mut Part` |
| `const struct Part *` | `*const Part` |
| `s16` / `int` (16-bit values) | `i16` |
| `u16` | `u16` |
| `s32` / `long` | `i32` |
| `byte` | `u8` |
| `bool` | `bool` (C99 `_Bool` is 1 byte, matches Rust) |
| `enum PartType` | `c_int` (C passes enums as int) |
| any `enum` global | `u32` — verify with `sizeof`; a bare C enum is 4 bytes here, and a narrower Rust static silently corrupts adjacent memory |
| `size_t` | `usize` |
| `struct ShortVec` | `ShortVec` (already `#[repr(C)]` in `tim_c.rs`) |

---

### Task 1: The verification gate

**Files:**
- Create: `scripts/verify.sh`
- Modify: `CLAUDE.md`

**Interfaces:**
- Produces: `./scripts/verify.sh`, exit 0 and prints `ALL CHECKS PASSED` when the build,
  tests, cross-configuration simulation comparison and reload check all pass. Every later
  task uses this as its test cycle.

- [ ] **Step 1: Write the gate script**

Create `scripts/verify.sh`:

```sh
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
```

- [ ] **Step 2: Make it executable and run it against current HEAD**

```bash
chmod +x scripts/verify.sh && ./scripts/verify.sh
```

Expected: ends with `ALL CHECKS PASSED`. If it fails here, the gate itself is wrong —
fix it before porting anything, because every later task trusts it.

- [ ] **Step 3: Prove the gate actually catches a regression**

Deliberately break the simulation, confirm the gate fails, then revert:

```bash
# Balloon density drives buoyancy, so changing it must change the simulation.
perl -pi -e 's/density: 9,/density: 11,/' src/parts/mod.rs
./scripts/verify.sh || echo "GATE CORRECTLY FAILED"
git checkout src/parts/mod.rs
./scripts/verify.sh
```

Expected: the middle run prints `CHECKS FAILED`, the last prints `ALL CHECKS PASSED`.
A gate that cannot fail is worthless; this step is not optional.

- [ ] **Step 4: Document it**

Add to `CLAUDE.md` under the build section:

```markdown
### Verification gate

`./scripts/verify.sh` is the gate for any change that must not alter simulation
behaviour. It builds debug, release and wasm, runs the unit tests, asserts that all
three configurations produce identical part state across 7 levels at 4 tick counts,
and checks that loading a level replaces the previous world. Run it after every
function ported from C.
```

- [ ] **Step 5: Commit**

```bash
git add scripts/verify.sh CLAUDE.md
git commit -m "Add the verification gate for the C to Rust port"
```

---

### Task 2: Move the globals into Rust

**Files:**
- Create: `src/globals.rs`
- Modify: `src/lib.rs`, `c_src/globals.h`, `c_src/globals.c`, `src/tim_c.rs`
- Delete: `c_src/globals.c` (contents become one line)

**Interfaces:**
- Consumes: `scripts/verify.sh` from Task 1.
- Produces: `opentim::globals` containing every former C global as
  `#[no_mangle] pub static mut <NAME>: <type>`, e.g. `GRAVITY: u16`, `AIR_PRESSURE: u16`,
  `STATIC_PARTS_ROOT: Part`, `LEVEL_STATE: u32`, `RESIZE_GOPHER: u16`, `SELECTED_PART:
  *mut Part`. Later tasks read and write these directly instead of adding `extern`
  declarations one at a time.

- [ ] **Step 1: Read the current globals**

```bash
cat c_src/globals.h
```

There are 39, declared through a `GLOBAL(declaration, init)` macro that expands to a
definition in `globals.c` and an `extern` declaration everywhere else.

- [ ] **Step 2: Create the Rust definitions**

Create `src/globals.rs`. Every global keeps its exact C name so the C half links against
it unchanged. Example of the required form — write one of these for each of the 39:

```rust
//! The simulation's global state, formerly c_src/globals.c.
//!
//! These keep their original C names because the remaining C code links against these
//! symbols directly. Many are short-lived temporaries the original used to pass values
//! between calls; they are preserved as-is during the port and retired in phase 2.

use crate::tim_c::Part;

/// TIMWIN: 1108:3e49. Ranges from 0 to 512 inclusive.
#[no_mangle]
pub static mut GRAVITY: u16 = 272;

/// TIMWIN: 1108:3e47. Ranges from 0 to 128 inclusive.
#[no_mangle]
pub static mut AIR_PRESSURE: u16 = 67;

/// TIMWIN: 1108:3faf
#[no_mangle]
pub static mut STATIC_PARTS_ROOT: Part = Part::ZERO;

/// TIMWIN: 1108:3bfb
#[no_mangle]
pub static mut RESIZE_GOPHER: u16 = 0;

/// TIMWIN: 1108:3e69
#[no_mangle]
pub static mut SELECTED_PART: *mut Part = std::ptr::null_mut();
```

`Part::ZERO` does not exist yet. Add it to `src/tim_c.rs` next to the generated struct:

```rust
impl Part {
    /// An all-zero Part, for the list-root globals which the C initialised with `{ 0 }`.
    pub const ZERO: Part = unsafe { std::mem::zeroed() };
}
```

- [ ] **Step 3: Declare the module**

In `src/lib.rs`, add alongside the other module declarations:

```rust
pub mod globals;
```

- [ ] **Step 4: Reduce the C side to declarations**

Change `c_src/globals.h` so the `GLOBAL` macro always produces an `extern` declaration,
never a definition:

```c
// Globals now live in Rust (src/globals.rs); this header only declares them.
#define GLOBAL(declaration, init) extern declaration;
```

Replace the whole contents of `c_src/globals.c` with:

```c
/* Globals moved to Rust; see src/globals.rs. This file is intentionally empty. */
```

- [ ] **Step 5: Remove the duplicate Rust imports**

In `src/tim_c.rs`, delete these lines from the `extern { ... }` block, since Rust now
defines them rather than importing them:

```rust
    pub static mut GRAVITY: u16;
    pub static mut AIR_PRESSURE: u16;
    pub static mut STATIC_PARTS_ROOT: Part;
    pub static mut MOVING_PARTS_ROOT: Part;
    pub static mut PARTS_BIN_ROOT: Part;
    pub static mut RESIZE_GOPHER: u16;
```

Then re-export them from `tim_c` so existing call sites keep compiling:

```rust
pub use crate::globals::{
    AIR_PRESSURE, GRAVITY, MOVING_PARTS_ROOT, PARTS_BIN_ROOT, RESIZE_GOPHER,
    STATIC_PARTS_ROOT,
};
```

- [ ] **Step 6: Verify**

```bash
./scripts/verify.sh
```

Expected: `ALL CHECKS PASSED`. A link error naming a global means one is missing from
`src/globals.rs` or misspelled; the C name must match exactly.

- [ ] **Step 7: Commit**

```bash
git add src/globals.rs src/lib.rs src/tim_c.rs c_src/globals.h c_src/globals.c
git commit -m "port: move the globals to Rust"
```

---

### Task 3: Port the three UNIMPLEMENTED shells

**Files:**
- Modify: `src/tim_c.rs`, `c_src/main.c`, `c_src/tim.h`

**Interfaces:**
- Consumes: the port recipe and `scripts/verify.sh`.
- Produces: `stub_10a8_0880`, `stub_10a8_1329`, `stub_10a8_28a5` as Rust
  `extern "C"` functions that panic when called.

These three have no real body — they are `UNIMPLEMENTED` shells. They carry zero
behavioural risk and are the right place to establish the pattern.

- [ ] **Step 1: Read them**

```bash
grep -n "stub_10a8_0880\|stub_10a8_1329\|stub_10a8_28a5" c_src/main.c
```

Each looks like:

```c
int stub_10a8_1329(struct BeltData *belt) {
    UNIMPLEMENTED;
    (void)(belt);
    return 0;
}
```

- [ ] **Step 2: Add the Rust versions**

In `src/tim_c.rs`, append (keeping each function's real signature — check the C for the
exact parameter types before writing these):

```rust
/* TIMWIN: 10a8:1329 */
#[no_mangle]
pub extern "C" fn stub_10a8_1329(_belt: *mut BeltData) -> c_int {
    unimplemented!("stub_10a8_1329")
}
```

- [ ] **Step 3: Delete the C bodies**

Remove the three function definitions from `c_src/main.c`, and make sure a prototype for
each exists in `c_src/tim.h` so remaining C callers still compile.

- [ ] **Step 4: Verify**

```bash
./scripts/verify.sh
```

Expected: `ALL CHECKS PASSED`. These functions are not reached by the 7 loadable levels,
so the simulation output must be unchanged.

- [ ] **Step 5: Commit**

```bash
git add src/tim_c.rs c_src/main.c c_src/tim.h
git commit -m "port: move the three UNIMPLEMENTED stubs to Rust"
```

---

### Task 4: Port the allocation leaves

**Files:**
- Modify: `src/tim_c.rs`, `c_src/main.c`, `c_src/tim.h`

**Interfaces:**
- Produces: `part_alloc() -> *mut Part`, `part_free(*mut Part)`,
  `part_free_borders(*mut Part)`, `belt_data_alloc() -> *mut BeltData`,
  `rope_data_alloc() -> *mut RopeData`, `debug_part_size() -> usize`,
  `remove_part_from_linked_list(*mut Part)` — all `extern "C"`, all still callable from C.

These allocate with `malloc` and zero with `memset`. In Rust they use the same allocator
the existing `src/wasm_libc.rs` shim wraps, so allocation behaviour is unchanged.

- [ ] **Step 1: Add the Rust implementations**

In `src/tim_c.rs`:

```rust
/// TIMWIN: 1078:00f2 (allocation half)
#[no_mangle]
pub extern "C" fn part_alloc() -> *mut Part {
    let layout = std::alloc::Layout::new::<Part>();
    unsafe {
        let p = std::alloc::alloc_zeroed(layout) as *mut Part;
        if p.is_null() { std::ptr::null_mut() } else { p }
    }
}

#[no_mangle]
pub extern "C" fn debug_part_size() -> usize {
    std::mem::size_of::<Part>()
}

/// TIMWIN: 10a8:1e18
#[no_mangle]
pub extern "C" fn remove_part_from_linked_list(part: *mut Part) {
    unsafe {
        (*(*part).prev).next = (*part).next;
        if !(*part).next.is_null() {
            (*(*part).next).prev = (*part).prev;
        }
    }
}

#[no_mangle]
pub extern "C" fn part_free_borders(part: *mut Part) {
    unsafe {
        if !(*part).borders_data.is_null() {
            let n = (*part).num_borders as usize;
            let layout = std::alloc::Layout::array::<BorderPoint>(n).unwrap();
            std::alloc::dealloc((*part).borders_data as *mut u8, layout);
            (*part).num_borders = 0;
            (*part).borders_data = std::ptr::null_mut();
        }
    }
}
```

Write `belt_data_alloc`, `rope_data_alloc` and `part_free` the same way. `part_free` must
keep its ownership rules exactly: free `borders_data` always; free `belt_data` only when
`F2_0001` is clear; free `rope_data[0]` only when the part is `P_PULLEY` or `P_ROPE`.

- [ ] **Step 2: A caution about `free`**

The C used `malloc`/`free`. Rust's `dealloc` needs the same `Layout` the allocation used,
which is why `part_free_borders` reconstructs it from `num_borders`. Any allocation whose
size is not recoverable at free time must keep using the C allocator until its partner
moves in the same commit. Port allocate/free pairs together, never one at a time.

- [ ] **Step 3: Delete the C bodies and keep prototypes**

Remove those functions from `c_src/main.c`; ensure `c_src/tim.h` declares each one.

- [ ] **Step 4: Remove the now-duplicate extern declarations**

Delete `part_alloc`, `part_free`, `belt_data_alloc`, `rope_data_alloc`,
`debug_part_size` from the `extern { ... }` block in `src/tim_c.rs`.

- [ ] **Step 5: Verify**

```bash
./scripts/verify.sh
```

Expected: `ALL CHECKS PASSED`. Then check for allocator mistakes specifically:

```bash
leaks -atExit -- ./target/debug/examples/reload game-data/tim1 L6.LEV L31.LEV L21.LEV 2>/dev/null | grep -E "leaks for|total leaked"
```

Expected: `0 leaks for 0 total leaked bytes`.

- [ ] **Step 6: Commit**

```bash
git add src/tim_c.rs c_src/main.c c_src/tim.h
git commit -m "port: move part and rope/belt allocation to Rust"
```

---

### Task 5: Set up Ghidra against the reference binary

**Files:**
- Create: `docs/reverse-engineering-setup.md`
- Modify: `reverse-engineering/README.md`

**Interfaces:**
- Produces: a Ghidra project containing `CD/TEMIM.EXE` with Win16 imports named, and a
  documented procedure for identifying a `stub_XXXX_XXXX` function from its TIMWIN address.

This runs in parallel with the ports and gates nothing in Tasks 1-4. It is needed before
the layer-0 stubs in Task 7 can be given real names.

- [ ] **Step 1: Install Ghidra and a JDK**

```bash
brew install --cask ghidra temurin
ghidraRun &
```

- [ ] **Step 2: Confirm the binary is the exact reference**

```bash
shasum -a 256 ~/Downloads/TemIM3x/CD/TEMIM.EXE
```

Expected exactly:
`03d56a132ff7c987488c6d28cc6ba9c4a28b6f9d085c53a3c5a0bfdd14e49e35`

Use `CD/TEMIM.EXE`. Do **not** use `TIMWIN/TEMIM.EXE` — it has been patched by
`CD/patch/PATCH.EXE` and its addresses will not match the `TIMWIN:` comments.

- [ ] **Step 3: Import and auto-analyse**

Create a new Ghidra project, import `CD/TEMIM.EXE`. Ghidra recognises the 16-bit NE
format. Run auto-analysis with defaults.

- [ ] **Step 4: Name the Win16 imports**

Add `reverse-engineering/ghidra-scripts` and `scripts` as script directories, then run
`rename-win16-fns.py`. It resolves ordinal imports from GDI/USER/KERNEL into real names
using the `.spec` files.

These scripts were written for Ghidra 9.1.2 and Jython 2.7. On a current Ghidra they may
need porting to PyGhidra. If they fail, record what broke in
`docs/reverse-engineering-setup.md` rather than silently skipping them — the Win16 names
are what make the graphics-adjacent code readable.

- [ ] **Step 5: Validate the address mapping on a known function**

Pick a function whose behaviour is already understood and confirm the disassembly matches.
`c_src/main.c` documents `remove_part_from_linked_list` as `TIMWIN: 10a8:1e18`. In Ghidra,
navigate to segment `10a8`, offset `0x1e18`, and confirm you find a short function that
unlinks a node from a doubly-linked list.

If this does not match, the address mapping is wrong and every later identification would
be wrong too. Stop and resolve it before continuing.

- [ ] **Step 6: Write down the procedure**

Create `docs/reverse-engineering-setup.md` recording: the Ghidra version used, the exact
import settings, whether the scripts needed porting and how, how segment:offset maps to a
Ghidra address, and the validation result from Step 5.

- [ ] **Step 7: Commit**

```bash
git add docs/reverse-engineering-setup.md reverse-engineering/README.md
git commit -m "Document the Ghidra setup against the reference binary"
```

---

### Task 6: Port the remaining identified layer-0 functions

**Files:**
- Modify: `src/tim_c.rs`, `src/parts/mod.rs`, `c_src/main.c`, `c_src/part_defs.c`,
  `c_src/draw_rope.c`, `c_src/tim.h`

**Interfaces:**
- Consumes: the port recipe, `scripts/verify.sh`.
- Produces: every identified layer-0 function as `extern "C"` Rust.

Work through the identified (non-`stub_`) layer-0 functions in the order listed in the
appendix, smallest first. Apply the port recipe to each one. Commit after each function,
or after a group of closely related ones, but run `./scripts/verify.sh` before every
commit.

- [ ] **Step 1: Port functions in appendix order, smallest first**

For each: read the C, write the Rust `extern "C"` equivalent, delete the C body, keep the
prototype in `tim.h`, remove any duplicate `extern` import, run the gate, commit.

- [ ] **Step 2: Handle the `static` helpers**

Eight functions have no external linkage and cannot be exported individually:

`utos`, `uneg`, `mul32`, `insert_part_into_root`, `calculate_border_normal_segment`,
`check_play_bowling_ball_impact_sound`, `move_llama2_to_beginning_of_llama1`,
`stub_10a8_0328`

Port each as a **private Rust function** at the same time as its first C caller moves, not
before. Until then it must stay in the C. `utos`, `uneg` and `mul32` encode the original's
16-bit semantics and must be transliterated exactly:

```rust
/// Unsigned to signed reinterpretation, as two's complement. The C comment notes that
/// doing this by cast is undefined, hence the manual wrap.
#[inline]
fn utos(v: u16) -> i16 {
    if v < 0x8000 { v as i16 } else { -((0x10000i32 - v as i32) as i16) }
}

/// Two's complement negation of a u16.
#[inline]
fn uneg(v: u16) -> u16 {
    (v ^ 0xFFFF).wrapping_add(1)
}
```

- [ ] **Step 3: Confirm layer 0 is empty of identified functions**

```bash
grep -c "^[a-zA-Z_].*)\s*{$" c_src/main.c
```

Expected: lower than the starting count of 80 by the number of functions ported.

- [ ] **Step 4: Full verification**

```bash
./scripts/verify.sh
```

Expected: `ALL CHECKS PASSED`.

---

### Task 7: Identify and port the layer-0 stubs

**Files:**
- Modify: `src/tim_c.rs`, `c_src/main.c`, `c_src/tim.h`, `docs/reverse-engineering-setup.md`

**Interfaces:**
- Consumes: the Ghidra project from Task 5.
- Produces: the eight layer-0 `stub_XXXX_XXXX` functions in Rust, renamed where
  identification succeeded.

The eight are: `stub_10a8_0328`, `stub_10a8_1329`, `stub_10a8_0880`, `stub_10a8_28f6`,
plus the remainder listed in the appendix. Three were already handled in Task 3 as
`UNIMPLEMENTED` shells; the rest have real bodies.

- [ ] **Step 1: Identify each in Ghidra**

For each stub, navigate to its `segment:offset` and read the original. Record what it
does, what it reads and writes, and who calls it.

- [ ] **Step 2: Rename only where confident**

If the purpose is unambiguous, rename the Rust function to something descriptive and add a
doc comment, keeping the `TIMWIN:` address. If it is not clear, keep the `stub_` name and
write down what was learned. A wrong name is worse than no name.

Renaming means updating every C caller too. Run the gate afterwards — a rename that misses
a caller is a link error, not a silent bug.

- [ ] **Step 3: Port each with the recipe, verifying after every one**

- [ ] **Step 4: Record what is still unidentified**

Update `docs/reverse-engineering-setup.md` with a table of the stubs, whether each was
identified, and any partial findings.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "port: move the layer-0 stubs to Rust"
```

---

### Task 8: Close out phase 1 layer 0

**Files:**
- Modify: `README.md`, `docs/specs/2026-07-22-c-to-rust-port-design.md`

- [ ] **Step 1: Measure what is left**

```bash
wc -l c_src/*.c
grep -c "^[a-zA-Z_].*)\s*{$" c_src/main.c
```

- [ ] **Step 2: Update the status table in `README.md`**

Add a row recording how much C remains, so the progress is visible to anyone landing on
the repository.

- [ ] **Step 3: Mark layer 0 complete in the spec**

Change the spec's status line from `approved, not yet started` to
`in progress — layer 0 complete`.

- [ ] **Step 4: Full verification and push**

```bash
./scripts/verify.sh
git add -A && git commit -m "Record layer 0 port progress"
```

---

## Appendix: the layer-0 worklist

Port in this order, smallest first. `Identified = no` means it needs Task 5's Ghidra setup
before it can be named, though it can still be transliterated.

| Function | File | Line | Lines | Identified |
|---|---|---:|---:|---|
| `debug_part_size` | main.c | 303 | 3 | yes |
| `part_get_movement_delta_angle` | main.c | 459 | 3 | yes |
| `uneg` | main.c | 24 | 4 | yes |
| `mul32` | main.c | 29 | 4 | yes |
| `stub_10a8_0328` | main.c | 1769 | 4 | no |
| `stub_10a8_1329` | main.c | 2034 | 5 | no |
| `remove_part_from_linked_list` | main.c | 308 | 6 | yes |
| `stub_10a8_0880` | main.c | 2271 | 6 | no |
| `part_alloc` | main.c | 210 | 7 | yes |
| `part_free_borders` | main.c | 218 | 7 | yes |
| `belt_data_alloc` | main.c | 232 | 7 | yes |
| `rope_data_alloc` | main.c | 240 | 7 | yes |
| `stub_10a8_28f6` | main.c | 1395 | 7 | no |
| `bucket_add_mass` | main.c | 1725 | 7 | yes |
| `check_play_bowling_ball_impact_sound` | main.c | 1760 | 7 | yes |
| `calculate_intersecting_rect` | main.c | 2025 | 8 | yes |
| `quadrant_from_angle` | main.c | 448 | 9 | yes |
| `stub_10a8_2bea` | main.c | 1384 | 9 | no |
| `stub_10a8_28a5` | main.c | 2091 | 9 | no |
| `tmp_3a6a_update_vars` | main.c | 675 | 10 | yes |
| `bucket_contains` | main.c | 1018 | 10 | yes |
| `utos` | main.c | 12 | 11 | yes |
| `get_first_part` | main.c | 165 | 12 | yes |
| `four_points_adjust_p1_by_one` | main.c | 119 | 13 | yes |
| `move_llama2_to_beginning_of_llama1` | main.c | 316 | 13 | yes |
| `should_parts_skip_collision` | main.c | 895 | 14 | yes |
| `part_clamp_to_terminal_velocity` | main.c | 421 | 15 | yes |
| `part_free` | main.c | 193 | 16 | yes |
| `tmp_3a6c_update_vars` | main.c | 656 | 16 | yes |
| `generate_hypot_samples` | draw_rope.c | 104 | 17 | yes |
| `stub_1050_025e` | main.c | 721 | 19 | no |
| `balloon_rope` | part_defs.c | 582 | 19 | yes |
| `initialize_llamas` | main.c | 94 | 22 | yes |
| `teeter_totter_helper_get_part_speed` | part_defs.c | 264 | 22 | yes |
| `part_set_size` | main.c | 331 | 27 | yes |
| `is_low_res_and_specific_part` | main.c | 2123 | 28 | yes |
| `bucket_handle_contained_parts` | main.c | 1689 | 33 | yes |
| `distance_to_rope_link` | main.c | 1301 | 38 | yes |
| `stub_10a8_4509` | main.c | 1342 | 40 | no |
| `insert_part_into_root` | main.c | 35 | 42 | yes |
| `belt_set_four_pos` | main.c | 494 | 47 | yes |
| `part_set_prev_vars` | main.c | 2168 | 48 | yes |
| `approximate_hypot_of_rope` | draw_rope.c | 5 | 53 | yes |
| `rope_calculate_flags` | part_defs.c | 162 | 57 | yes |
| `teeter_totter_helper_1` | part_defs.c | 87 | 58 | yes |

## Later plans

Layers 1 to 6 (47 functions, 2,219 lines) each get their own plan, written once layer 0 is
complete and the recipe has proven itself:

| Layer | Functions | Lines | Unidentified |
|---|---|---:|---:|---|
| 1 | 25 | 690 | 7 |
| 2 | 14 | 799 | 5 |
| 3 | 4 | 239 | 2 |
| 4 | 2 | 330 | 1 |
| 5 | 1 | 33 | 1 |
| 6 | 1 | 128 | 0 |
