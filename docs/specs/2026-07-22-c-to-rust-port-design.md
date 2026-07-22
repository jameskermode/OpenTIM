# Porting the remaining C to Rust

Status: approved, not yet started
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
