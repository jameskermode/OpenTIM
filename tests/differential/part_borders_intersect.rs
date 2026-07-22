//! Differential test for `part_borders_intersect`.
//!
//! Compares the NEW Rust `opentim::tim_c::part_borders_intersect` against
//! `ref_part_borders_intersect`, a frozen, byte-for-byte copy of the OLD decompiled C
//! extracted from git history (see tests/differential/reference.c for full provenance and
//! the rules governing that file). Both are driven with identical, deterministically
//! generated `Part` pairs; any disagreement fails the test and prints the exact inputs
//! that diverged.
//!
//! WHY `part_borders_intersect` FIRST
//! -----------------------------------
//! Coverage instrumentation showed this function's 10 branches all stay cold under the
//! project's normal verification gate (7 levels x 4 tick counts) -- its only caller path,
//! Pokey the Cat's walk state machine, never activates even at 3000 ticks. So a
//! mistranslation here would change collision behaviour game-wide with nothing to catch
//! it. This harness is the catch.
//!
//! A NOTE ON BUFFER SIZES (please read before changing the generator)
//! --------------------------------------------------------------------
//! `part_borders_intersect`'s *first* two memory reads (`p1bd[0]`, `p1bd[1]`) happen
//! unconditionally, gated only on `borders_data` being non-null -- NOT on `num_borders`.
//! Real game data never has `num_borders < 4` (see every `set_border(&[...])` call site in
//! src/parts/mod.rs), so this combination never arises there, but the task requires
//! exercising `num_borders` of 0 and 1 too. To do that WITHOUT reading uninitialised or
//! out-of-bounds memory (which would make the test flaky/UB instead of a real differential
//! check), this generator always backs `borders_data` with a real buffer of at least 2
//! `BorderPoint`s whenever it is non-null, even for `num_borders` 0 or 1. Both
//! implementations then read the exact same bytes, so the comparison stays meaningful and
//! deterministic. `num_borders >= 2` uses a buffer of exactly `num_borders` points, which
//! is enough: tracing the loop shows the highest index it can read is `num_borders - 1`
//! (the closing edge substitutes the cached first point instead of reading past the end).

use opentim::tim_c::{BorderPoint, Part, ShortVec};
use std::os::raw::c_int;

use super::prng::Prng;

// The frozen reference C, compiled by build.rs from tests/differential/reference.c.
extern "C" {
    fn ref_part_borders_intersect(part1: *const Part, part2: *const Part) -> c_int;
}

// Arbitrary fixed seed -- any constant works as long as it never changes, since the whole
// point is that the exact same sequence of cases runs every time.
const SEED: u64 = 0xD1FF_5EED_0000_0001;
const RANDOM_CASES: usize = 5000;

/// Everything needed to materialise one side (`part1` or `part2`) of a test case. Owns its
/// border buffer so the buffer outlives the `Part` that points into it.
struct PartSpec {
    pos: (i16, i16),
    num_borders: u16,
    borders: Vec<BorderPoint>,
    null_borders: bool,
}

impl PartSpec {
    fn no_borders() -> Self {
        PartSpec { pos: (0, 0), num_borders: 0, borders: Vec::new(), null_borders: true }
    }

    fn with_points(pos: (i16, i16), num_borders: u16, points: &[(u8, u8)]) -> Self {
        let borders = points
            .iter()
            .map(|&(x, y)| BorderPoint { x, y, normal_angle: 0 })
            .collect();
        PartSpec { pos, num_borders, borders, null_borders: false }
    }

    /// Builds the live `Part`. The returned `Part` borrows `self.borders`'s backing
    /// allocation, so `self` must outlive it (and must not be mutated in the meantime).
    fn make(&self) -> Part {
        let mut part = Part::new_zero();
        part.pos = ShortVec { x: self.pos.0, y: self.pos.1 };
        part.num_borders = self.num_borders;
        part.borders_data = if self.null_borders {
            std::ptr::null_mut()
        } else {
            self.borders.as_ptr() as *mut BorderPoint
        };
        part
    }
}

