# Differential testing harness

Runs the OLD decompiled C (frozen, extracted verbatim from git history) against the NEW
Rust port on identical, deterministically generated inputs, and fails loudly -- printing
the exact diverging input -- on any disagreement.

This exists because the project's normal verification gate (`scripts/verify.sh`) only
exercises code paths that 7 specific game levels actually take at 4 tick counts.
Instrumentation showed some ported functions -- `part_borders_intersect` being the worst
case found so far -- never run under the gate at all, at any tick count. The gate can't
catch a mistranslation in code it never executes. This harness does, by running the real
original C side by side with the port.

Entry point: `tests/differential.rs` (discovered automatically by `cargo test`). It just
wires in one module per function under test, plus the shared PRNG.

## Layout

```
tests/differential.rs                     <- cargo test entry point, wires modules together
tests/differential/prng.rs                <- shared fixed-seed PRNG (SplitMix64, no dependency)
tests/differential/reference.c            <- frozen C for part_borders_intersect
tests/differential/part_borders_intersect.rs <- generator + test for part_borders_intersect
tests/differential/README.md              <- this file
build.rs                                  <- compiles tests/differential/*.c (native only)
```

## Adding a second function

1. **Find the commit to extract from.** The reference must come from the commit
   *immediately before* the function was ported (i.e. the parent of the porting commit),
   so it's the real, unmodified original. Find the porting commit with e.g.:

   ```sh
   git log --oneline -S"bool your_function_name" -- c_src/main.c
   ```

   The oldest matching commit that *removes* the function (replacing it with a "has moved
   to Rust" comment) is the porting commit; its parent (`<commit>^`) is what you extract
   from.

2. **Extract mechanically, never retype.** Find the line range of the function body in
   that commit and pull it out with `git show` + `sed`/similar -- never copy-paste by hand
   and never "clean up" anything on the way:

   ```sh
   git show <parent-commit>:c_src/main.c | sed -n '<start>,<end>p' > /tmp/body.c
   ```

   Diff the result against the extraction before and after any edits to prove the only
   change is the rename (see the diff done for `part_borders_intersect` in the commit that
   added this harness, for the exact technique).

3. **Freeze it in its own file**, `tests/differential/<function_name>.c`, with:
   - A header comment identical in spirit to the one in `reference.c`: which commit it's
     from, that it's a byte-for-byte copy, that it must never be "fixed" or modernised, and
     that the RUST changes if it ever disagrees with this file -- never the reverse.
   - The function renamed to `ref_<function_name>` (rename ONLY the symbol -- nothing else
     in the body).
   - Any local forward-declarations the original file relied on for helper functions that
     had *already* been ported to Rust by that commit (check the top of `c_src/main.c` at
     that commit for `int foo(...);` style declarations above the function) -- these aren't
     part of "the body" and reproducing them is what makes the file compile standalone, not
     a modification to the frozen logic.

4. **Compile it in `build.rs`.** Copy the existing `tests/differential/reference.c` block:
   add another `cc::Build::new().file("tests/differential/<function>.c").include("c_src").compile("tim_c_differential_<function>")`
   inside the same `if !target.starts_with("wasm32")` guard. Give every reference file its
   own `cc::Build`/lib name so they can never interfere with each other or with the real
   `tim_c` build.

5. **Write the generator + test**, `tests/differential/<function_name>.rs`, following the
   shape of `part_borders_intersect.rs`:
   - A small "spec" struct that owns whatever backing memory the C/Rust structs point into
     (so it isn't freed before the call).
   - A `handcrafted_cases()` function for the specific scenarios that matter for this
     function (equal to the "must always test" list in the original task: boundary values,
     known-tricky combinations, previously-fixed-bug shapes).
   - A `random_*` generator using `super::prng::Prng` (already fixed-seed) for broad
     coverage -- a few thousand cases is a reasonable floor.
   - One `#[test]` function that runs every case through both implementations and, on ANY
     mismatch, panics with every diverging case's exact inputs (not just a count) so it can
     be reproduced and turned into a regression case in `handcrafted_cases()`.
   - An `extern "C"` block declaring the frozen C function's signature, matched to
     whatever ABI-equivalent Rust types the crate already uses for that function's real
     parameters (see `scripts/check-ffi-signatures.py`'s type-equivalence table in its
     module doc comment if unsure what's ABI-equivalent to what).

6. **Wire it in**: add `#[path = "differential/<function_name>.rs"] mod <function_name>;`
   to `tests/differential.rs`.

7. **Prove it can fail.** Temporarily break the Rust implementation (flip a comparison,
   change a loop bound), run `cargo test --test differential`, confirm it fails and names a
   concrete diverging input, then revert. A differential test that has never been observed
   to fail is not trustworthy -- do this every time, not just once for the first function.

## Design notes worth knowing before you extend this

- **Buffers, not raw pointers to nothing.** If the function you're adding reads memory
  unconditionally (gated on a pointer being non-null, not on a separate length field), any
  input state you construct must back that pointer with real, deterministic memory covering
  every index the function can read for the count you're testing -- even for degenerate
  counts of 0 or 1 that never occur in real game data. Otherwise the harness compares
  garbage against garbage and calls it a match, or worse, is flaky. See the "A NOTE ON
  BUFFER SIZES" comment at the top of `part_borders_intersect.rs` for a worked example of
  tracing exactly which indices a loop can read.
- **The PRNG is fixed-seed on purpose.** Never seed from time/randomness -- a failure that
  can't be reproduced by re-running `cargo test` is nearly useless.
- **The reference C files are historical facts, not specs.** If a reference ever disagrees
  with the Rust port, fix the RUST. If the *original* game turns out to have a real bug and
  the Rust intentionally reproduces or deviates from it, document that decision next to the
  Rust implementation (and note the deviation in the differential test itself) -- the
  frozen `.c` file's code must stay an unmodified copy of what git history says it was.
