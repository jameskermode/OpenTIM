# Porting the remaining C to Rust

Status: in progress — layer 0 complete
Date: 2026-07-22

## Goal

Move the remaining 3,392 lines of decompiled C into Rust so that `c_src/` and `build.rs`
can be deleted. The build then needs no C toolchain at all: no `cc` crate, and no `zig` for
the WebAssembly target.

Three things motivate it beyond tidiness:

* **The build gets simpler.** The wasm build currently needs `zig cc`, a `zig ar` archiver,
  hand-written libc shims and generated libc headers, all because Apple clang has no
  WebAssembly backend. All of that disappears with the C.
* **A whole bug class disappears.** The FFI boundary already produced one silent
  correctness bug: 22 functions were exported with the Rust ABI while C called them as C
  functions, and the simulation differed between `-O0` and `-O2` as a result. Every
  function that migrates removes a place where that can happen.
* **One language to debug.** The current split means a divergence can come from either
  half, as the optimisation-level investigation showed.

## What is left

| File | Lines | Functions |
|---|---|---|
| `c_src/main.c` | 2,466 | 80 |
| `c_src/part_defs.c` | 755 | 32 |
| `c_src/draw_rope.c` | 171 | included above |

3,392 lines across those files, of which 3,038 sit inside the 92 function bodies; the rest
is declarations, comments and the `#if ENABLE_TEST_SUITE` blocks.

92 function definitions in total, of which **23 are still named `stub_XXXX_XXXX`** — the
upstream author decompiled them but never established their purpose, so they are named
after their address in the original executable.

There are 39 globals, including about 20 short-lived `TMP_*` temporaries the original used
to pass values between calls, referenced 129 times.

## Decisions

1. **Transliterate first, clean up second.** Phase 1 is a mechanical port that preserves
   structure exactly: same control flow, same globals, same raw-pointer linked lists, same
   `stub_` names. Phase 2 refactors behind the harness, where any behaviour change shows up
   immediately. Porting and restructuring at the same time would make a divergence
   ambiguous, which matters enormously in code where 23 functions are not understood.

2. **Bottom-up, leaves first.** The FFI surface then shrinks monotonically and every step
   is independently verifiable.

3. **Interleave the reverse-engineering.** Identify each unidentified function with Ghidra
   as the port reaches it, rather than blocking the port on it or deferring it entirely.

## The call graph is a clean DAG

Measured: **92 functions, zero mutual recursion**. Every function can move independently;
there are no clusters that must migrate together.

| Layer | Functions | Lines | Unidentified | Notable members |
|---|---|---|---|---|
| 0 | 45 | 819 | 8 | `belt_data_alloc`, `calculate_intersecting_rect`, `bucket_contains` |
| 1 | 25 | 690 | 7 | `insert_part_into_*`, `part_alloc_borders`, `calculate_rope_sag` |
| 2 | 14 | 799 | 5 | `restore_parts_state_from_design`, `part_find_interactions`, `adjust_part_position` |
| 3 | 4 | 239 | 2 | `balloon_run`, `pokey_the_cat_run` |
| 4 | 2 | 330 | 1 | `teeter_totter_run`, `stub_10a8_3cc1` |
| 5 | 1 | 33 | 1 | `stub_1080_1777` |
| 6 | 1 | 128 | 0 | `advance_parts` (the tick driver, ported last) |

Note that **8 of the 23 unidentified functions are leaves**, so the Ghidra track starts
immediately rather than at the end — on the simplest functions, which is a gentle on-ramp.

## Technique

### Moving one function

Rewrite it in Rust as `#[no_mangle] pub extern "C" fn` with the same name and signature,
then delete the C body. Remaining C callers still link, so the build and the entire
verification harness stay green at every commit. Keep the `/* TIMWIN: seg:off */` comments
and keep `stub_` names until the function is actually identified.

### Use raw pointers, never references

Ported functions must take `*mut Part`, not `&mut Part`. While both halves exist, C holds
pointers to the same objects, so handing Rust a `&mut` asserts an exclusivity that does not
hold. That is undefined behaviour of exactly the kind that works until the optimiser
notices — the same failure mode as the ABI bug. References can be reintroduced in phase 2,
once nothing else aliases the data.

### Globals move first

Move the 39 globals from `globals.c` into Rust as `#[no_mangle]` statics, reducing
`globals.h` to plain `extern` declarations. One mechanical, independently verifiable step
that avoids adding `extern` declarations piecemeal across 92 later ports.