impl std::fmt::Debug for PartSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PartSpec {{ pos: {:?}, num_borders: {}, null_borders: {}, borders: {:?} }}",
            self.pos,
            self.num_borders,
            self.null_borders,
            self.borders.iter().map(|b| (b.x, b.y)).collect::<Vec<_>>()
        )
    }
}

/// Bytes biased towards the boundary values this project has already been bitten by
/// (0/255 wraparound), while still covering the full u8 range over many calls.
fn biased_byte(rng: &mut Prng) -> u8 {
    match rng.range_i64(0, 9) {
        0 => 0,
        1 => 255,
        2 => 1,
        3 => 254,
        _ => rng.next_u8(),
    }
}

/// i16 positions biased towards zero, negative, and the values that make
/// `pos + border_byte` cross the i16 wraparound boundary (this is exactly the class of bug
/// the task description calls out: byte-level arithmetic that must wrap, not saturate).
fn biased_pos(rng: &mut Prng) -> i16 {
    match rng.range_i64(0, 12) {
        0 => 0,
        1 => -1,
        2 => 1,
        3 => i16::MAX,
        4 => i16::MIN,
        5 => i16::MAX - 1,
        6 => i16::MIN + 1,
        7 => i16::MAX - 255, // + a border byte up to 255 can just touch the boundary
        8 => i16::MAX - 10,  // + a border byte can cross it
        9 => i16::MIN + 10,  // - style wrap on the low side
        10 => 16384,
        11 => -16384,
        _ => rng.next_i16(),
    }
}

fn random_points(rng: &mut Prng, count: usize) -> Vec<(u8, u8)> {
    (0..count).map(|_| (biased_byte(rng), biased_byte(rng))).collect()
}

/// The offset between the two parts in a generated pair.
///
/// This now spans the FULL i16 range, including combinations that place the two parts at
/// opposite extremes (e.g. one at `i16::MAX`, the other at `i16::MIN`). It used to be kept
/// bounded: `part_borders_intersect`'s downstream helper `calculate_line_intersection` ->
/// `math::line_intersection` did its cross-multiplication in plain (non-wrapping) `i32`,
/// sized for realistic in-level geometry, and placing two parts at opposite i16 extremes
/// overflowed that multiplication and panicked in a debug build (`attempt to multiply with
/// overflow` at src/math.rs:285) -- confirmed while writing this generator. `line_intersection`
/// has since been fixed to use `wrapping_*` arithmetic (matching the original C's silent
/// wraparound on overflow), so that panic is gone and this generator no longer needs to
/// avoid the combination. Covering the extreme range here is exactly the point: it proves
/// `part_borders_intersect` (and the `line_intersection` it calls through) agrees with the
/// frozen C reference even in the previously-overflowing cases.
fn biased_delta(rng: &mut Prng) -> i16 {
    match rng.range_i64(0, 11) {
        0 => 0,
        1 => 10,
        2 => -10,
        3 => 255,
        4 => -255,
        5 => 256,
        6 => -256,
        7 => i16::MAX,
        8 => i16::MIN,
        9 => i16::MAX - 1,
        10 => i16::MIN + 1,
        _ => rng.next_i16(),
    }
}

fn random_part_spec_at(rng: &mut Prng, pos: (i16, i16)) -> PartSpec {
    // Weighted towards the values the task calls out explicitly (0, 1, 2), plus a long
    // tail of "larger" polygons.
    let num_borders: u16 = match rng.range_i64(0, 9) {
        0 | 1 => 0,
        2 | 3 => 1,
        4 | 5 => 2,
        6 => 3,
        7 => rng.range_u16(4, 8),
        _ => rng.range_u16(9, 40),
    };

    if num_borders == 0 {
        // Mix the two distinct code paths for num_borders == 0: the realistic
        // null-borders_data invariant (the early `return 0`), and the -- unusual, but not
        // forbidden by the C -- non-null-buffer-with-zero-count combination, which the
        // loop handles as a single degenerate edge (see the module doc comment above).
        if rng.chance(2) {
            PartSpec::no_borders()
        } else {
            let pts = random_points(rng, 2);
            PartSpec::with_points(pos, 0, &pts)
        }
    } else if num_borders == 1 {
        // See the module doc comment: needs a real 2-point buffer to stay memory-safe.
        let pts = random_points(rng, 2);
        PartSpec::with_points(pos, 1, &pts)
    } else {
        let pts = random_points(rng, num_borders as usize);
        PartSpec::with_points(pos, num_borders, &pts)
    }
}

