# OpenTIM (fork)

An in-progress open source implementation of "The Incredible Machine" and "The
Even More! Incredible Machine".

> **This is a fork** of [mrfixit2001/OpenTIM](https://github.com/mrfixit2001/OpenTIM),
> originally written by Danny Spencer. The upstream project reverse-engineered the
> simulation from the 16-bit Windows build of TEMIM; all of that work is theirs.
>
> This fork exists to get the project building and running again on a modern
> toolchain (Apple Silicon in particular), and to carry it forward. See
> [Changes in this fork](#changes-in-this-fork).

## You need your own copy of the game

**This repository contains no game data, and never will.** OpenTIM is an
implementation of the game engine only. To run it you must supply the original
data files from a copy of the game that **you legally own** — an original disc,
or a purchase from a digital storefront that sells it.

Do not commit game assets to this repository, and do not ask for copies of them
in issues or pull requests. The `game-data/` directory is gitignored for exactly
this reason.

## Building

Struct layouts are generated from the CSVs in `structures/` and are not checked
in, so generate them before the first build:

```sh
mkdir -p c_src/generated src/generated
uv run --no-project python scripts/generate-structs.py      > c_src/generated/structs.h
uv run --no-project python scripts/generate-structs.py --rs > src/generated/structs.rs
```

(The generator is stdlib-only; plain `python3` works just as well.)

Then:

```sh
cargo build
cargo test
```

Put your game files somewhere under `game-data/`, e.g. `game-data/tim1`
containing `RESOURCE.MAP` and `RESOURCE.00*`:

```sh
cargo run -- game-data/tim1 L6.LEV --play
```

Window controls: **Space** run/pause, **B** collision-border overlay,
**S** screenshot, **G** graphviz dump, **Esc** quit.

Other modes:

```sh
opentim <game-dir> --list-resources              # list the archive index
opentim <game-dir> --extract <NAME> <out-file>   # extract a raw archive payload
opentim <game-dir> --dump-images <dir> [filter]  # decode sprites to PPM
opentim <game-dir> <level> [ticks]               # headless: load, step, dump parts
opentim <game-dir> <level> --screenshot <out.ppm> [ticks] [--borders]
```

A level is either a saved machine on disk (`CATOMATC.TIM`) or the name of a
puzzle inside the archive (`L6.LEV`).

## Browser build

OpenTIM also runs in the browser. The simulation is the same C core cross-compiled to
wasm32, and it produces bit-identical results to the native build.

Prerequisites:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.126
brew install zig     # Apple clang has no WebAssembly backend
```

Then:

```sh
./scripts/build-web.sh          # writes web/pkg
python3 -m http.server -d web 8080
```

Open <http://localhost:8080/> and drop in your own game folder — the one holding
`RESOURCE.MAP` and `RESOURCE.00*`. Nothing is uploaded; the files are read in the browser
and the whole game runs locally.

`web/test-auto.html` is a development harness that fetches the game data over HTTP instead
of asking for a drop, so the browser path can be checked without clicking. Serve the repo
root rather than `web/` to use it, and put the data in `game-data/tim1`.

## Status

The asset pipeline and level parsing are complete and verified against the
original DOS data. The simulation core runs. What is missing is part behaviour
and the entire editing UI.

| Area | State |
|---|---|
| Asset pipeline (LZW/LZHUF, BMP/SCN, VGA palette) | Complete — all 484 TIM 1 sprites decode |
| Level parsing | Works — titles, objectives, gravity, parts |
| Simulation core | Works — gravity, collision, bounce |
| Part behaviour | 28 of 66 parts implemented |
| Levels | 7 of the 87 shipped puzzles load and simulate |
| Browser build | Works, bit-identical to native |
| Editing / design mode | Not started — see `docs/specs/` |
| C-to-Rust port | Layer 0 complete — 37 of 92 legacy C functions (781 of 3,392 lines) moved to Rust; 55 functions / 2,611 lines remain in `c_src/main.c`, `part_defs.c` and `draw_rope.c`. See `docs/specs/2026-07-22-c-to-rust-port-design.md`. |

Levels that fail do so only because they contain parts that are still
`unimplemented()`. Missing parts cluster by theme: everything electrical, all
weapons and pyrotechnics, and most characters. The playable seven are
`L6`, `L20`, `L21`, `L24`, `L25`, `L31` and `L79`; the browser build offers only
those, and `parts::is_implemented` is what decides. That list is checked against
reality by a test which creates every part type under `catch_unwind`, so it cannot
go stale as parts get ported.

"The Incredible Machine 2" (`TIM2.EXE`, 1994) is **not** supported. Its levels
use magic `0xACEF` against TIM 1's `0xACED` and carry extra fields.

## Changes in this fork

* Builds on modern Rust and on Apple Silicon. The pinned 2020 nannou master
  pulled `winit 0.22`, which cannot compile for `aarch64-apple-darwin`.
* The renderer is now a software rasterizer (`src/render.rs`) drawing into a
  640x480 framebuffer via `minifb`, which suits a palette-indexed 2D sprite game
  and builds in seconds. The old nannou renderer is kept behind the `gui`
  feature for reference and does not build.
* Levels can be loaded directly out of `RESOURCE.MAP` rather than only from
  loose files on disk.
* Added `--list-resources`, `--extract`, `--dump-images` and `--screenshot`.
* Fixed a null-pointer dereference on unlinked pulleys, and an integer underflow
  on zero-height walls (the original truncated to a byte and wrapped).
* Added a WebAssembly build (upstream listed this as a stretch goal). The C core is
  cross-compiled with `zig cc`, freestanding wasm has no libc so `src/wasm_libc.rs`
  supplies the handful of symbols the core needs, and `src/web.rs` drives a canvas from a
  requestAnimationFrame loop. Verified bit-identical to native across every loadable level.
* Added `CLAUDE.md` with architecture notes, and a design spec for the editor in
  `docs/specs/`.

## Fixed: the simulation used to depend on optimisation level

Debug and release builds once simulated differently — balloons diverged on the same
machine purely from `-O0` versus `-O2`. The cause was not undefined behaviour in the C, as
first assumed, but an **ABI mismatch at the FFI boundary**: 22 functions were exported with
`#[no_mangle] pub fn`, giving them the *Rust* ABI, while C called them as C functions. The
Rust ABI is explicitly unspecified, so how a small integer return lands in the register is
a codegen decision that changes with optimisation level.

The visible symptom: `part_acceleration()` returns −198 for a balloon, but the C caller
testing `part_acceleration(part->type) < 0` read it as non-negative at `-O0` and took the
wrong branch when snapping a resting part's sub-pixel position.

Every `#[no_mangle]` export now declares `extern "C"`. Debug and release agree across all
loadable levels, and the wasm build agrees with both. When adding an export, always give it
`extern "C"` — without it the code will appear to work and then diverge under optimisation.

## Goals

Inherited from upstream:

* Run on desktop platforms (Windows, Mac, Linux). Requires the user to provide
  the original game assets.
* WebAssembly is a stretch goal. Not actively targeted, but kept in mind.
* Simulation should be as accurate to the original game as possible.
* Keep simulation quirks. Bugs carried over from the original may be fixed if
  there is reasonable consensus that they are disruptive or game-breaking.

## Dev notes

All deliberate deviations from the original game's behaviour are labelled
`VANILLAFIX`. Comments of the form `/* TIMWIN: 10a8:1e46 */` give the
segment:offset of the corresponding routine in the original executable — keep
them when moving code, they are the cross-reference to the disassembly.

See `CLAUDE.md` for a fuller architecture guide.

## License

GPL-3.0, as upstream.
