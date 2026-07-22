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

## Status

The asset pipeline and level parsing are complete and verified against the
original DOS data. The simulation core runs. What is missing is part behaviour
and the entire editing UI.

| Area | State |
|---|---|
| Asset pipeline (LZW/LZHUF, BMP/SCN, VGA palette) | Complete — all 484 TIM 1 sprites decode |
| Level parsing | Works — titles, objectives, gravity, parts |
| Simulation core | Works — gravity, collision, bounce |
| Part behaviour | 29 of 67 parts implemented |
| Levels | 7 of the 87 shipped puzzles load and simulate |
| Editing / design mode | Not started — see `docs/specs/` |

Levels that fail do so only because they contain parts that are still
`unimplemented()`. Missing parts cluster by theme: everything electrical, all
weapons and pyrotechnics, and most characters.

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
* Added `CLAUDE.md` with architecture notes, and a design spec for the editor in
  `docs/specs/`.

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