/// A curated set of cases specifically targeting the scenarios the task calls out by name:
/// clear overlap, clear disjoint, an exact shared edge, an exact shared vertex, and byte
/// boundary / 16-bit truncation interactions. The random fuzz below covers the broad space;
/// these pin down the specific behaviours that must never regress.
fn handcrafted_cases() -> Vec<(PartSpec, PartSpec)> {
    let square = |x0: u8, y0: u8, side: u8| -> Vec<(u8, u8)> {
        vec![(x0, y0), (x0 + side, y0), (x0 + side, y0 + side), (x0, y0 + side)]
    };

    let mut cases = Vec::new();

    // Clearly overlapping: two 10x10 squares offset by 5.
    cases.push((
        PartSpec::with_points((0, 0), 4, &square(0, 0, 10)),
        PartSpec::with_points((5, 5), 4, &square(0, 0, 10)),
    ));

    // Clearly disjoint: same squares, far apart.
    cases.push((
        PartSpec::with_points((0, 0), 4, &square(0, 0, 10)),
        PartSpec::with_points((1000, 1000), 4, &square(0, 0, 10)),
    ));

    // Exact shared edge: B's left edge is exactly A's right edge, no interior overlap.
    cases.push((
        PartSpec::with_points((0, 0), 4, &square(0, 0, 10)),
        PartSpec::with_points((10, 0), 4, &square(0, 0, 10)),
    ));

    // Exact shared vertex only (corner-to-corner touch).
    cases.push((
        PartSpec::with_points((0, 0), 4, &square(0, 0, 10)),
        PartSpec::with_points((10, 10), 4, &square(0, 0, 10)),
    ));

    // One part with no borders at all (realistic null-borders_data invariant) against a
    // normal square: must short-circuit to "no intersection".
    cases.push((PartSpec::no_borders(), PartSpec::with_points((0, 0), 4, &square(0, 0, 10))));
    cases.push((PartSpec::with_points((0, 0), 4, &square(0, 0, 10)), PartSpec::no_borders()));
    cases.push((PartSpec::no_borders(), PartSpec::no_borders()));

    // num_borders == 1 and == 2 as single/double-edge "polygons", overlapping and not.
    cases.push((
        PartSpec::with_points((0, 0), 1, &[(0, 0), (10, 10)]),
        PartSpec::with_points((0, 5), 1, &[(0, 0), (10, 0)]),
    ));
    cases.push((
        PartSpec::with_points((0, 0), 2, &[(0, 0), (10, 0), (10, 10)]),
        PartSpec::with_points((5, 0), 2, &[(0, 0), (0, 10), (10, 10)]),
    ));

    // Byte boundary coordinates (0 and 255) on the border points themselves.
    cases.push((
        PartSpec::with_points((0, 0), 4, &[(0, 0), (255, 0), (255, 255), (0, 255)]),
        PartSpec::with_points((100, 100), 4, &square(0, 0, 10)),
    ));
    cases.push((
        PartSpec::with_points((0, 0), 4, &[(0, 0), (255, 0), (255, 255), (0, 255)]),
        PartSpec::with_points((0, 0), 4, &[(0, 0), (255, 0), (255, 255), (0, 255)]),
    ));

    // Positions that push pos + border_byte across the i16 wraparound boundary (this
    // project's already had a real bug where such wrapping needed to happen, not saturate).
    cases.push((
        PartSpec::with_points((i16::MAX - 5, 0), 4, &square(0, 0, 10)),
        PartSpec::with_points((i16::MAX - 5, 5), 4, &square(0, 0, 10)),
    ));
    cases.push((
        PartSpec::with_points((i16::MIN + 5, 0), 4, &square(0, 0, 10)),
        PartSpec::with_points((i16::MIN + 5, 5), 4, &square(0, 0, 10)),
    ));
    // Both parts sit at the SAME extreme corner (close together, as any real colliding pair
    // would be), which still stresses each part's own pos+border wraparound.
    cases.push((
        PartSpec::with_points((i16::MAX, i16::MAX), 4, &[(0, 0), (255, 0), (255, 255), (0, 255)]),
        PartSpec::with_points((i16::MAX - 20, i16::MAX - 20), 4, &[(0, 0), (255, 0), (255, 255), (0, 255)]),
    ));

    // Two parts at OPPOSITE extremes of the full i16 range (one at i16::MAX, the other at
    // i16::MIN). This used to be deliberately excluded: it drove `math::line_intersection`'s
    // internal i32 cross-multiplication past i32::MAX and panicked in a debug build
    // (`attempt to multiply with overflow` at src/math.rs:285), which aborted the process
    // because the call path runs through the `extern "C"` `calculate_line_intersection`.
    // `line_intersection` now uses `wrapping_*` arithmetic (matching the original C's silent
    // wraparound), so this combination is exactly the fidelity case worth pinning down: it
    // must return without panicking AND agree with the frozen C reference.
    cases.push((
        PartSpec::with_points((i16::MAX, i16::MAX), 4, &[(0, 0), (255, 0), (255, 255), (0, 255)]),
        PartSpec::with_points((i16::MIN, i16::MIN), 4, &[(0, 0), (255, 0), (255, 255), (0, 255)]),
    ));
    cases.push((
        PartSpec::with_points((i16::MIN, i16::MAX), 4, &[(0, 0), (255, 0), (255, 255), (0, 255)]),
        PartSpec::with_points((i16::MAX, i16::MIN), 4, &[(0, 0), (255, 0), (255, 255), (0, 255)]),
    ));

    // Larger polygon (octagon-ish) against a square.
    cases.push((
        PartSpec::with_points(
            (0, 0),
            8,
            &[(8, 0), (23, 0), (31, 8), (31, 23), (23, 31), (8, 31), (0, 23), (0, 8)],
        ),
        PartSpec::with_points((15, 15), 4, &square(0, 0, 10)),
    ));

    cases
}

