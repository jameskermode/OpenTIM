# C to Rust Port — Wave 2

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the 24 leaf functions (501 lines) that became leaves once wave 1 completed,
leaving a working build and a green gate at every commit.

**Architecture:** Unchanged from wave 1. Each C function is rewritten as
`#[no_mangle] pub extern "C" fn` with an identical name and signature, then deleted from the
C. Remaining C callers link against the Rust symbol, so the program builds and runs after
every function moves.

**Spec:** `docs/specs/2026-07-22-c-to-rust-port-design.md`
**Predecessor:** `docs/plans/2026-07-22-c-to-rust-phase1-layer0.md` (complete)

**Result (did not fully go to plan):** 18 functions were named and ported across Tasks 1-4
(the allocation pair counts as one commit but two functions; the four-stub batch in Task 4
counts as four). Measured net effect on `c_src/` (`grep -cE` function count across
`main.c`/`part_defs.c`/`draw_rope.c`): 55 → 38, i.e. only **17** fewer function
*definitions*, and **405** fewer lines, not the 24 functions / 501 lines the goal above
estimated. Two reasons, both honest gaps rather than silent shortfalls:

* `angle_between_part_centers` (formerly `stub_10a8_0328`) is `static inline` in C with no
  external linkage. Its one remaining C caller, `stub_1090_0809`, is part of the
  collision-response chain that did not move this wave, so per the porting recipe the C
  copy had to stay (renamed to match) alongside the new private Rust port — it counts as
  "ported" but does not reduce the C function count.
* The seven other `static` helpers flagged in the hazard section below (`utos`, `uneg`,
  `mul32`, `insert_part_into_root`, `calculate_border_normal_segment`,
  `check_play_bowling_ball_impact_sound`, `move_llama2_to_beginning_of_llama1`) were
  **never ported** — the plan's rule was to port each "at the same time as its first C
  caller moves," but every one of their C callers (the collision-response chain,
  `advance_parts`, `restore_parts_state_from_design`) is still C and deferred to the next
  plan, so the trigger condition never fired. They remain pure C, unchanged, for whichever
  plan finally moves those callers.

## What changed since wave 1

The call graph re-layered when 37 functions left the C, so functions that were interior are
now leaves. 55 C functions remain across 7 layers; this plan covers the new leaf layer only.

The gate is substantially stronger than it was at the start of wave 1. It now enforces:

1. **Golden baselines** in `tests/baselines/` — now **45 fields** per part plus full
   `RopeData`/`BeltData`, not the original 7. Most of wave 1 was only indirectly observed;
   this wave is directly observed.
2. **Cross-configuration agreement** — debug == release == wasm.
3. **C diagnostics** — fails on `-Wimplicit-function-declaration` and
   `-Wincompatible-pointer-types`.
4. **TIMWIN provenance tags** — every exported port must carry its own.
5. **FFI signatures** — C prototypes are compared against Rust `extern "C"` definitions for
   both ABI compatibility and signedness.

`PartsIteratorMut` now **detects** the next-caching divergence at runtime in debug builds
rather than merely documenting it.

## Global Constraints

- **Behaviour must not change.** Verified against committed golden baselines. Never run
  `./scripts/verify.sh --bless` — a gate failure means the port is wrong.
- **`#[no_mangle] pub extern "C" fn`**, never bare `pub fn`.
- **Raw pointers (`*mut Part`), never `&mut Part`**, while C still holds pointers to the
  same objects.
- **Preserve 16-bit arithmetic exactly** — C widens to 32-bit `int` then truncates on
  assignment back to 16 bits. Use `wrapping_*` and explicit casts. Arithmetic shift for
  signed operands, logical for unsigned. Do not add or remove null checks relative to the C.
- **Keep `TIMWIN: seg:off` comments**, one per function, blank-line separated.
- **Keep `stub_XXXX_XXXX` names** until a function is genuinely identified.
- **Never commit anything under `game-data/`**; never modify `tests/baselines/`.

## The port recipe

1. Read the C function, including any `static`/`static inline` helpers it uses.
2. Write the Rust in `src/tim_c.rs` (engine) or `src/parts/mod.rs` (per-part behaviour) as
   `#[no_mangle] pub extern "C" fn <same_name>(...)`, with its own doc comment and TIMWIN tag.
3. Delete the C body.
4. Ensure a prototype exists in `c_src/tim.h`, inside the include guard, after any struct
   type it references is defined.
5. Remove any now-duplicate declaration from the `extern { ... }` block in `src/tim_c.rs`.
6. Run `./scripts/verify.sh`; it must print `ALL CHECKS PASSED`.
7. Commit: `port: move <name> to Rust`.

## Two hazards specific to this wave

### The `static` helpers are now leaves

Six functions in this wave have no external linkage and **cannot be exported individually**:

`utos`, `uneg`, `mul32`, `insert_part_into_root`, `calculate_border_normal_segment`,
`check_play_bowling_ball_impact_sound`, `move_llama2_to_beginning_of_llama1`,
`stub_10a8_0328`

