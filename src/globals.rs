//! The simulation's global state, formerly c_src/globals.c.
//!
//! These keep their original C names because the remaining C code links against these
//! symbols directly. Many are short-lived temporaries the original used to pass values
//! between calls; they are preserved as-is during the port and retired in phase 2.
//!
//! Every static here corresponds 1:1 to a `GLOBAL(...)` entry in `c_src/globals.h`, which
//! remains the authoritative list of names, C types and initial values. `c_src/globals.h`
//! now only declares these as `extern`; the definitions (and the storage backing them)
//! live here.

use crate::tim_c::Part;

/// Concrete layout for `struct Llama` (`c_src/tim.h`), field-for-field, so pointers shared
/// with the still-C code (which walks/reorders these same nodes through `struct Llama *`)
/// see an identical memory layout. `initialize_llamas` (src/tim_c.rs) is the only code that
/// allocates or frees these nodes, and it does so with libc `malloc`/`free` rather than
/// Rust's allocator, since other C code still frees/reads nodes it never touched.
#[repr(C)]
pub struct Llama {
    pub next: *mut Llama,
    pub part: *mut Part,
    pub force: i32,
}

/// A signature matching `void (*)(struct Part *)` (`c_src/tim.h`), the type of `MEL_JUMPY`.
/// `Option<unsafe extern "C" fn(..)>` has the same representation as the bare function
/// pointer, with `None` as the null value the C initialiser (`0`) used.
pub type PartFn = unsafe extern "C" fn(*mut Part);

/* TIMWIN: 1108:3a6c */
#[no_mangle]
pub static mut PART_3a6c: *mut Part = std::ptr::null_mut();
/* TIMWIN: 1108:3a6a */
#[no_mangle]
pub static mut PART_3a6a: *mut Part = std::ptr::null_mut();
/* TIMWIN: 1108:3a68 */
#[no_mangle]
pub static mut PART_3a68: *mut Part = std::ptr::null_mut();
/* TIMWIN: 1108:3a8e */
#[no_mangle]
pub static mut TMP_QUADRANT: u16 = 0;
/* TIMWIN: 1108:3a90 */
#[no_mangle]
pub static mut TMP_BOUNCE_ANGLE_3a6c: u16 = 0;
/* TIMWIN: 1108:3a92 */
#[no_mangle]
pub static mut TMP_MOVEMENT_ANGLE_3a6c: u16 = 0;

#[no_mangle]
pub static mut TMP_X2_3a6c: i16 = 0;
#[no_mangle]
pub static mut TMP_Y2_3a6c: i16 = 0;
#[no_mangle]
pub static mut TMP_X_CENTER_3a6c: i16 = 0;
#[no_mangle]
pub static mut TMP_Y_CENTER_3a6c: i16 = 0;
#[no_mangle]
pub static mut TMP_X_DELTA_3a6c: i16 = 0;
#[no_mangle]
pub static mut TMP_Y_DELTA_3a6c: i16 = 0;
#[no_mangle]
pub static mut TMP_X_LEFTMOST_3a6c: i16 = 0;
#[no_mangle]
pub static mut TMP_Y_TOPMOST_3a6c: i16 = 0;
#[no_mangle]
pub static mut TMP_X_RIGHT_3a6c: i16 = 0;
#[no_mangle]
pub static mut TMP_Y_BOTTOM_3a6c: i16 = 0;

#[no_mangle]
pub static mut TMP_X_3a6a: i16 = 0;
#[no_mangle]
pub static mut TMP_Y_3a6a: i16 = 0;
#[no_mangle]
pub static mut TMP_X_CENTER_3a6a: i16 = 0;
#[no_mangle]
pub static mut TMP_Y_CENTER_3a6a: i16 = 0;
#[no_mangle]
pub static mut TMP_X_RIGHT_3a6a: i16 = 0;
#[no_mangle]
pub static mut TMP_Y_BOTTOM_3a6a: i16 = 0;

/* TIMWIN: 1108:0cc8 */
#[no_mangle]
pub static mut SQUIRREL: i16 = 0;

/// TIMWIN: 1108:3e47. Ranges from 0 to 128 inclusive.
#[no_mangle]
pub static mut AIR_PRESSURE: u16 = 67;

/// TIMWIN: 1108:3e49. Ranges from 0 to 512 inclusive.
#[no_mangle]
pub static mut GRAVITY: u16 = 272;

/* TIMWIN: 1108:3e43 */
#[no_mangle]
pub static mut BONUS_1: u16 = 0;

/* TIMWIN: 1108:3e45 */
#[no_mangle]
pub static mut BONUS_2: u16 = 0;

/* TIMWIN: 1108:3e4f */
#[no_mangle]
pub static mut MUSIC_TRACK: u16 = 0x03e9;

/* Codename. TIMWIN: 1108:3e53 */
#[no_mangle]
pub static mut GOOBER_ARRAY: [*mut Part; 6] = [std::ptr::null_mut(); 6];

/* Codename. TIMWIN: 1108:0c5e */
#[no_mangle]
pub static mut MEL_JUMPY: Option<PartFn> = None;

/* Codename. TIMWIN: 1108:3be6 */
#[no_mangle]
pub static mut LLAMA_1: *mut Llama = std::ptr::null_mut();

/* Codename. TIMWIN: 1108:3be8 */
#[no_mangle]
pub static mut LLAMA_2: *mut Llama = std::ptr::null_mut();

/// TIMWIN: 1108:3bfb
#[no_mangle]
pub static mut RESIZE_GOPHER: u16 = 0;

/// TIMWIN: 1108:3bfd
///
/// C declares this `enum LevelState`. On this toolchain a plain C enum is `int`-sized
/// (verified: `sizeof(enum LevelState) == 4` with the project's actual headers and
/// compiler, since nothing here uses `-fshort-enums` or a fixed underlying type), so the
/// matching Rust type is `u32`, not `u16` -- declaring it as `u16` would only back half of
/// the storage C reads and writes, corrupting whatever follows it in memory. The value
/// below is `SIMULATION_MODE` (`0x2000`) from `c_src/tim.h`.
#[no_mangle]
pub static mut LEVEL_STATE: u32 = 0x2000;

/// TIMWIN: 1108:3e69
#[no_mangle]
pub static mut SELECTED_PART: *mut Part = std::ptr::null_mut();

/// TIMWIN: 1108:3faf
#[no_mangle]
pub static mut STATIC_PARTS_ROOT: Part = Part::ZERO;
/// TIMWIN: 1108:3f0d
#[no_mangle]
pub static mut MOVING_PARTS_ROOT: Part = Part::ZERO;
/// TIMWIN: 1108:3e6b
#[no_mangle]
pub static mut PARTS_BIN_ROOT: Part = Part::ZERO;

/// TIMWIN: 1108:35f4. Probably specific to the Windows 3.1 port.
#[no_mangle]
pub static mut VALUES_PER_PIXEL: u32 = 16;
