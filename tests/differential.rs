//! Differential testing harness: runs OLD (frozen, extracted-from-git-history) C
//! implementations against their NEW Rust ports on identical, deterministically generated
//! inputs, and fails loudly -- with the exact diverging input -- on any disagreement.
//!
//! WHY THIS EXISTS
//! ---------------
//! The project's verification gate (scripts/verify.sh) drives the simulation through 7
//! game levels at 4 tick counts and compares against golden baselines and cross-build-config
//! output. That's a strong check for code that actually RUNS during those drives -- but
//! coverage instrumentation showed some ported functions never execute at all under it.
//! `part_borders_intersect` is the worst case measured so far: 0 of its 10 branches run
//! under the gate at any tick count, because its only caller path (Pokey the Cat's walk
//! state machine) never activates. A mistranslation there would change collision behaviour
//! game-wide with nothing to catch it -- the C still exists in git history, so this harness
//! runs it side-by-side with the Rust instead of trusting the port unverified.
//!
//! HOW A CASE IS RUN
//! -----------------
//! For each function under test:
//!   1. Its original C body is extracted VERBATIM from the git commit immediately before it
//!      was ported (never retyped), renamed only to avoid a symbol clash, and frozen in its
//!      own file under tests/differential/ (see tests/differential/reference.c for the
//!      first one). build.rs compiles it into a separate static lib for native test builds
//!      only -- see the comment there for why it can't be gated more tightly than that, and
//!      why that's safe.
//!   2. A generator (a plain Rust module here, e.g. tests/differential/part_borders_intersect.rs)
//!      builds many concrete inputs using the shared fixed-seed PRNG in
//!      tests/differential/prng.rs, so any failure is reproducible just by re-running
//!      `cargo test`.
//!   3. Both the Rust export and the frozen C are called with the SAME inputs and their
//!      results compared; any mismatch fails the test and prints the full diverging input.
//!
//! ADDING A SECOND FUNCTION
//! ------------------------
//! See tests/differential/README.md for the step-by-step process (extracting the reference
//! from history, wiring it into build.rs, and writing a generator module).

#[path = "differential/prng.rs"]
mod prng;

#[path = "differential/part_borders_intersect.rs"]
mod part_borders_intersect;