### Preserve 16-bit semantics exactly

The helpers `utos`, `uneg`, `mul32` and the byte-truncating `SWAP` macro encode the
original's 16-bit arithmetic. Transliterate them precisely; do not "improve" them. Rust
equivalents must use `wrapping_*` and explicit casts. A wall of size 16x0 in `L25.LEV`
already depends on byte truncation wrapping to 255.

## Verification

Run after **every** function moves. Formalise as `scripts/verify.sh`.

The gate needs **two independent comparisons**, because they catch different failures:

1. **Against a committed golden baseline.** Part state for 7 levels at 0, 30, 120 and 300
   ticks, captured from the pre-port code and committed as fixtures. This is what catches a
   transliteration that got the arithmetic subtly wrong.
2. **Across build configurations** — debug == release == wasm. This is what caught the ABI
   bug, and catches platform-specific divergence the baseline cannot see.

Configuration comparison alone is **not sufficient**, and assuming it was is a mistake this
spec previously made. All three configurations are rebuilt from the same source, so a port
that changes behaviour identically everywhere leaves them mutually consistent and passes.
That is the single most likely way a bad port slips through, so the baseline is the more
important of the two.

The baseline fixtures must only ever change when a behaviour change is intended and
justified in the commit message. A port task changing them means the port is wrong.

3. `cargo test` — 40 tests.
4. The reload harness: part counts after any load order match a fresh load.

`cargo run --example trace -- <dir> <level> <ticks> <part-type>` dumps one part's internal
state per tick; diffing two traces localises any divergence to an exact tick and field.

## The Ghidra track

Unblocked and verified. `~/Downloads/TemIM3x/CD/TEMIM.EXE` matches all three hashes pinned
in `reverse-engineering/README.md`, so it is byte-for-byte the binary every `TIMWIN:`
annotation refers to. It is a 16-bit NE executable, 34 segments, importing GDI, KERNEL and
USER — exactly the properties the upstream README gives for choosing the Windows build.

Use `CD/TEMIM.EXE`, **not** `TIMWIN/TEMIM.EXE`: the latter has been patched by
`CD/patch/PATCH.EXE` and has different hashes, so its addresses will not match.

Static analysis needs only the file. Ghidra runs natively on macOS and no emulation is
involved. Two known unknowns: Ghidra is not installed yet, and the repo's scripts target
Ghidra 9.1.2 with Jython, so expect some porting to a current release.

For behavioural questions there is also a working dynamic oracle — see
`scripts/run-temim-win.sh`. Two traps are documented there: 386 enhanced mode will not
start from a mounted host folder (use standard mode, or build a real disk image with
`imgmake`), and `WIN.COM` lives in `C:\WINDOWS` so the game needs an absolute path.

## Risks

* **Aliasing UB** is the one that bites silently. Mitigated by the raw-pointer rule.
* **Transliteration errors in 16-bit arithmetic** — mitigated by the per-function gate, but
  only for behaviour the 7 loadable levels actually exercise. Levels that cannot yet load
  cover code paths the harness never reaches.
* **Ghidra script compatibility** is unknown until tried.
* **Scale.** 3,038 lines of function bodies across 92 functions is many sessions. The layering makes it
  interruptible: any prefix of the order leaves a working build.

## Definition of done

Phase 1 is complete when `c_src/`, `build.rs` and `src/wasm_libc.rs` are deleted, the
`cc` build-dependency is gone from `Cargo.toml`, `scripts/build-web.sh` no longer needs
zig, and the full verification gate passes.

Phase 2 (cleanup: retiring the `TMP_*` globals, replacing raw pointers with safe
abstractions, renaming identified functions) is a separate effort with its own spec.

## Layer 0 outcome

Measured against the baseline above (3,392 lines, 92 function definitions across
`c_src/main.c`, `part_defs.c` and `draw_rope.c`):

* **37 functions (781 lines) moved to Rust.** `c_src/` is down to 55 function definitions
  in 2,611 lines across those three files (`main.c` 1,922/44, `part_defs.c` 590/10,
  `draw_rope.c` 99/1). `cat c_src/*.c | wc -l` reports 2,614 — that total also includes the
  1-line `globals.c` placeholder left after Task 2 moved the 39 globals to
  `src/globals.rs`, and an unrelated 2-line `foo.c` fixture.