Port each as a **private Rust function at the same time as its first C caller moves**, and
leave the C copy in place while any C code still uses it. `utos`, `uneg` and `mul32` encode
the original's 16-bit semantics and must be transliterated exactly, not simplified.

### `part_alloc_borders` / `part_free_borders` must move together

`part_free_borders` is in this wave; its allocator partner `part_alloc_borders` is in the
next. **The pairing rule overrides the layer ordering** — Rust's `dealloc` requires the same
`Layout` the allocation used, so splitting them across languages corrupts the heap.

Port both in the same commit, even though one is technically out of this wave. Wave 1
correctly deferred `part_free_borders` for exactly this reason; do not defer it again, and
do not port it alone.

---

### Task 1: The allocation pair and the small list helpers

**Files:** `src/tim_c.rs`, `c_src/main.c`, `c_src/tim.h`

Port, in this order, one commit each except where noted:

1. `part_free_borders` **and** `part_alloc_borders` — **one commit, together** (see hazard above)
2. `part_init_rope_data_primary` (8 lines)
3. `part_init_belt_data` (8 lines)
4. `next_part_or_fallback` (12 lines)
5. `bucket_add_mass_of_contained` (5 lines)

- [x] **Step 1: Port each per the recipe, running `./scripts/verify.sh` before every commit**
- [x] **Step 2: For the allocation pair, additionally run the leak check**

```bash
leaks -atExit -- ./target/debug/examples/reload game-data/tim1 L6.LEV L31.LEV L21.LEV 2>/dev/null | grep -E "leaks for|total leaked"
```

Expected: `0 leaks for 0 total leaked bytes`. Also run the reload example under
`MallocScribble=1 MallocPreScribble=1` and confirm no crash — double-free and use-after-free
are invisible to the simulation gate.

- [x] **Step 3: Confirm `./scripts/verify.sh` prints `ALL CHECKS PASSED`**

---

### Task 2: Part construction and geometry

**Files:** `src/tim_c.rs`, `c_src/main.c`, `c_src/tim.h`

1. `all_parts_set_prev_vars` (11 lines)
2. `part_set_size_and_pos_render` (23 lines)
3. `update_rope_pos` (27 lines)
4. `part_new` (34 lines) — allocates via `part_alloc`; confirm the allocator pairing still
   holds after Task 1
5. `calculate_rope_sag` (38 lines, `draw_rope.c`)

- [x] **Step 1: Port each per the recipe, gate green before each commit**
- [x] **Step 2: Report which of these the gate actually exercises, and which rest on reading**

  Recorded per-function in each port's doc comment (`src/tim_c.rs`):

  - `all_parts_set_prev_vars` — exercised every tick with `SELECTED_PART` null (the batch
    harness the gate drives never sets it); the `SELECTED_PART` branch itself is never hit
    and rests on reading the C.
  - `part_set_size_and_pos_render` — fully exercised: runs every tick for every moving part,
    including both flip branches and the early-return path, across all 28
    golden-baseline level/tick combinations.
  - `update_rope_pos` — partially exercised: the `part1` non-null path, the `part2`-present
    path and the pulley-chain loop are hit every tick for every rope in the test levels; the
    `LEVEL_STATE != SIMULATION_MODE` block and the `!rope->part1`/`!rope->part2` early-outs
    are never hit (the gate always ticks in `SIMULATION_MODE`) and rest on reading the C.
  - `part_new` — exercised for the success path only (every part placed in the 28
    golden-baseline combinations is built through it); the `part_alloc` failure path and the
    `part_create_func` failure path are never hit (no `create_fn` currently signals failure)
    and rest on reading the C.
  - `calculate_rope_sag` — exercised only for the `rope_data->part1 == part` branch and the
    `ROPETIME_CURRENT` case, since every current call site passes `time == ROPETIME_CURRENT`.
    The `ROPETIME_PREV1`/`ROPETIME_PREV2` cases, the `part2` (`else`) branch, its early-outs,
    and the `P_PULLEY` `nextpart` selection are not confirmed exercised and rest on reading
    the C.

---

### Task 3: Collision and teeter-totter behaviour

**Files:** `src/tim_c.rs`, `src/parts/mod.rs`, `c_src/main.c`, `c_src/part_defs.c`, `c_src/tim.h`

1. `teeter_totter_helper_2` (41 lines, `part_defs.c`)
2. `teeter_totter_bounce` (48 lines, `part_defs.c`) — currently reached through the
   `bounce_c!` macro; replace that indirection with a direct Rust call
3. `part_borders_intersect` (73 lines) — the largest in this wave, and collision-critical

