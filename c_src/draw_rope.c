#include "tim.h"

// approximate_hypot_of_rope: implemented in Rust (src/tim_c.rs); prototyped in tim.h.

// calculate_rope_sag has moved to Rust (src/tim_c.rs).

#if ENABLE_TEST_SUITE
// generate_hypot_samples(int n, int max_val) used to live here as dev tooling for
// regenerating the ASSERT_EQ lines below; it has moved to src/tim_c.rs (still gated off of
// wasm32, and still never compiled here since ENABLE_TEST_SUITE is never defined).

TEST_SUITE(draw_rope) {
    TEST("approx_hypot") {
        ASSERT_EQ(approx_hypot(0, 0), 0);
        ASSERT_EQ(approx_hypot(256, 256), 352);

        // The largest hypoteneuse of equal arms possible with this function.
        ASSERT_EQ(approx_hypot(23831, 23831), 32766);
    }

    TEST("approx_hypot, random samples") {
        // generate_hypot_samples(16, 64);
        // generate_hypot_samples(8, 512);
        // generate_hypot_samples(8, 32767);
        ASSERT_EQ(approx_hypot(   39,     6),    40); // Accurate:    39.46 (error = +1.37%)
        ASSERT_EQ(approx_hypot(   41,    51),    66); // Accurate:    65.44 (error = +0.86%)
        ASSERT_EQ(approx_hypot(   17,    63),    69); // Accurate:    65.25 (error = +5.74%)
        ASSERT_EQ(approx_hypot(   10,    44),    47); // Accurate:    45.12 (error = +4.16%)
        ASSERT_EQ(approx_hypot(   41,    13),    45); // Accurate:    43.01 (error = +4.62%)
        ASSERT_EQ(approx_hypot(   58,    43),    73); // Accurate:    72.20 (error = +1.11%)
        ASSERT_EQ(approx_hypot(   50,    59),    77); // Accurate:    77.34 (error = -0.44%)
        ASSERT_EQ(approx_hypot(   35,     6),    36); // Accurate:    35.51 (error = +1.38%)
        ASSERT_EQ(approx_hypot(   60,     2),    60); // Accurate:    60.03 (error = -0.06%)
        ASSERT_EQ(approx_hypot(   20,    56),    63); // Accurate:    59.46 (error = +5.95%)
        ASSERT_EQ(approx_hypot(   27,    40),    49); // Accurate:    48.26 (error = +1.53%)
        ASSERT_EQ(approx_hypot(   39,    13),    43); // Accurate:    41.11 (error = +4.60%)
        ASSERT_EQ(approx_hypot(   54,    26),    63); // Accurate:    59.93 (error = +5.12%)
        ASSERT_EQ(approx_hypot(   46,    35),    58); // Accurate:    57.80 (error = +0.34%)
        ASSERT_EQ(approx_hypot(   51,    31),    61); // Accurate:    59.68 (error = +2.21%)
        ASSERT_EQ(approx_hypot(    9,    26),    29); // Accurate:    27.51 (error = +5.40%)

        ASSERT_EQ(approx_hypot(  358,   306),   472); // Accurate:   470.96 (error = +0.22%)
        ASSERT_EQ(approx_hypot(   13,   439),   443); // Accurate:   439.19 (error = +0.87%)
        ASSERT_EQ(approx_hypot(   49,    88),   106); // Accurate:   100.72 (error = +5.24%)
        ASSERT_EQ(approx_hypot(  163,   346),   406); // Accurate:   382.47 (error = +6.15%)
        ASSERT_EQ(approx_hypot(  293,   349),   458); // Accurate:   455.69 (error = +0.51%)
        ASSERT_EQ(approx_hypot(  261,   279),   376); // Accurate:   382.05 (error = -1.58%)
        ASSERT_EQ(approx_hypot(   88,   233),   266); // Accurate:   249.06 (error = +6.80%)
        ASSERT_EQ(approx_hypot(   94,   212),   246); // Accurate:   231.91 (error = +6.08%)
        
        ASSERT_EQ(approx_hypot(10192, 18558), 22380); // Accurate: 21172.54 (error = +5.70%)
        ASSERT_EQ(approx_hypot( 2108, 24418), 25208); // Accurate: 24508.82 (error = +2.85%)
        ASSERT_EQ(approx_hypot(21627,  8866), 24951); // Accurate: 23373.77 (error = +6.75%)
        ASSERT_EQ(approx_hypot(28064,  5736), 30215); // Accurate: 28644.19 (error = +5.48%)
        ASSERT_EQ(approx_hypot(12833, 13754), 18566); // Accurate: 18811.12 (error = -1.30%)
        ASSERT_EQ(approx_hypot(10316,  5954), 12548); // Accurate: 11910.92 (error = +5.35%)
        ASSERT_EQ(approx_hypot(17783,  9170), 21221); // Accurate: 20008.10 (error = +6.06%)
        ASSERT_EQ(approx_hypot(13844, 11637), 18207); // Accurate: 18085.25 (error = +0.67%)
    }
}
#endif