fn run_one(part1: &Part, part2: &Part) -> (bool, bool) {
    let rust_result = opentim::tim_c::part_borders_intersect(part1, part2) != 0;
    let c_result = unsafe { ref_part_borders_intersect(part1, part2) != 0 };
    (rust_result, c_result)
}

#[test]
fn part_borders_intersect_matches_frozen_c_reference() {
    let mut rng = Prng::new(SEED);

    let mut cases: Vec<(PartSpec, PartSpec)> = handcrafted_cases();
    for _ in 0..RANDOM_CASES {
        // part2's position is a bounded offset from part1's (see `biased_delta`'s doc
        // comment for why the offset must stay bounded rather than being fully independent).
        let pos1 = (biased_pos(&mut rng), biased_pos(&mut rng));
        let pos2 = (
            pos1.0.wrapping_add(biased_delta(&mut rng)),
            pos1.1.wrapping_add(biased_delta(&mut rng)),
        );
        cases.push((random_part_spec_at(&mut rng, pos1), random_part_spec_at(&mut rng, pos2)));
    }

    let total = cases.len();
    let mut mismatches: Vec<(usize, String)> = Vec::new();

    for (i, (spec1, spec2)) in cases.iter().enumerate() {
        let part1 = spec1.make();
        let part2 = spec2.make();

        let (rust_result, c_result) = run_one(&part1, &part2);

        if rust_result != c_result {
            mismatches.push((
                i,
                format!(
                    "case #{i}:\n  part1 = {:?}\n  part2 = {:?}\n  rust part_borders_intersect() = {}\n  C   ref_part_borders_intersect() = {}",
                    spec1, spec2, rust_result, c_result
                ),
            ));
        }
    }

    if !mismatches.is_empty() {
        let shown: Vec<&String> = mismatches.iter().take(20).map(|(_, s)| s).collect();
        panic!(
            "part_borders_intersect DIVERGES from the frozen C reference in {} of {} cases.\n\
             Showing up to 20 diverging cases (full inputs, reproducible from SEED = {:#x}):\n\n{}\n",
            mismatches.len(),
            total,
            SEED,
            shown
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n\n")
        );
    }

    eprintln!("part_borders_intersect: {} cases checked against the frozen C reference, 0 mismatches", total);
}