* **Four functions were identified for the first time** via the Ghidra setup from Task 5,
  recorded in full in `docs/reverse-engineering-setup.md`: `stub_1050_025e` →
  `set_bounce_side_flags`, `stub_10a8_4509` → `llama2_insert_by_force`, `stub_10a8_2bea` →
  `queue_dirty_rect`, and `stub_10a8_28f6` → `queue_rope_dirty_rects`. The latter two are
  **deliberate no-ops** in the Rust port: Ghidra's decompilation showed the originals were
  not stubs at all but real dirty-rectangle bookkeeping for the legacy GDI blitter (queuing
  and deduplicating screen-space redraw rectangles as ropes and other parts moved). That
  logic has no effect on simulation state — it only fed a renderer that no longer exists,
  since this project's software rasterizer repaints every frame unconditionally. The C
  bodies were already no-ops for exactly this reason, and the Rust ports preserve that.

### The verification gap

The test gate (`scripts/verify.sh`) exercises exactly **7 of the 87 shipped levels**
(`L6`, `L20`, `L21`, `L24`, `L25`, `L31`, `L79`) at **4 tick counts** (0, 30, 120, 300).
That is a real but narrow slice of the simulation, and several functions ported in layer 0
are **not exercised by it at all** — their correctness rests on transliteration care and
code review, not on the gate having actually run them. Known unexercised functions:

* `teeter_totter_helper_1`
* `rope_calculate_flags` (probably — no loadable level's rope/pulley configuration has been
  confirmed to reach every branch)
* the design-mode branch of `part_set_prev_vars` (guarded by `LEVEL_STATE == 0x1000`,
  which headless simulation runs never enter)
* `balloon_rope`
* `teeter_totter_helper_get_part_speed`
* `generate_hypot_samples` (dead code in every build — the original's own callers are
  commented out in `c_src/draw_rope.c`, and nothing calls the Rust port either)
* the three `UNIMPLEMENTED` stubs (`stub_10a8_1329`, `stub_10a8_28a5`, `stub_10a8_0880`),
  which panic if ever called and are only proven never to be called by the 7 loadable
  levels, not by any broader guarantee

This gap **widens in later layers**, not shrinks. Much of `part_defs.c` implements parts
(electrical components, weapons, characters) that no currently loadable level contains, so
layers 1-6 will move progressively more code the gate cannot see running at all. The
direct way to close the gap is to widen level coverage — every additional part type
implemented (see the README status table) turns levels that currently fail to load into
levels the gate can add to its rotation, which is a strictly better source of confidence
than code review alone.

### What the gate gained during layer 0

Later layers depend on infrastructure the gate did not have before this phase:

* **Golden baselines** in `tests/baselines/` (28 files: 7 levels × 4 tick counts),
  captured from known-good pre-port code. These catch a port that computes the wrong
  answer identically in every build configuration, which a debug/release/wasm comparison
  alone cannot see.
* **A C compiler diagnostics check** that fails the gate on
  `-Wimplicit-function-declaration` or `-Wincompatible-pointer-types` from the system
  compiler. Both warning classes indicate real C type errors that `cc-rs` compiles anyway
  without failing `cargo build`, so they were previously invisible to normal development.
* **A TIMWIN-tag completeness check** that fails the gate if any exported
  `#[no_mangle] pub extern "C" fn` in `src/tim_c.rs` lacks its own `TIMWIN:` provenance
  comment (against a small, manually-audited allowlist of project-infrastructure exports
  that were never part of the original TIM code). This was added after the tag went
  missing silently twice, when a new doc comment was written directly under a previous
  function's doc comment with no blank line between them and rustdoc merged both `///`
  blocks onto the second function.


## Deferred: continuous integration

There is no CI. The verification gate is strong but entirely opt-in, and nothing stops a
change landing without it having been run.

Full enforcement is impossible here — the 28 baseline comparisons need the user's own game
files, which cannot be uploaded. But most of the gate does NOT need them, and already runs
under `ALLOW_NO_GAME_DATA=1`:

* native debug and release builds, and the wasm build
* the unit tests
* the C diagnostics check (`-Wimplicit-function-declaration`, `-Wincompatible-pointer-types`)
* the TIMWIN provenance-tag check
* the FFI signature check (C prototypes vs Rust `extern "C"` definitions)

That is the whole structural safety net minus the behavioural comparison, and it would catch
the majority of what has actually gone wrong during this port. Worth adding a workflow that
runs `ALLOW_NO_GAME_DATA=1 ./scripts/verify.sh`, with the limitation stated plainly so nobody
mistakes a green tick for behavioural verification.