- [x] **Step 1: Port each per the recipe, gate green before each commit**
- [x] **Step 2: For `part_borders_intersect`, state explicitly which branches the gate covers**

  `part_borders_intersect` covers zero of the gate's branches: coverage instrumentation
  showed none of its 10 branches run under the 7-level x 4-tick drive at any tick count,
  because its only caller path, Pokey the Cat's walk state machine, never activates in
  those levels. A differential harness (`tests/differential/`) was added against a frozen
  copy of the original C body to cover it independently (5000+ generated `Part` pairs,
  fixed-seed PRNG; verified to actually catch divergence by deliberately inverting a
  comparison and confirming 321/5015 cases failed, then reverting).

  Also confirmed not exercised, by measurement rather than assumption: `teeter_totter_helper_2`
  and `teeter_totter_bounce` are only reachable through a `TeeterTotter` part instance
  (`teeter_totter_run` / the DEF's `bounce_fn`), and a tick-0 trace of all 7 gate levels
  (`cargo run --example trace -- game-data/tim1 <LEV> 0 3`, part-type 3 = `TeeterTotter`)
  found zero such parts in any of them, so neither function runs under the gate at all.

---

### Task 4: Identify and port the four unidentified functions

**Files:** `src/tim_c.rs`, `c_src/main.c`, `c_src/tim.h`, `docs/reverse-engineering-setup.md`

`stub_10a8_0328` (static, 4 lines), `stub_10a8_2b6d` (13), `stub_10a8_280a` (19),
`stub_10a8_0ab8` (35).

Ghidra is set up and the address mapping is validated — see
`docs/reverse-engineering-setup.md` for the working headless invocation. `TIMWIN: seg:off`
resolves directly to the same Ghidra address.

- [x] **Step 1: Read each in the C, then look it up in the original binary**
- [x] **Step 2: Rename only where the purpose is unambiguous**

A wrong name is worse than no name. State confidence and evidence per function; keeping
`stub_` is a perfectly good outcome. If renaming, update every caller in both languages.

  Ghidra decompilation of all four addresses matched the existing C instruction-for-
  instruction (no drift found). Three renamed with high confidence: `stub_10a8_0328` →
  `angle_between_part_centers`, `stub_10a8_2b6d` → `queue_part_dirty_rects`,
  `stub_10a8_280a` → `queue_dirty_rects_for_attachments`. `stub_10a8_0ab8` was **not**
  renamed — it depends entirely on the still-unidentified `stub_10a8_0880`, and every
  caller found in Ghidra lives in not-yet-ported UI-side code, so there was no confident
  name to give it; kept as `stub_` per the "a wrong name is worse than no name" rule.

- [x] **Step 3: Port each per the recipe, gate green before each commit**
- [x] **Step 4: Update the reverse-engineering doc with what was and was not established**

  `angle_between_part_centers` is `static inline` in C (no external linkage), so it was
  ported as a private Rust function rather than `#[no_mangle] extern "C"`; the C copy is
  kept (renamed to match) since the still-C `stub_1090_0809` calls it — the two copies must
  be kept in step by hand (a follow-up commit, `ac928e5`, added a warning comment to that
  effect after this was flagged as a latent trap). Gate coverage confirmed by temporary
  instrumentation (reverted before the porting commit): `queue_part_dirty_rects` and
  `queue_dirty_rects_for_attachments` run every tick for any moving part (28 calls in a
  30-tick `L6.LEV` run); `angle_between_part_centers` runs on bounce impacts (8 calls in a
  300-tick `L31.LEV` run); `stub_10a8_0ab8` has zero call sites anywhere and is not
  exercised (dead code, like `calculate_intersecting_rect`).

---

### Task 5: Close out wave 2

- [x] **Step 1: Measure — lines and functions remaining, functions ported**

  `cat c_src/*.c | wc -l` → 2,215 (includes the 2-line `foo.c` fixture and 1-line
  `globals.c` placeholder, unrelated to the port, same as noted in the layer-0 outcome).
  `grep -cE "^[a-zA-Z_][a-zA-Z0-9_ \*]*\b[a-z_0-9]+\([^;]*\)\s*\{$" c_src/main.c
  c_src/part_defs.c c_src/draw_rope.c` → 30 / 8 / 0 = **38 functions remaining**, in
  **2,212 lines** (1,649 + 503 + 60) across those three files. At wave 2's start
  (commit `f891aac`) the same measurement gave 55 functions / 2,617 lines. **Wave 2 moved
  17 functions and 405 lines.** Against the whole-port baseline (92 functions / 3,392
  lines), **54 functions (1,180 lines) are now moved in total.**

- [x] **Step 2: Update the status table in `README.md`**
- [x] **Step 3: Record the wave-2 outcome in the spec, including which ports the gate does
      not exercise**
- [x] **Step 4: Run the full gate and confirm `ALL CHECKS PASSED`**

## Remaining after this wave

Roughly 31 functions across the higher layers, including the tick driver `advance_parts`,
the collision response chain (`stub_1090_0644`, `stub_1090_033f`, `stub_1090_0809`,
`stub_1080_1777`) and `restore_parts_state_from_design`. Those get their own plan.
