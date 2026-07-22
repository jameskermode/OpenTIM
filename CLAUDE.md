# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

OpenTIM is an in-progress open-source reimplementation of "The Incredible Machine" and "The Even More! Incredible Machine". It is reverse-engineered from the 16-bit *Windows* build of TEMIM (referred to throughout the code as "TIMWIN"), and reads the original game's assets at runtime — the repo ships no game data.

The overriding goal is simulation accuracy to the original, *including* its quirks. Do not "fix" surprising behaviour unless it is clearly game-breaking; deliberate deviations are labelled `VANILLAFIX` in a comment.

## Build & run

Struct layouts are code-generated and **gitignored**, so a fresh checkout does not build until you generate them:

```sh
mkdir -p c_src/generated src/generated
uv run --no-project python scripts/generate-structs.py      > c_src/generated/structs.h
uv run --no-project python scripts/generate-structs.py --rs > src/generated/structs.rs
```

Re-run both after editing anything in `structures/` (`Part.csv`, `RopeData.csv`, `BeltData.csv`). Never hand-edit the generated files. The generator is stdlib-only, so `--no-project` (no venv) is enough.

```sh
cargo build                  # headless by default
cargo test                   # 39 tests, all passing
cargo test sine_cosine_equivalence   # single test (tests live in #[cfg(test)] mods next to the code)
```

`build.rs` compiles the C half (`cc` crate) into `libtim_c`. Its `c_sources` list is explicit — a new `.c` file will be silently ignored until you add it there.

### Rendering: `src/render.rs`, not nannou

`src/render.rs` is a **software rasterizer** drawing into a 640x480 framebuffer, shown with `minifb`. The game is a palette-indexed 2D sprite blitter, so this matches it directly, builds in seconds, and avoids a GPU stack entirely. Sprite pixels the SCN decoder never plotted keep alpha 0, which is what `Canvas::blit` treats as transparent.

`src/nannou.rs` is the **dead** original renderer, kept behind the `gui` feature for reference. It cannot build: `Cargo.lock` pins nannou to a 2020 git commit → `winit 0.22.2` + `cocoa 0.20.2`, and on `aarch64-apple-darwin` `objc 0.2.7` defines `BOOL = bool` while winit passes `i8`. It also pins `BackendBit::VULKAN`, which macOS lacks. `cargo build --features gui` reproduces the failure. Port anything you need out of it into `render.rs` rather than trying to revive it.

Window keys: `Space` run/pause, `B` border debug overlay, `S` screenshot, `G` graphviz dump to `out.gv`, `Esc` quit.

### WebAssembly

`./scripts/build-web.sh` builds the browser version into `web/pkg`, then serve `web/`.

The crate is a lib + bin: `src/lib.rs` holds everything, `src/main.rs` is the desktop CLI
(gated off for wasm), and `src/web.rs` is the browser entry point. Build the **lib only**
for wasm — the bin and cdylib would otherwise collide on the same output name.

Things that are easy to get wrong here, all learned the hard way:

- Apple clang has no wasm backend, so `build.rs` drives `zig cc` for wasm targets.
- The `cc` crate only emits `--target` for compilers it *recognises*. It does not
  recognise the zig wrapper, so the target flag must be passed explicitly, otherwise zig
  builds for the host.
- The host `ar` writes an archive `wasm-ld` cannot read. Use `zig ar`.
- Both of those failures are silent: `wasm32-unknown-unknown` turns undefined symbols into
  module *imports*, so the link succeeds and you get a working-looking `.wasm` with the
  entire C engine missing. **Check the import section, not the exit code.**
- Freestanding wasm has no libc. `src/wasm_libc.rs` provides malloc/free/calloc/abs;
  memcpy/memset come from `compiler_builtins` and `sqrtf` is a native instruction.
- The browser cannot run a blocking loop, so `src/web.rs` uses requestAnimationFrame and
  paces the simulation from wall-clock time. Do not tick once per frame: on a 120Hz display
  that runs the game at four times speed.

