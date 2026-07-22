# Design mode editor

Status: approved, not yet implemented
Date: 2026-07-22

## Goal

Add the design half of the game: a parts palette down the right edge, freeform
placement of parts onto the playfield, a small set of per-part controls, and the
two-mode structure where editing is frozen, **start** hands the world to the
simulator, and **stop** restores the pre-run state exactly.

Today the project is a simulation core with a viewer. There is no input handling
of any kind, `SELECTED_PART` is never assigned, and `part_flip()`/`part_resize()`
are exported to C with zero callers.

## Key finding: snapshot/restore already exists

The obvious way to implement mode transitions is to serialise the level on
*start* and reload it on *stop*. **That is not necessary, and would be less
faithful.** The original stores the design in-band, in each part:

```
Part:      original_pos_x, original_pos_y, original_state1,
           original_state2, original_flags2, links_to_design[2]
RopeData:  original_part1, original_part2, original_part{1,2}_rope_slot
```

`restore_parts_state_from_design()` (`c_src/main.c:530`) is already a complete
stop transition. It:

* restores position, `state1`, `state2` and `flags2` from the `original_*` fields
* zeroes velocity, `extra1`/`extra2`, and `bounce_part`
* frees every `F1_EPHEMERAL` part (bullets, explosions, severed rope ends) and
  unlinks it from its list
* restores rope topology from `original_part1`/`original_part2`, walking pulley
  chains, and recomputes belt positions
* calls `part_reset()` to regenerate collision borders

It is already called on every level load, which is how levels come up clean.

The consequence for this design: **the level-format writer is not on the critical
path for mode transitions.** It is still built (see below) because saving and
exchanging levels is worth having, but the two concerns are independent.

## Second finding: `flags1 & 0x4000` means "static part"

Placement has to decide whether a newly placed part joins `STATIC_PARTS_ROOT` or
`MOVING_PARTS_ROOT`. `level_load` never had to decide, because the file format
stores the two lists separately.

Measured across all 240 parts in the 7 loadable levels, `flags1 & 0x4000`
predicts the list with 100% agreement. The flag is currently the unnamed
`F1_4000` in `c_src/tim.h`; rename it to `F1_STATIC` per the repo convention of
naming flags once their meaning is known.

## Scope

In v1:

* Both palette modes: puzzle (bin-driven) and freeform (all implemented parts)
* Place, move, flip horizontally, delete, recycle back to the bin
* Belt connections
* Start / stop with correct snapshot-restore
* Save and load levels (writer half of the level file format)
* Chrome drawn with the original shipped art

Out of v1:

* **Ropes** — they thread through pulleys as a linked chain that
  `restore_parts_state_from_design` has to walk and rebuild. v2.
* **Resize / stretch handles** — `RESIZE_GOPHER` and the per-part `resize_fn`
  callbacks already exist and are unused, so this is a cheap follow-up. Worth
  doing early in v2 since walls in real puzzles are stretchable.
* Gravity / air pressure sliders, win detection, sound, undo, palette scrolling.

## Architecture

`render.rs` stays a pure rasterizer (`Canvas`, blit, line, polyline) and holds no
editor state. New module tree:

| Module | Responsibility |
|---|---|
| `editor/mod.rs` | Mode state machine; editor state (held part, selection, active tool) |
| `editor/palette.rs` | Palette model and hit-testing; two sources (bin / freeform) |
| `editor/chrome.rs` | Draws bin, control panel and playfield border from the shipped art |
| `editor/input.rs` | Mouse/keyboard events to editor actions |
| `level_file_format.rs` | Gains `write()` |

### Mode transitions

Design state lives in the `original_*` fields. Editing mutates those, never the
live fields directly.

* **Edit** — after any edit, call `restore_parts_state_from_design()` to derive
  live state from design state. This is the single existing code path for that
  derivation and is already exercised on every level load, so edit-sync and
  stop-restore cannot drift apart. Part counts are under 50, so the cost is
  irrelevant.
