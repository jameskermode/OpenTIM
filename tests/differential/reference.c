/*
 * ============================================================================
 * FROZEN REFERENCE -- extracted verbatim from git history. DO NOT "FIX" THIS FILE.
 * ============================================================================
 *
 * This is `part_borders_intersect` exactly as it existed in the original decompiled C,
 * copied byte-for-byte from commit cd62fa1a99567f9dc34ce53f3af53a51bce87d7e
 * ("port: move teeter_totter_bounce to Rust"), which is the commit immediately BEFORE
 * `part_borders_intersect` itself was ported to Rust in commit 218f8b6
 * ("port: move part_borders_intersect to Rust").
 *
 * Extracted mechanically, not retyped:
 *   git show cd62fa1a99567f9dc34ce53f3af53a51bce87d7e:c_src/main.c | sed -n '518,590p'
 *
 * The ONLY edit made to the body below is renaming the function from
 * `part_borders_intersect` to `ref_part_borders_intersect`, so it can be linked into the
 * same test binary as the Rust `#[no_mangle] pub extern "C" fn part_borders_intersect`
 * (src/tim_c.rs) without a symbol clash. Nothing else has been changed: no reformatting,
 * no "cleanup", no modernisation, no bug fixes -- not even the `bool` return type (this
 * codebase's `bool` is `typedef int bool`, see c_src/int.h, so it is ABI-compatible with
 * the `c_int` the Rust side returns).
 *
 * This file is used ONLY by the differential test harness
 * (tests/differential/part_borders_intersect.rs), which runs the Rust and C
 * implementations against identical generated inputs and fails loudly on any
 * disagreement. It is compiled by build.rs for native test builds only -- see the guard
 * there -- and is never linked into the game binary or the wasm build.
 *
 * If this reference ever disagrees with the Rust port, that means the PORT is wrong:
 * the RUST changes, never this file. This file records a historical fact (what the
 * original C actually did), not an opinion about what it should have done. If a genuine
 * bug is later confirmed in the *original* game's behaviour and the Rust deliberately
 * reproduces or deviates from it, document that decision next to the Rust
 * implementation and/or in the differential test -- but the C code below must stay an
 * unmodified copy of what git history says it was.
 *
 * See tests/differential/README.md for how to add a reference for another function.
 * ============================================================================
 */

#include "tim.h"

// At commit cd62fa1, `calculate_line_intersection` had already been ported to Rust (see
// src/tim_c.rs) but c_src/tim.h did not yet carry its prototype -- c_src/main.c declared it
// itself, as a local forward declaration at the top of the file (line 12 of
// `git show cd62fa1:c_src/main.c`). Reproduced verbatim here (only) so this file can see the
// same declaration `part_borders_intersect`'s body relied on; it links against the real
// `#[no_mangle] pub extern "C" fn calculate_line_intersection` in src/tim_c.rs, unmodified.
int calculate_line_intersection(const struct Line *a, const struct Line *b, struct ShortVec *out);

bool ref_part_borders_intersect(const struct Part *part1, const struct Part *part2) {
    u16 p1bi = 1;
    struct BorderPoint *p1bd = part1->borders_data;
    if (!p1bd) return 0;

    s16 p1b0x = part1->pos.x + p1bd[0].x;
    s16 p1b0y = part1->pos.y + p1bd[0].y;
    s16 p1b1x = part1->pos.x + p1bd[1].x;
    s16 p1b1y = part1->pos.y + p1bd[1].y;
    s16 p1origin_x = p1b0x;
    s16 p1origin_y = p1b0y;

    while (p1bd) {
        struct Line line1 = { {0, 0}, {p1b1x - p1origin_x, p1b1y - p1origin_y} };
        four_points_adjust_p1_by_one(&line1);

        u16 p2bi = 1;
        struct BorderPoint *p2bd = part2->borders_data;
        if (p2bd) {
            s16 p2b0x = part2->pos.x + p2bd[0].x;
            s16 p2b0y = part2->pos.y + p2bd[0].y;
            s16 p2b1x = part2->pos.x + p2bd[1].x;
            s16 p2b1y = part2->pos.y + p2bd[1].y;
            s16 p2origin_x = p2b0x;
            s16 p2origin_y = p2b0y;

            while (p2bd) {
                struct Line line2 = { {p2origin_x - p1origin_x, p2origin_y - p1origin_y},
                                      {p2b1x - p1origin_x, p2b1y - p1origin_y} };
                four_points_adjust_p1_by_one(&line2);

                struct ShortVec intersection;
                bool intersects = calculate_line_intersection(&line1, &line2, &intersection);

                if (intersects && !VEC_EQ(intersection, line1.p1)) {
                    return 1;
                }

                p2bi += 1;
                if (p2bi > part2->num_borders) {
                    p2bd = 0;
                } else {
                    p2origin_x = p2b1x;
                    p2origin_y = p2b1y;
                    if (part2->num_borders == p2bi) {
                        p2b1x = p2b0x;
                        p2b1y = p2b0y;
                    } else {
                        p2b1x = part2->pos.x + p2bd[2].x;
                        p2b1y = part2->pos.y + p2bd[2].y;
                    }
                    p2bd += 1;
                }
            }
        }
        p1bi += 1;
        if (p1bi > part1->num_borders) {
            p1bd = 0;
        } else {
            p1origin_x = p1b1x;
            p1origin_y = p1b1y;
            if (part1->num_borders == p1bi) {
                p1b1x = p1b0x;
                p1b1y = p1b0y;
            } else {
                p1b1x = part1->pos.x + p1bd[2].x;
                p1b1y = part1->pos.y + p1bd[2].y;
            }
            p1bd += 1;
        }
    }
    return 0;
}