To check the wasm engine against native, build with `--target nodejs` and diff
`Game::parts_summary()` against the CLI's dump at the same tick count and **the same
optimisation level** (see the known issue below).

### The FFI boundary must use `extern "C"`

Every `#[no_mangle]` export in `tim_c.rs` and `parts/mod.rs` must be declared
`pub extern "C" fn`. `#[no_mangle] pub fn` compiles and links fine but gives the function
the *Rust* ABI, which is explicitly unspecified, while C calls it as a C function.

This caused a real bug: debug and release builds simulated differently, because
`part_acceleration()` returned -198 for a balloon and the C caller testing `< 0` read it as
non-negative at `-O0`. It looked like undefined behaviour in the C core and was not.
Symptoms of this class are opt-level-dependent behaviour and small integer returns that
seem to arrive corrupted.

Two consequences worth knowing:

- A panic unwinding out of an `extern "C"` function aborts rather than being catchable, so
  `catch_unwind` cannot wrap a Rust callback invoked through C. `implemented_matches_reality`
  calls the part callbacks directly for exactly this reason.
- `c_src/tim.h` declares `s16 part_mass(...)` while Rust returns `u16`. Harmless at current
  value ranges, but the declarations and signatures should be kept in step.

To check simulation changes, compare **like-for-like profiles**, and use
`cargo run --example trace -- <dir> <level> <ticks> <part-type>` to dump one part's internal
state per tick; diffing two traces localises a divergence to the exact tick and field.

### CLI

`argv[1]` is always the game install directory (the one holding `RESOURCE.MAP`). Assets are kept in the gitignored `game-data/` — `game-data/tim1` (DOS TIM 1) and `game-data/tim2` (TIM 2). Never commit anything under it.

```sh
cargo run -- game-data/tim1 L6.LEV --play
```

```sh
opentim <game-dir> --list-resources              # archive index
opentim <game-dir> --extract <NAME> <out-file>   # raw archive payload
opentim <game-dir> --dump-images <dir> [filter]  # decode sprites to PPM
opentim <game-dir> <level> [ticks]               # load, step, dump parts (headless)
opentim <game-dir> <level> --play                # window, 30Hz
opentim <game-dir> <level> --screenshot <out.ppm> [ticks] [--borders]
```

`--screenshot` renders a frame without needing a display, which is the way to check rendering changes from a terminal. Convert with `uv run --no-project --with pillow python -c "from PIL import Image; Image.open('out.ppm').save('out.png')"`.

The level argument is either a path to a saved machine on disk (parsed as **freeform**, e.g. `CATOMATC.TIM`) or the name of an entry inside the archive (parsed as a **puzzle**, e.g. `L6.LEV`), decompressed via `decoders::generic_decode` if needed.

## Architecture

### Two-language body, one data model