* **Start** — restore, then `LEVEL_STATE = SIMULATION_MODE`. Runs always begin
  from the design.
* **Stop** — `restore_parts_state_from_design()`, then
  `LEVEL_STATE = DESIGN_MODE`. Ephemeral cleanup and rope/belt rebuild come free.

Edit mode is frozen by not calling `advance_parts()`. No serialisation is
involved in either transition.

### Screen layout

Measured across the 7 loadable levels, part coordinates span x in [0, 552] and
y in [0, 345]. The playfield occupies the top-left of the 640x480 screen, leaving
roughly 88px down the right edge for the parts bin and 135px across the bottom
for the control panel. Part coordinates are therefore already screen coordinates:
chrome goes in the margins and no transform is needed.

Art: `ICONS.BMP` (58 slices, indexed by `PartType`, blanks for non-placeable
parts such as gun bullet) for palette icons, `CP.BMP` for control panel buttons,
`GP_BORD.BMP` for the playfield border. Positions are laid out from the measured
margins and refined by eye against DOSBox-X screenshots.

### Palette

* **Puzzle mode** lists `PARTS_BIN_ROOT`, which `level_load` already populates
  from the level file (L6 has 3 entries, L21 has 6, L31 has 5). Every entry is
  guaranteed placeable, because a level only loads at all if all of its parts,
  bin included, are implemented.
* **Freeform mode** lists the implemented parts. This must come from an
  `implemented` predicate on `PartDef`, not a hand-maintained list, because
  placing an unimplemented part panics in `part_reset`.

### Placement

1. Pick up. Puzzle mode detaches the existing `Part` instance from the bin list;
   freeform mode calls `part_new(type)`.
2. The held part follows the cursor.
3. Drop is validated with `part_collides_with_playfield_part()` (`main.c:973`,
   currently only used by Pokey the Cat's walk code). Invalid drops are refused
   and the part stays held. No grid snap.
4. Commit sets `original_pos_x`/`original_pos_y` and inserts into the static or
   moving list according to `flags1 & 0x4000`.

Recycle reverses this: remove from the playfield list, return to the bin in
puzzle mode or free in freeform mode.

### Per-part controls

* **Flip horizontal** via the existing `part_flip()`, which sets `F2_FLIP_HORZ`
  and dispatches to the part's `flip_fn`.
* **Lock** via the existing `F3_LOCKED`.
* **Delete**.
* **Belt tool** — pick two parts, allocate with `part_init_belt_data` (which only
  allocates and back-links), then wire `part1`/`part2`, `belt_loc` and
  `belt_width` mirroring what `level_load.rs` does for file-loaded belts.

### Writer

`level_file_format::write(&Level) -> Vec<u8>`, mirroring `read()` with the same
version gating, plus a `Level`-from-live-parts snapshot which is the inverse of
`level_load`.

## Testing

* **Writer round-trip** — read, write, re-read all 87 shipped `L*.LEV` plus the 3
  `.TIM` saves and assert semantic equality. This validates the writer against 90
  real files regardless of the part stubs, because parsing is independent of
  simulation. Skipped when `game-data/` is absent.
* **Mode transition** — load a level, capture the design fields, simulate N
  ticks, stop, then assert live state equals design state for every part. Strong
  regression test that needs no UI.
* **Placement** — unit tests on the collision-validity predicate.
* **Manual** — the 7 loadable levels.

## Risks

* `part_flip()` and `part_resize()` have never been called in the lifetime of the
  codebase. Expect latent bugs in the per-part `flip_fn` implementations.
* Chrome positions remain guesses until compared against DOSBox-X.
* The freeform `implemented` predicate is a drift risk if it is not derived from
  the part definitions themselves.

## Prerequisites

None blocking. Related work that would compound:

* Porting more of the 38 unimplemented parts widens the freeform palette and
  unlocks more of the 87 levels.
* Deleting the dead `src/nannou.rs` (509 lines, unbuildable) before adding UI
  code, to avoid two competing renderers in the tree.