The project is mid-migration: `c_src/` is a fairly direct transliteration of the decompiled original, and `src/` is where code moves as it is understood. Both halves manipulate the *same* `struct Part` objects (malloc'd in C, `#[repr(C)]` on the Rust side) through three global intrusive linked lists declared in `c_src/globals.h`:

- `STATIC_PARTS_ROOT` — playfield parts that don't move
- `MOVING_PARTS_ROOT` — simulated parts, kept sorted by mass
- `PARTS_BIN_ROOT` — the parts bin, kept sorted by `part_order`

C walks them with the `EACH_STATIC_PART` / `EACH_MOVING_PART` / `EACH_INTERACION` macros in `c_src/tim.h`; Rust walks them with `tim_c::static_parts_iter()` / `moving_parts_iter()` (and `_mut` variants). All of it is `unsafe` by nature — the lists mutate during iteration in the original's algorithms.

Because the world lives in globals, **loading a level must first tear down the previous
one** — `lib.rs::clear_level()` does this and `load_level` calls it. Nothing else empties
the lists, so skipping it silently accumulates parts across loads. `part_free` owns the
rules about what to release: borders always, belt data unless `F2_0001` marks it shared,
and `rope_data[0]` only for ropes and pulleys since everything else just points at a rope
owned elsewhere.

`src/tim_c.rs` is the entire FFI boundary:
- an `extern` block importing C functions and globals (`advance_parts`, `part_new`, `GRAVITY`, `AIR_PRESSURE`, the list roots…),
- `#[no_mangle]` Rust functions that C calls back into (`part_mass`, `part_run`, `part_bounce`, `part_data30_flags1`, `sine_c`, …), declared C-side in `c_src/tim.h`,
- `include!("./generated/structs.rs")`.

### Part definitions live in Rust

`src/parts/mod.rs` holds one private module per part type, each exporting `const DEF: PartDef` — physical constants (density, mass, bounciness, friction), flags, sizes, render images/offsets, and optional `create_fn` / `reset_fn` / `run_fn` / `bounce_fn` / `flip_fn` / `resize_fn` / `rope_fn` callbacks. `parts::get_def(PartType)` dispatches; the `#[no_mangle]` accessors in `tim_c.rs` are how the C simulation reads them. These tables correspond to "Segment 30" and "Segment 31" data in the original executable.

**The migration pattern:** a part whose behaviour hasn't been ported yet keeps its logic in `c_src/part_defs.c` and its `DEF` callback forwards there via the `run_c!` / `bounce_c!` / `reset_c!` / `flip_c!` / `resize_c!` / `rope_c!` macros (defined in the `parts::prelude` module). Porting a part means rewriting the body in Rust and deleting the macro use. Conversely, C code that needs a Rust-only helper declares it in `part_defs.c` under the `// From Rust` comment, and the Rust side marks it `#[no_mangle]`.

In practice most `*_c!` calls you will see are **commented out**, sitting above an `unimplemented()` — a placeholder for a C body that was never written. Only `teeter_totter`, `balloon` and `pokey_the_cat` actually delegate to C today.

## How complete is this? (measured 2026-07-21)

- **Parts:** all 67 modules exist with real constants and tables, but only **29 have working `create`+`reset`**; the other 38 hit `unimplemented()` at *load* time. Notably missing: everything electrical (`generator`, `light_switch_outlet`, `lightbulb`, `electric_engine`, `solar_panels`, `fan`), all weapons/pyro (`gun`, `cannon`, `dynamite`, `rocket`, `explosion`), and most characters (`mel_schlemming`, `mort_the_mouse`, `kelly_the_monkey`, `ernie_the_alligator`).
- **Levels:** of the 87 `L*.LEV` puzzles in the DOS TIM 1 archive, **7 load and simulate** (L6, L20, L21, L24, L25, L31, L79). The other 80 fail only on unimplemented parts. Physics genuinely runs — L6 rolls a basketball down an incline into a trampoline and bounces it.
- **Assets: complete.** `--list-resources` reads all 159 TIM 1 / 1467 TIM 2 entries, and `--dump-images` decodes all 484 TIM 1 sprites correctly (LZW/LZHUF + BMP/SCN + VGA palette). Sprite sheets are named `PART<n>.BMP` where `<n>` is the `PartType` discriminant.
- **Level formats:** magic `0xACED` (TIM 1) parses. `0xACEE` is Toons. **`0xACEF` — "The Incredible Machine 2" (`TIM2.EXE`, 1994) — is NOT supported**; its levels report `BadMagic(44271)` and carry extra fields (per-level hint text). Do not assume TIM2 data works.

### Using DOSBox as an oracle

Comparing against the running original is the way to fill in missing parts and check fidelity, and the repo is already built for it — but note the two tiers:

- **Vanilla DOSBox** (`/Applications/dosbox.app`, 0.74) runs `TIM.EXE` fine and is a usable *behavioural* oracle (watch what a part actually does), but it has **no debugger**, so `memdumpbin` is unavailable.
- **DOSBox-X** (`/Applications/dosbox-x.app`, installed; its arm64 binary does contain the debugger) is what `reverse-engineering/README.md` and `scripts/read-acceleration-from-segment-31.py` assume: the debugger dumps memory, `scripts/deserialize-parts.py` turns a dump into JSON, and `reverse-engineering/partScrubber.html` inspects it. That gives exact per-tick `Part` structs to diff against the headless `opentim <dir> <level> <ticks>` dump.

Mount `game-data/tim1` in DOSBox-X and run `TIM.EXE` to compare against the original.

### Simulation tick

`c_src/main.c` is the core: `advance_parts()` is one tick (a long, order-sensitive sequence of passes over the lists — static parts, gears, teapots, buckets, velocity/force, collision, bounce), followed by `all_parts_set_prev_vars()` which rolls `pos`/`state1` into their `_prev1`/`_prev2` slots. Part state carries two frames of history because the original interpolates and rope-draws from it. `restore_parts_state_from_design()` resets everything to the loaded design.

`src/atmosphere.rs` computes acceleration and terminal velocity from gravity + air pressure + density. TIMWIN precalculated this into a table whenever the control panel changed; OpenTIM recomputes per call. `src/math.rs` holds the original's fixed-point sine/cosine/arctan lookups — angles are `u16` over a full turn, and results are `-0x4000..=0x4000`.

### Assets and levels

- `src/resource_dos.rs` — reads the DOS distribution's `RESOURCE.MAP` + `.RES` archives (Sierra/Dynamix format, filename-hash lookup).
- `src/decoders/{lzw,lzhuf}.rs` — the two compression schemes used inside those archives.
- `src/image/bmp_scn.rs`, `src/image/scr.rs` — sprite decoders producing RGBA using `TIM.PAL`.
- `src/level_file_format.rs` — parses `.LEV` designs; the `GameOptions` enum selects between TIM puzzle/freeform and Sid & Al's Incredible Toons header layouts, and fields are gated on the file's version word.
- `src/level_load.rs` — pre-allocates every `Part` first, then fills them in, because parts reference each other by index in both directions.
- `src/nannou.rs` — window, texture cache, layering (`goobers.0` is the draw layer), and the update/draw loop.

## Conventions

- `/* TIMWIN: 10a8:1e46 */` comments are the segment:offset of the corresponding routine in the original executable. They are the primary cross-reference to the disassembly — preserve them when moving or rewriting code, and mark partial ports as `Partial from TIMWIN: …`.
- Anything not yet understood gets a **codename**, not a number: `LLAMA`, `GOOBER`, `SQUIRREL`, `RESIZE_GOPHER`, `stub_10a8_21cb`. Unknown struct fields are `field_0xNN`; unknown flag bits are `F1_0004`, `F2_0200`, etc., and get renamed once their meaning is known (`F1_EPHEMERAL`, `F3_LOCKED`).
- Unported code paths: `UNIMPLEMENTED` macro in C, `unimplemented()` (panics) in `parts::prelude`.
- Integer semantics matter. The original is 16-bit and relies on wrapping and two's-complement reinterpretation; use the `utos`/`uneg`/`mul32` helpers in C and `wrapping_*` / `angle_to_signed` in Rust rather than plain casts.

## Reverse-engineering tooling

Only relevant when extending the port from the disassembly (see `reverse-engineering/README.md` for the methodology and TEMIM.EXE hashes):

- `reverse-engineering/ghidra-scripts/` — Jython scripts for Ghidra 9.1.2 (labelling Win16 imports by ordinal, patching odd call/stack idioms, reading part tables). Add `scripts/` as a Ghidra script directory; `scripts/tim_structures.py` is shared between Ghidra (Jython 2.6) and Python 3, so keep it compatible with both.
- `scripts/deserialize-parts.py` — turns a DOSBoxX memory dump into JSON for the `reverse-engineering/partScrubber.html` inspector.
- `scripts/read-acceleration-from-segment-31.py` — generates the acceleration/terminal-velocity fixtures used by the `atmosphere.rs` tests.
- `structures/*.csv` is the single source of truth for struct layouts, feeding the code generator, the Ghidra scripts, and the JS tools alike.
