// Rust/C interop layer

use std::os::raw::{c_int, c_char};
use crate::part::PartType;
use crate::atmosphere;
use crate::parts;

/**** Import C declarations to Rust ****/
extern {
    pub fn part_new(part_type: c_int) -> *mut Part;
    pub fn part_init_rope_data_primary(part: *mut Part);
    pub fn part_init_belt_data(part: *mut Part);
    pub fn part_alloc_borders(part: *mut Part, length: u16);
    pub fn part_calculate_border_normals(part: *mut Part);
    pub fn part_set_size_and_pos_render(part: *mut Part);
    pub fn restore_parts_state_from_design();
    pub fn advance_parts();
    pub fn all_parts_set_prev_vars();
    pub fn insert_part_into_static_parts(part: *mut Part);
    pub fn insert_part_into_moving_parts(part: *mut Part);
    pub fn insert_part_into_parts_bin(part: *mut Part);
    pub fn calculate_rope_sag(part: *const Part, rope_data: *const RopeData, time: c_int) -> i16;

    pub fn stub_10a8_21cb(part: *mut Part, c: u8);
    pub fn stub_10a8_2b6d(part: *mut Part, c: c_int);
    pub fn stub_10a8_280a(part: *mut Part, c: c_int);
    pub fn search_for_interactions(part: *mut Part, choice: c_int, search_x_min: i16, search_x_max: i16, search_y_min: i16, search_y_max: i16);
}

// `part_alloc_borders` (still C, above) allocates `borders_data` with libc `malloc` and is
// NOT part of this port, so the buffer it hands out must still be released with libc `free`
// rather than Rust's `dealloc` (which would reconstruct a `Layout` the allocator that made
// the memory never used). Import `free` itself rather than moving `part_alloc_borders` /
// `part_free_borders`.
//
// `initialize_llamas` (below) uses the same libc `malloc`/`free` pair for `struct Llama`
// nodes, for the same reason: other still-C code (e.g. `stub_10a8_4509`) walks and reorders
// those same nodes and expects to be able to free ones it never allocated, and vice versa,
// so every `Llama` allocation anywhere must go through this one allocator.
extern "C" {
    fn free(ptr: *mut std::os::raw::c_void);
    fn malloc(size: usize) -> *mut std::os::raw::c_void;
}

/// TIMWIN: 1078:00f2 (allocation half)
///
/// Safety: `alloc_zeroed` either returns a valid, freshly zeroed `Part`-sized allocation or
/// null; the null case is passed straight through to the caller (matching the C, which also
/// returned `0` from `malloc` failure) rather than being dereferenced here, so there is no
/// dereference in this function at all.
#[no_mangle]
pub extern "C" fn part_alloc() -> *mut Part {
    let layout = std::alloc::Layout::new::<Part>();
    unsafe {
        let p = std::alloc::alloc_zeroed(layout) as *mut Part;
        if p.is_null() { std::ptr::null_mut() } else { p }
    }
}

/// TIMWIN: 1078:1402
///
/// Safety: every dereference of `part` is guarded by the leading null check, matching the
/// C's `if (!part) return;`. Once past that check `part` is a live, uniquely-owned `Part`
/// (callers give up ownership when they call `part_free`, mirroring `free()`), so reading
/// its fields and freeing the buffers it points at is sound. The three interior pointers
/// (`borders_data`, `belt_data`, `rope_data[0]`) are each null-checked before use, matching
/// the C's `if (part->x) free(part->x)` guards.
///
/// Ownership rules preserved exactly from the C:
///   - `borders_data`: always freed here if non-null. It is allocated by `part_alloc_borders`
///     (still C, using `malloc`), so it is released with libc `free`, not Rust `dealloc`.
///   - `belt_data`: freed only when flag `F2_0001` is clear, matching the C's
///     `NO_FLAGS(part->flags2, F2_0001)`. When the flag is set this part merely points at a
///     belt buffer owned by another part, so freeing it here would be a double free.
///   - `rope_data[0]`: freed only when `part_type` is `Pulley` or `Rope`, matching the C's
///     `part->type == P_PULLEY || part->type == P_ROPE`. Every other part type merely links
///     to a rope owned by the pulley/rope that created it; `rope_data[1]` is never owned by
///     this part and is never freed here (matching the C, which never frees it either).
#[no_mangle]
pub extern "C" fn part_free(part: *mut Part) {
    unsafe {
        if part.is_null() {
            return;
        }

        if !(*part).borders_data.is_null() {
            free((*part).borders_data as *mut std::os::raw::c_void);
        }
        if !(*part).belt_data.is_null() && (*part).flags2 & 0x0001 == 0 /* F2_0001 clear */ {
            let layout = std::alloc::Layout::new::<BeltData>();
            std::alloc::dealloc((*part).belt_data as *mut u8, layout);
        }
        let rope0 = (*part).rope_data[0];
        if !rope0.is_null()
            && ((*part).part_type == PartType::Pulley as u16 || (*part).part_type == PartType::Rope as u16)
        {
            let layout = std::alloc::Layout::new::<RopeData>();
            std::alloc::dealloc(rope0 as *mut u8, layout);
        }

        let layout = std::alloc::Layout::new::<Part>();
        std::alloc::dealloc(part as *mut u8, layout);
    }
}

/// Safety: mirrors `part_alloc` — returns null on allocation failure without dereferencing
/// anything, or a fresh zeroed `BeltData`-sized allocation.
#[no_mangle]
pub extern "C" fn belt_data_alloc() -> *mut BeltData {
    let layout = std::alloc::Layout::new::<BeltData>();
    unsafe {
        let p = std::alloc::alloc_zeroed(layout) as *mut BeltData;
        if p.is_null() { std::ptr::null_mut() } else { p }
    }
}

/// Safety: mirrors `part_alloc` — returns null on allocation failure without dereferencing
/// anything, or a fresh zeroed `RopeData`-sized allocation.
#[no_mangle]
pub extern "C" fn rope_data_alloc() -> *mut RopeData {
    let layout = std::alloc::Layout::new::<RopeData>();
    unsafe {
        let p = std::alloc::alloc_zeroed(layout) as *mut RopeData;
        if p.is_null() { std::ptr::null_mut() } else { p }
    }
}

// Only used this for debugging purposes
///
/// Safety: no pointer dereferences at all — just reports a compile-time constant size.
#[no_mangle]
pub extern "C" fn debug_part_size() -> usize {
    std::mem::size_of::<Part>()
}

/// TIMWIN: 10a8:1e18
///
/// Safety: `part->prev` is unconditionally dereferenced, matching the C exactly
/// (`part->prev->next = part->next;` has no null check either). This is sound because every
/// part in these doubly linked lists is threaded onto a permanent sentinel root
/// (`STATIC_PARTS_ROOT` / `MOVING_PARTS_ROOT` / `PARTS_BIN_ROOT`), so `prev` is never null for
/// a part that is actually linked into a list; callers only ever call this on linked parts.
/// `part` itself is assumed non-null and valid by the same contract the C relied on (no null
/// check existed there either). `part->next` is null-checked before dereferencing, matching
/// the C's `if (part->next) { part->next->prev = part->prev; }`.
#[no_mangle]
pub extern "C" fn remove_part_from_linked_list(part: *mut Part) {
    unsafe {
        (*(*part).prev).next = (*part).next;
        if !(*part).next.is_null() {
            (*(*part).next).prev = (*part).prev;
        }
    }
}

/// Partial from TIMWIN: 10b0:02a5
///
/// Safety: `struct Llama` (`c_src/globals.rs`) nodes are allocated and freed here with libc
/// `malloc`/`free`, matching the C exactly. This is required, not just preserved-for-its-own-
/// sake: other C code (e.g. `stub_10a8_4509`) still walks and reorders `LLAMA_1`/`LLAMA_2`
/// nodes, moving them between the two lists without allocating or freeing them itself, so
/// whichever allocator creates a node must be the same one every other piece of code (C or
/// Rust) that might eventually free it uses -- mixing in Rust's `std::alloc`/`dealloc` here
/// would corrupt the heap the first time C code freed (or this function re-freed) a node
/// allocated by the other side. Both the free and the (re-)allocation for these nodes happen
/// in this single function, so there is no cross-allocator split to worry about.
///
/// The two cleanup loops walk `LLAMA_1`/`LLAMA_2` from their current heads, freeing every
/// reachable node: `cur->next` is read into `next` *before* `cur` is freed, exactly matching
/// the C's `struct Llama *next = cur->next; free(cur); cur = next;`, so freed memory is never
/// read again. This is sound as long as `LLAMA_1`/`LLAMA_2` form well-formed, non-cyclic
/// singly linked lists of live `malloc`'d nodes -- the same invariant the C relied on and
/// every other list-mutating function here preserves.
///
/// The final loop allocates 20 fresh nodes with `malloc` and unconditionally writes
/// `(*o).next = LLAMA_1` with no null check on `o`, exactly matching the C (`o->next =
/// LLAMA_1;` right after `malloc`, with no check on its result either). A `malloc` failure
/// here dereferences null in both languages equally; that is a preexisting property of the
/// C being ported, not a regression introduced by this port.
#[no_mangle]
pub extern "C" fn initialize_llamas() {
    unsafe {
        // Release any llamas from a previously loaded level. Without this, loading a second
        // level leaks the whole pool and leaves LLAMA_2 pointing at freed parts.
        let mut cur = crate::globals::LLAMA_1;
        while !cur.is_null() {
            let next = (*cur).next;
            free(cur as *mut std::os::raw::c_void);
            cur = next;
        }
        let mut cur = crate::globals::LLAMA_2;
        while !cur.is_null() {
            let next = (*cur).next;
            free(cur as *mut std::os::raw::c_void);
            cur = next;
        }

        crate::globals::LLAMA_1 = std::ptr::null_mut();
        crate::globals::LLAMA_2 = std::ptr::null_mut();
        for _ in 0..20 {
            let o = malloc(std::mem::size_of::<crate::globals::Llama>()) as *mut crate::globals::Llama;
            (*o).next = crate::globals::LLAMA_1;
            crate::globals::LLAMA_1 = o;
        }
    }
}

// The globals below now live in src/globals.rs; re-export them so existing call sites
// that refer to them as `tim_c::GRAVITY` etc. keep compiling.
pub use crate::globals::{
    AIR_PRESSURE, GRAVITY, MOVING_PARTS_ROOT, PARTS_BIN_ROOT, RESIZE_GOPHER,
    STATIC_PARTS_ROOT,
};

#[derive(Clone)]
pub struct PartsIterator<'a> {
    cur: *const Part,
    _phantom: std::marker::PhantomData<&'a Part>
}
impl<'a> PartsIterator<'a> {
    /// Initializes the iterator with a pointer to the first Part.
    /// Unsafe because 1) the pointer could be invalid, and 2) the parts could change during iteration.
    pub unsafe fn new(ptr: *const Part) -> Self {
        PartsIterator {
            cur: ptr,
            _phantom: std::marker::PhantomData
        }
    }
}
impl<'a> Iterator for PartsIterator<'a> {
    type Item = &'a Part;

    fn next(&mut self) -> Option<&'a Part> {
        let r = unsafe { self.cur.as_ref() };

        if let Some(part) = r {
            self.cur = part.next;
        }

        r
    }
}

/// Returns an iterator of static parts.
/// Unsafe because the parts could change during iteration.
pub unsafe fn static_parts_iter<'a>() -> PartsIterator<'a> {
    PartsIterator::new(STATIC_PARTS_ROOT.next)
}

/// Returns an iterator of moving parts.
/// Unsafe because the parts could change during iteration.
pub unsafe fn moving_parts_iter<'a>() -> PartsIterator<'a> {
    PartsIterator::new(MOVING_PARTS_ROOT.next)
}

#[derive(Clone)]
pub struct PartsIteratorMut<'a> {
    cur: *mut Part,
    _phantom: std::marker::PhantomData<&'a mut Part>
}
impl<'a> PartsIteratorMut<'a> {
    /// Initializes the iterator with a pointer to the first Part.
    /// Unsafe because 1) the pointer could be invalid, and 2) the parts could change during iteration.
    pub unsafe fn new(ptr: *mut Part) -> Self {
        PartsIteratorMut {
            cur: ptr,
            _phantom: std::marker::PhantomData
        }
    }
}
impl<'a> Iterator for PartsIteratorMut<'a> {
    type Item = *mut Part;

    fn next(&mut self) -> Option<*mut Part> {
        let ptr = self.cur;
        if let Some(part) = unsafe { self.cur.as_ref() } {
            self.cur = part.next;
            Some(ptr)
        } else {
            None
        }
    }
}

/// Returns an iterator of static parts.
/// Unsafe because the parts could change during iteration.
pub unsafe fn static_parts_iter_mut<'a>() -> PartsIteratorMut<'a> {
    PartsIteratorMut::new(STATIC_PARTS_ROOT.next)
}

/// Returns an iterator of moving parts.
/// Unsafe because the parts could change during iteration.
pub unsafe fn moving_parts_iter_mut<'a>() -> PartsIteratorMut<'a> {
    PartsIteratorMut::new(MOVING_PARTS_ROOT.next)
}

#[derive(Clone)]
pub struct PartInteractionsIteratorMut {
    cur: *mut Part
}
impl<'a> PartInteractionsIteratorMut {
    /// Initializes the iterator with a pointer to the first Part.
    /// Unsafe because 1) the pointer could be invalid, and 2) the parts could change during iteration.
    pub unsafe fn new(ptr: *mut Part) -> Self {
        PartInteractionsIteratorMut {
            cur: ptr
        }
    }
}
impl Iterator for PartInteractionsIteratorMut {
    type Item = *mut Part;

    fn next(&mut self) -> Option<*mut Part> {
        let ptr = self.cur;
        if let Some(part) = unsafe { self.cur.as_ref() } {
            self.cur = part.interactions;
            Some(ptr)
        } else {
            None
        }
    }
}

pub fn print_parts() {
    {
        let iter = unsafe { static_parts_iter() };
        println!("Total static parts: {}", iter.clone().count());
        for part in iter {
            println!("{:?}", part);
        }
    }
    {
        let iter = unsafe { moving_parts_iter() };
        println!("Total moving parts: {}", iter.clone().count());
        for part in iter {
            println!("{:?}", part);
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ByteVec {
    pub x: u8,
    pub y: u8,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SByteVec {
    pub x: i8,
    pub y: i8,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ShortVec {
    pub x: i16,
    pub y: i16,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct BorderPoint {
    pub x: u8,
    pub y: u8,
    pub normal_angle: u16,
}

include!("./generated/structs.rs");

use std::ops::{Deref, DerefMut};

pub struct RopeDataRefMut<'a> {
    ptr: &'a mut RopeData
}

impl<'a> Deref for RopeDataRefMut<'a> {
    type Target = RopeData;

    fn deref(&self) -> &RopeData {
        self.ptr
    }
}

impl<'a> DerefMut for RopeDataRefMut<'a> {
    fn deref_mut(&mut self) -> &mut RopeData {
        self.ptr
    }
}

pub struct BeltDataRefMut<'a> {
    ptr: &'a mut BeltData
}

impl<'a> Deref for BeltDataRefMut<'a> {
    type Target = BeltData;

    fn deref(&self) -> &BeltData {
        self.ptr
    }
}

impl<'a> DerefMut for BeltDataRefMut<'a> {
    fn deref_mut(&mut self) -> &mut BeltData {
        self.ptr
    }
}

impl Part {
    /// An all-zero Part, for the list-root globals which the C initialised with `{ 0 }`.
    pub const ZERO: Part = unsafe { std::mem::zeroed() };

    pub fn new_zero() -> Self {
        unsafe { std::mem::zeroed() }
    }
    pub fn border_points(&self) -> &[BorderPoint] {
        unsafe {
            let size = self.num_borders as usize;
            if size == 0 || self.borders_data.is_null() {
                std::slice::from_raw_parts(std::ptr::NonNull::dangling().as_ptr(), 0)
            } else {
                std::slice::from_raw_parts(self.borders_data, size)
            }
        }
    }

    pub fn bounce_part(&self) -> Option<&Part> {
        unsafe { self.bounce_part.as_ref() }
    }

    pub fn bounce_part_mut(&mut self) -> Option<&mut Part> {
        unsafe { self.bounce_part.as_mut() }
    }

    pub fn border_points_mut(&mut self) -> &mut [BorderPoint] {
        unsafe {
            let size = self.num_borders as usize;
            if size == 0 || self.borders_data.is_null() {
                std::slice::from_raw_parts_mut(std::ptr::NonNull::dangling().as_ptr(), 0)
            } else {
                std::slice::from_raw_parts_mut(self.borders_data, size)
            }
        }
    }

    // Allocates the borders to the part, and recalculates the normals
    pub fn set_border(&mut self, points: &[(u8, u8)]) {
        unsafe {
            part_alloc_borders(self, points.len() as u16);

            let b = self.border_points_mut();

            for (i, &(x, y)) in points.iter().enumerate() {
                b[i].x = x;
                b[i].y = y;
            }

            part_calculate_border_normals(self);
        }
    }

    /// Sets borders in an already-allocated buffer. Does NOT update border normals.
    /// Known quirk for parts like Bob the Fish.
    pub fn update_border_ignore_normals_quirk(&mut self, points: &[(u8, u8)]) {
        unsafe {
            if points.len() > self.num_borders as usize {
                panic!("Cannot update borders");
            }

            self.num_borders = points.len() as u16;

            let b = self.border_points_mut();

            for (i, &(x, y)) in points.iter().enumerate() {
                b[i].x = x;
                b[i].y = y;
            }
        }
    }

    pub fn init_rope_data_primary(&mut self) {
        unsafe {
            part_init_rope_data_primary(self);
        }
    }

    pub fn init_belt_data(&mut self) {
        unsafe {
            part_init_belt_data(self);
        }
    }

    pub fn rope_mut(&mut self, rope_slot: usize) -> Option<RopeDataRefMut> {
        if let Some(rope) = unsafe { self.rope_data[rope_slot].as_mut() } {
            Some(RopeDataRefMut {
                ptr: rope
            })
        } else {
            None
        }
    }

    pub fn belt_mut(&mut self) -> Option<BeltDataRefMut> {
        if let Some(belt) = unsafe { self.belt_data.as_mut() } {
            Some(BeltDataRefMut {
                ptr: belt
            })
        } else {
            None
        }
    }

    pub unsafe fn interactions_iter(&self) -> PartInteractionsIteratorMut {
        PartInteractionsIteratorMut::new(self.interactions)
    }

    /// Returns a list of tuples: ((x1, y1), (x2, y2), sag)
    pub fn rope_sections(&self) -> Option<Vec<((i16, i16), (i16, i16), i16)>> {
        if self.part_type != PartType::Rope.to_u16() { return None; }

        let mut sections = vec![];

        let rope = unsafe { self.rope_data[0].as_ref().unwrap() };
        let mut curpart_raw = rope.part1;
        let mut nextpart_raw = unsafe { curpart_raw.as_ref().unwrap() }.links_to[rope.part1_rope_slot as usize];
        if nextpart_raw.is_null() {
            nextpart_raw = rope.part2;
        }

        while !curpart_raw.is_null() && !nextpart_raw.is_null() {
            let curpart = unsafe { curpart_raw.as_ref().unwrap() };
            let nextpart = unsafe { nextpart_raw.as_ref().unwrap() };

            let pos1: ShortVec;
            if curpart.part_type == PartType::Pulley.to_u16() {
                let rpd = unsafe { curpart.rope_data[0].as_ref().unwrap() };
                pos1 = rpd.ends_pos[1];
            } else {
                pos1 = rope.ends_pos[0];
            }

            let pos2: ShortVec;
            if nextpart.part_type == PartType::Pulley.to_u16() {
                let rpd = unsafe { nextpart.rope_data[0].as_ref().unwrap() };
                pos2 = rpd.ends_pos[0];
            } else {
                pos2 = rope.ends_pos[1];
            }

            // DRAW ROPE HERE

            let sag: i16;
            if curpart.part_type == PartType::Pulley.to_u16() && nextpart.part_type == PartType::Pulley.to_u16() {
                sag = 0;
            } else {
                sag = unsafe { calculate_rope_sag(curpart, rope, 3) };
            }

            sections.push(( (pos1.x, pos1.y), (pos2.x, pos2.y), sag ));
            if sections.len() > 256 {
                // we're probably doing something wrong
                // might have ended up with a cycle somehow
                panic!("too many sections!");
            }

            curpart_raw = nextpart_raw;
            if nextpart.part_type == PartType::Pulley.to_u16() {
                nextpart_raw = nextpart.links_to[0];
            } else {
                nextpart_raw = std::ptr::null_mut();
            }
        }

        Some(sections)
    }

    pub fn belt_section(&self) -> Option<((i16, i16, i16), (i16, i16, i16))> {
        if self.part_type != PartType::Belt.to_u16() { return None; }

        let belt = unsafe { self.belt_data.as_ref().unwrap() };
        let part1 = unsafe { belt.part1.as_ref().unwrap() };
        let part2 = unsafe { belt.part2.as_ref().unwrap() };

        Some((((part1.pos_render.x + part1.belt_loc.x as i16), (part1.pos_render.y + part1.belt_loc.y as i16), (part1.belt_width as i16)),
              ((part2.pos_render.x + part2.belt_loc.x as i16), (part2.pos_render.y + part2.belt_loc.y as i16), (part2.belt_width as i16))))
    }
}

impl BeltData {
    pub fn new_zero() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

impl RopeData {
    pub fn new_zero() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[no_mangle]
pub extern "C" fn unimplemented() {
    panic!("Unimplemented");
}

#[no_mangle]
pub extern "C" fn output_c(c_str: *const c_char) {
    use std::ffi::CStr;
    let c_str = unsafe { CStr::from_ptr(c_str) };
    if let Ok(s) = c_str.to_str() {
        println!("Output: {}", s);
    } else {
        println!("Error handling string");
    }
}

#[no_mangle]
pub extern "C" fn output_part_c(ptr: *const Part) {
    let part = unsafe { ptr.as_ref() };
    if let Some(part) = part {
        println!("Output: {:?}", part);
    } else {
        println!("Output: null part");
    }
}

#[no_mangle]
pub extern "C" fn output_int_c(v: i64) {
    println!("Output: {}", v);
}

/// TIMWIN: 10a8:4d2d
#[no_mangle]
pub extern "C" fn play_sound(id: c_int) {
    println!("Play sound: {}", id);
}

/**** Export math functions to C ****/
use crate::math;

#[no_mangle]
pub extern "C" fn arctan_c(dx: i32, dy: i32) -> u16 {
    math::arctan(dx, dy)
}

/// TIMWIN: 1050:0221
/// Returns 0 to 3.
/// Accurate
///
/// Safety: no pointer dereferences at all -- pure arithmetic on the `u16` angle.
///
/// In C, `angle + 0x2000` promotes `angle` to (32-bit) `int` before adding, so the addition
/// itself never wraps -- the sum can reach 0x11FFF for `angle == 0xFFFF`. Here `wrapping_add`
/// instead truncates the sum to 16 bits before the shift. That still produces an identical
/// final answer: `>> 14` followed by `& 3` only ever looks at bits 14 and 15 of the sum,
/// and truncating to 16 bits only discards bit 16 and above, which the final `& 3` mask
/// would have discarded anyway (that carry bit lands on bit 2 after the shift, outside the
/// 2-bit mask). So the two formulations are equivalent for every `u16` input, and
/// `wrapping_add` additionally satisfies Rust's overflow checks in debug builds.
#[no_mangle]
pub extern "C" fn quadrant_from_angle(angle: u16) -> u16 {
    if angle == 0x2000 {
        return 0;
    }
    if angle == 0xa000 {
        return 2;
    }
    (angle.wrapping_add(0x2000) >> 14) & 3
}

/// TIMWIN: 1050:01e7
///
/// Safety: `part` is dereferenced unconditionally, exactly matching the C (`part->pos_prev1`
/// / `part->pos` with no null check there either). Every call site passes a part that is
/// currently being processed by the simulation (either the loop variable from
/// `EACH_STATIC_THEN_MOVING_PART`, or the `PART_3a6c` global while it points at the part
/// under consideration), so it is guaranteed non-null and pointing at a live `Part`; there is
/// no null path to preserve because the C had none either.
///
/// The subtractions mirror C's integer promotion: `part->pos_prev1.x - part->pos.x` promotes
/// both `i16` operands to (32-bit) `int` before subtracting, so the result cannot overflow
/// and no truncation happens before it reaches `arctan_c`'s `i32` parameters. Casting to
/// `i32` before subtracting reproduces that exactly.
#[no_mangle]
pub extern "C" fn part_get_movement_delta_angle(part: *mut Part) -> u16 {
    unsafe {
        let dx = (*part).pos_prev1.x as i32 - (*part).pos.x as i32;
        let dy = (*part).pos.y as i32 - (*part).pos_prev1.y as i32;
        arctan_c(dx, dy)
    }
}

/// TIMWIN: 10a8:45f8
///
/// Safety: `bucket` and `part` are both dereferenced unconditionally, exactly matching the C
/// (no null checks there either). The only caller, `bucket_add_mass_of_contained` (still C),
/// invokes this once per node of `bucket->interactions` via `EACH_INTERACION`, which never
/// yields a null node, and passes the same non-null `bucket` throughout, so both pointers are
/// always valid live `Part`s.
///
/// The sum is computed in `i32` exactly like the C's `(s32)bucket->mass + (s32)part->mass`,
/// so it cannot overflow (both `mass` fields are `i16`, the widest possible sum comfortably
/// fits in 32 bits). The clamp to 32000 matches the C exactly. Note there is no lower-bound
/// clamp: if both masses are negative the sum can go well below `i16::MIN`, and the final
/// `sum as i16` truncates/wraps to 16 bits exactly as C's `(s16)sum` cast does.
#[no_mangle]
pub extern "C" fn bucket_add_mass(bucket: *mut Part, part: *mut Part) {
    unsafe {
        let mut sum: i32 = (*bucket).mass as i32 + (*part).mass as i32;
        if sum > 32000 {
            sum = 32000;
        }
        (*bucket).mass = sum as i16;
    }
}

/// TIMWIN: 1050:00a8
/// Accurate
///
/// Safety: `PART_3a6a` (a raw `*mut Part` global) is dereferenced unconditionally, exactly
/// matching the C (no null check there either). Every call site invokes this only after
/// `PART_3a6a` has just been assigned from `get_first_part(...)` or is otherwise known to be
/// the live part currently under consideration by the bounce-resolution logic, so it is
/// non-null and valid at every call site, the same contract the C relied on.
///
/// Each `TMP_*_3a6a` output is `i16`, and every right-hand side is computed in `i32` (mirroring
/// C's integer promotion of the `i16` operands before the `+`/`>>`) and then cast down with
/// `as i16`, which truncates to 16 bits and reinterprets two's complement exactly like the
/// implicit narrowing C performs on assignment to an `s16` variable. The `>> 1` on
/// `size_something2.{x,y}` (both `i16`, promoted to `i32`) is an arithmetic (sign-extending)
/// shift in both languages, so it agrees for negative inputs too.
#[no_mangle]
pub extern "C" fn tmp_3a6a_update_vars() {
    unsafe {
        let part = crate::globals::PART_3a6a;
        crate::globals::TMP_X_3a6a = (*part).pos.x;
        crate::globals::TMP_Y_3a6a = (*part).pos.y;

        crate::globals::TMP_X_CENTER_3a6a =
            ((*part).pos.x as i32 + ((*part).size_something2.x as i32 >> 1)) as i16;
        crate::globals::TMP_Y_CENTER_3a6a =
            ((*part).pos.y as i32 + ((*part).size_something2.y as i32 >> 1)) as i16;

        crate::globals::TMP_X_RIGHT_3a6a = ((*part).pos.x as i32 + (*part).size.x as i32) as i16;
        crate::globals::TMP_Y_BOTTOM_3a6a =
            ((*part).pos.y as i32 + (*part).size.y as i32) as i16;
    }
}

/// TIMWIN: 1050:001e
/// Accurate
///
/// Safety: `PART_3a6c` (a raw `*mut Part` global) is dereferenced unconditionally, exactly
/// matching the C (no null check there either). Every call site sets `PART_3a6c` to a live
/// part immediately before calling this, the same contract the C relied on.
///
/// Each `TMP_*_3a6c` output is `i16`; every right-hand side is computed in `i32` (mirroring
/// C's promotion of the `i16` operands to `int` before the arithmetic) and then cast back
/// down with `as i16`, truncating to 16 bits exactly like C's implicit narrowing on
/// assignment to an `s16` variable. The `>> 1` on `size_something2.{x,y}` (both `i16`,
/// promoted to `i32`) is an arithmetic (sign-extending) shift in both languages.
/// `TMP_X_RIGHT_3a6c`/`TMP_Y_BOTTOM_3a6c` read back the just-stored, already-truncated
/// `TMP_X_DELTA_3a6c`/`TMP_Y_DELTA_3a6c` globals (matching the C, which does the same) rather
/// than the pre-truncation `i32` delta, and `abs()` on those never overflows `i32` since the
/// values are always sign-extended from `i16`.
#[no_mangle]
pub extern "C" fn tmp_3a6c_update_vars() {
    unsafe {
        let part = crate::globals::PART_3a6c;

        crate::globals::TMP_X2_3a6c = (*part).pos.x;
        crate::globals::TMP_Y2_3a6c = (*part).pos.y;

        crate::globals::TMP_X_CENTER_3a6c =
            ((*part).pos.x as i32 + ((*part).size_something2.x as i32 >> 1)) as i16;
        crate::globals::TMP_Y_CENTER_3a6c =
            ((*part).pos.y as i32 + ((*part).size_something2.y as i32 >> 1)) as i16;

        crate::globals::TMP_X_DELTA_3a6c =
            ((*part).pos.x as i32 - (*part).pos_prev1.x as i32) as i16;
        crate::globals::TMP_Y_DELTA_3a6c =
            ((*part).pos.y as i32 - (*part).pos_prev1.y as i32) as i16;

        crate::globals::TMP_X_LEFTMOST_3a6c = std::cmp::min((*part).pos_prev1.x, (*part).pos.x);
        crate::globals::TMP_Y_TOPMOST_3a6c = std::cmp::min((*part).pos_prev1.y, (*part).pos.y);

        crate::globals::TMP_X_RIGHT_3a6c = ((*part).pos.x as i32
            + (*part).size.x as i32
            + (crate::globals::TMP_X_DELTA_3a6c as i32).abs()) as i16;
        crate::globals::TMP_Y_BOTTOM_3a6c = ((*part).pos.y as i32
            + (*part).size.y as i32
            + (crate::globals::TMP_Y_DELTA_3a6c as i32).abs()) as i16;
    }
}

/// TIMWIN: 10a8:2485
///
/// Safety: no `Part` is ever dereferenced here. `STATIC_PARTS_ROOT`, `MOVING_PARTS_ROOT` and
/// `PARTS_BIN_ROOT` are permanent sentinel `Part`s that exist for the whole program lifetime
/// (`static mut Part`, never freed), so reading their `.next` field is always sound; that
/// field is merely returned to the caller, not dereferenced. The null path matches the C
/// exactly: if none of the three roots has a non-null `.next` for a selected list, this
/// returns a null pointer (`std::ptr::null_mut()`), exactly like the C's trailing `return 0`.
#[no_mangle]
pub extern "C" fn get_first_part(choice: c_int) -> *mut Part {
    const CHOOSE_FROM_PARTS_BIN: c_int = 0x800;
    const CHOOSE_MOVING_PART: c_int = 0x1000;
    const CHOOSE_STATIC_PART: c_int = 0x2000;
    unsafe {
        if !STATIC_PARTS_ROOT.next.is_null() && (choice & CHOOSE_STATIC_PART) != 0 {
            return STATIC_PARTS_ROOT.next;
        }
        if !MOVING_PARTS_ROOT.next.is_null() && (choice & CHOOSE_MOVING_PART) != 0 {
            return MOVING_PARTS_ROOT.next;
        }
        if !PARTS_BIN_ROOT.next.is_null() && (choice & CHOOSE_FROM_PARTS_BIN) != 0 {
            return PARTS_BIN_ROOT.next;
        }
        std::ptr::null_mut()
    }
}

/// TIMWIN: 10a8:03ac
///
/// Safety: no pointer is ever touched here. `a` and `b` are plain `enum PartType` values
/// passed by value (a bare C enum here is a 4-byte value, not a pointer), matching the C
/// signature exactly, so there is no null path to preserve.
#[no_mangle]
pub extern "C" fn should_parts_skip_collision(a: c_int, b: c_int) -> bool {
    // Checks if the two part types are a set of any two specific parts, regardless of order.
    let a = PartType::from_u16(a as u16);
    let b = PartType::from_u16(b as u16);
    let chk = |x: PartType, y: PartType| (a == x && b == y) || (b == x && a == y);

    if chk(PartType::PokeyTheCat, PartType::MortTheMouse) {
        return true;
    }
    if chk(PartType::MortTheMouse, PartType::Cheese) {
        return true;
    }
    if chk(PartType::MelSchlemming, PartType::MelsHouse) {
        return true;
    }
    if chk(PartType::MelSchlemming, PartType::MelSchlemming) {
        return true;
    }

    false
}

/// TIMWIN: 1090:158b
/// Accurate
///
/// Safety: `bucket` is dereferenced unconditionally, exactly matching the C (no null check
/// there either) -- every call site passes a currently-processed, live part (e.g. `part` /
/// `PART_3a6c` while `part->bounce_part` / `PART_3a6a` are known non-null). The
/// `EACH_INTERACION` walk only dereferences `curpart` after checking it against null in the
/// loop condition (mirroring the C macro's `varname != 0` loop test), so every node visited
/// is guaranteed live. `contains` is only ever compared by pointer value, never
/// dereferenced, so it may safely be null.
#[no_mangle]
pub extern "C" fn bucket_contains(bucket: *mut Part, contains: *mut Part) -> bool {
    unsafe {
        if (*bucket).part_type != PartType::Bucket as u16 {
            return false;
        }

        let mut curpart = (*bucket).interactions;
        while !curpart.is_null() {
            if curpart == contains {
                return true;
            }
            curpart = (*curpart).interactions;
        }
        false
    }
}

/// TIMWIN: 1020:02ba
#[repr(C)]
pub struct GDIRect {
    pub left: i16,
    pub top: i16,
    pub right: i16,
    pub bottom: i16,
}

/// TIMWIN: 1020:02ba
///
/// Safety: `out`, `a` and `b` are all dereferenced unconditionally, exactly matching the C
/// (no null checks there either). This function currently has no callers anywhere in the
/// codebase (it was dead in the C too), so there is no concrete call site to point to, but
/// the contract carried over verbatim from C is that all three point at live `GDIRect`s
/// (typically caller-owned stack locals), never null.
#[no_mangle]
pub extern "C" fn calculate_intersecting_rect(
    out: *mut GDIRect,
    a: *const GDIRect,
    b: *const GDIRect,
) -> bool {
    unsafe {
        (*out).left = (*a).left.max((*b).left);
        (*out).right = (*a).right.min((*b).right);
        (*out).top = (*a).top.max((*b).top);
        (*out).bottom = (*a).bottom.min((*b).bottom);

        (*out).left < (*out).right && (*out).top < (*out).bottom
    }
}

#[no_mangle]
pub extern "C" fn sine_c(angle: u16) -> i16 {
    math::sine(angle)
}

#[no_mangle]
pub extern "C" fn cosine_c(angle: u16) -> i16 {
    math::cosine(angle)
}

#[no_mangle]
pub extern "C" fn rotate_point_c(x: &mut i16, y: &mut i16, angle: u16) {
    let (nx, ny) = math::rotate_point(*x, *y, angle);
    *x = nx;
    *y = ny;
}

// bool calculate_line_intersection(const struct Line *a, const struct Line *b, struct ShortVec *out);

#[repr(C)]
#[derive(Debug)]
pub struct Line {
    p0: ShortVec,
    p1: ShortVec,
}

/// TIMWIN: 10a8:0290
/// Accurate
///
/// Safety: `points` is dereferenced unconditionally, exactly matching the C (no null check
/// there either); every call site passes the address of a live, stack-allocated `Line`, so
/// there is no null path to preserve.
///
/// The `+1`/`-1` adjustments are computed in `i32` (mirroring C's promotion of the `i16`
/// operand to `int` before the arithmetic) and then cast back down with `as i16`, which
/// truncates/wraps two's-complement on the boundary case (e.g. `p1.x == i16::MIN - 1`)
/// exactly like C's implementation-defined narrowing conversion on assignment back to the
/// `s16` lvalue — using plain `i16` `-=`/`+=` here would instead panic on overflow in a debug
/// build, which the C never did.
#[no_mangle]
pub extern "C" fn four_points_adjust_p1_by_one(points: *mut Line) {
    unsafe {
        if (*points).p1.x < (*points).p0.x {
            (*points).p1.x = ((*points).p1.x as i32 - 1) as i16;
        } else if (*points).p1.x > (*points).p0.x {
            (*points).p1.x = ((*points).p1.x as i32 + 1) as i16;
        }

        if (*points).p1.y < (*points).p0.y {
            (*points).p1.y = ((*points).p1.y as i32 - 1) as i16;
        } else if (*points).p1.y > (*points).p0.y {
            (*points).p1.y = ((*points).p1.y as i32 + 1) as i16;
        }
    }
}

#[no_mangle]
pub extern "C" fn calculate_line_intersection(a: *const Line, b: *const Line, out: *mut ShortVec) -> c_int {
    let a = unsafe { a.as_ref().unwrap() };
    let b = unsafe { b.as_ref().unwrap() };
    let (intersects, o) = math::line_intersection(((a.p0.x, a.p0.y), (a.p1.x, a.p1.y)),
                                                  ((b.p0.x, b.p0.y), (b.p1.x, b.p1.y)));
    
    {
        if let Some(out) = unsafe { out.as_mut() } {
            out.x = o.0;
            out.y = o.1;
        }
    }

    if intersects { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn calculate_line_intersection_helper(a: i16, b: i16, c: i16) -> c_int {
    let intersects = math::line_intersection_helper(a, b, c);

    if intersects { 1 } else { 0 }
}

/**** Ported UNIMPLEMENTED stubs (still unimplemented; ported to establish the pattern) ****/

/// TIMWIN: 10a8:1329
#[no_mangle]
pub extern "C" fn stub_10a8_1329(_belt: *mut BeltData) -> c_int {
    unimplemented!("stub_10a8_1329")
}

/// TIMWIN: 10a8:28a5
#[no_mangle]
pub extern "C" fn stub_10a8_28a5(_part: *mut Part, _unused: c_int) {
    unimplemented!("stub_10a8_28a5")
}

/// TIMWIN: 10a8:0880
#[no_mangle]
pub extern "C" fn stub_10a8_0880(_a: *mut Part, _b: *mut Part) -> *mut Part {
    unimplemented!("stub_10a8_0880")
}

static mut PART_IMAGE_SIZES: Vec<(i16, i16)> = vec![];

// Returns true if a valid image size was found.
// Returns false otherwise. size_out is unchanged in this case.
#[no_mangle]
pub extern "C" fn part_image_size(part_type: c_int, index: u16, out: *mut ShortVec) -> c_int {
    // relies on global variables for now, because the original game did.

    // In TIMWIN, this value comes from (pseudocode):
    // width  = data31[part_type].field_0x14[state].field_0x04 (16-bit signed)
    // height = data31[part_type].field_0x14[state].field_0x06 (16-bit signed)

    // some hard-coded part sizes until we implement loading them from the resource files
    let t = match PartType::from_u16(part_type as u16) {
        PartType::BowlingBall => Some((32, 32)),
        PartType::BrickWall => Some((16, 16)),
        PartType::Incline => match index {
            0 => Some((16, 32)),
            1 => Some((32, 32)),
            2 => Some((48, 32)),
            3 => Some((64, 32)),
            _ => None
        },
        PartType::TeeterTotter => match index {
            0 => Some((80, 36)),
            1 => Some((80, 23)),
            2 => Some((80, 36)),
            _ => None
        },
        PartType::Balloon => match index {
            0 => Some((32, 48)),
            1 => Some((72, 71)),
            2 => Some((80, 67)),
            3 => Some((96, 61)),
            4 => Some((88, 54)),
            5 => Some((88, 46)),
            6 => Some((88, 50)),
            _ => None
        },
        PartType::Conveyor => Some((32 + (index as i16 / 7)*16, 16)),
        PartType::MortTheMouseCage => Some((48, 32)),
        PartType::Pulley => Some((16, 16)),
        PartType::Basketball => Some((32, 32)),
        PartType::Cage => Some((48, 64)),
        PartType::PokeyTheCat => match index {
            0 => Some((40, 41)),
            1 => Some((72, 57)),
            2 => Some((56, 42)),
            3 => Some((48, 41)),
            4 => Some((56, 41)),
            5 => Some((56, 41)),
            6 => Some((56, 43)),
            7 => Some((56, 43)),
            8 => Some((56, 43)),
            9 => Some((56, 44)),
            _ => None
        },
        PartType::Gear => Some((40, 35)),
        PartType::Bucket => Some((40, 48)),
        PartType::EyeHook => Some((16, 16)),
        PartType::Baseball => Some((16, 15)),
        PartType::RopeSeveredEnd => None,
        PartType::Nail => Some((16, 17)),
        _ => {
            println!("Unimplemented part_image_size: {:?}", PartType::from_u16(part_type as u16));
            None
        }
    };

    if let Some((width, height)) = t {
        let out = unsafe { out.as_mut().unwrap() };
        out.x = width;
        out.y = height;
        return 1;
    }

    return 0;
}

/// Partial from TIMWIN: 1090:0000
/// Was pre-calculated in TIM each time the air pressure or gravity changed. Here we recalculate it each time.
/// We can possibly memoize this call in the future if performance calls for it.
#[no_mangle]
pub extern "C" fn part_acceleration(part_type: c_int) -> i16 {
    match PartType::from_u16(part_type as u16) {
        PartType::GunBullet => 0,
        PartType::Eightball => 0,
        _ => atmosphere::calculate_acceleration(unsafe { GRAVITY }, unsafe { AIR_PRESSURE }, unsafe { part_density(part_type) })
    }
}

/// Partial from TIMWIN: 1090:0000
/// Was pre-calculated in TIM each time the air pressure or gravity changed. Here we recalculate it each time.
/// We can possibly memoize this call in the future if performance calls for it.
#[no_mangle]
pub extern "C" fn part_terminal_velocity(part_type: c_int) -> i16 {
    match PartType::from_u16(part_type as u16) {
        PartType::GunBullet => 0x3000,
        PartType::CannonBall => 0x3000,
        _ => atmosphere::calculate_terminal_velocity(unsafe { AIR_PRESSURE })
    }
}

/// TIMWIN: 1090:012d
/// Accurate
///
/// Safety: `part` is dereferenced unconditionally, exactly matching the C (no null check
/// there either); every caller passes a live, currently-simulated `Part`.
///
/// `tv` and the `vel_hi_precision` components are all `i16`, but every comparison and the
/// negation `-tv` are done in `i32` here (mirroring C's promotion of `s16` operands to `int`
/// for `<`/unary `-`), and every assignment back to a `vel_hi_precision` field truncates with
/// `as i16`, matching C's implicit narrowing conversion back to the `s16` lvalue. This
/// matters on the boundary case `tv == i16::MIN`: C's `-tv` promotes `tv` to `int` first, so
/// `-tv` is `32768` (no overflow at `int` width), and assigning that back to an `s16` wraps
/// to `i16::MIN` again — `(-tv) as i16` with `tv: i32` reproduces that same wraparound rather
/// than panicking the way plain `i16` negation would in a debug build.
#[no_mangle]
pub extern "C" fn part_clamp_to_terminal_velocity(part: *mut Part) {
    unsafe {
        let tv = part_terminal_velocity((*part).part_type as c_int) as i32;

        if tv < (*part).vel_hi_precision.x as i32 {
            (*part).vel_hi_precision.x = tv as i16;
        } else if ((*part).vel_hi_precision.x as i32) < -tv {
            (*part).vel_hi_precision.x = (-tv) as i16;
        }

        if tv < (*part).vel_hi_precision.y as i32 {
            (*part).vel_hi_precision.y = tv as i16;
        } else if ((*part).vel_hi_precision.y as i32) < -tv {
            (*part).vel_hi_precision.y = (-tv) as i16;
        }
    }
}

#[no_mangle]
pub extern "C" fn part_density(part_type: c_int) -> u16 {
    let t = PartType::from_u16(part_type as u16);
    parts::get_def(t).density
}

#[no_mangle]
pub extern "C" fn part_mass(part_type: c_int) -> u16 {
    let t = PartType::from_u16(part_type as u16);
    parts::get_def(t).mass
}

#[no_mangle]
pub extern "C" fn part_bounciness(part_type: c_int) -> i16 {
    let t = PartType::from_u16(part_type as u16);
    parts::get_def(t).bounciness
}

#[no_mangle]
pub extern "C" fn part_friction(part_type: c_int) -> i16 {
    let t = PartType::from_u16(part_type as u16);
    parts::get_def(t).friction
}

#[no_mangle]
pub extern "C" fn part_order(part_type: c_int) -> u16 {
    let t = PartType::from_u16(part_type as u16);
    let list = parts::parts_bin_order();

    list.iter().position(|&x| x == t).unwrap() as u16
}

#[no_mangle]
pub extern "C" fn part_data30_flags1(part_type: c_int) -> u16 {
    let t = PartType::from_u16(part_type as u16);
    parts::get_def(t).flags1
}

#[no_mangle]
pub extern "C" fn part_data30_flags3(part_type: c_int) -> u16 {
    let t = PartType::from_u16(part_type as u16);
    parts::get_def(t).flags3
}

#[no_mangle]
pub extern "C" fn part_data30_size_something2(part_type: c_int) -> ShortVec {
    let t = PartType::from_u16(part_type as u16);
    let (w, h) = parts::get_def(t).size_something2;
    ShortVec { x: w as i16, y: h as i16 }
}

#[no_mangle]
pub extern "C" fn part_data30_size(part_type: c_int) -> ShortVec {
    let t = PartType::from_u16(part_type as u16);
    let (w, h) = parts::get_def(t).size;
    ShortVec { x: w as i16, y: h as i16 }
}

#[no_mangle]
pub extern "C" fn part_data31_render_pos_offset(part_type: c_int, state1: u16, out: &mut SByteVec) -> c_int {
    let t = PartType::from_u16(part_type as u16);
    if let Some(offsets) = parts::get_def(t).render_pos_offsets {
        let (x, y) = offsets[state1 as usize];
        out.x = x;
        out.y = y;
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn part_explicit_size(part_type: c_int, index: u16, out: &mut ShortVec) -> c_int {
    let t = PartType::from_u16(part_type as u16);
    if let Some(sizes) = parts::get_def(t).explicit_sizes {
        let (w, h) = sizes[index as usize];
        out.x = w;
        out.y = h;
        1
    } else {
        0
    }
}

/// TIMWIN: 10a8:25d9
///
/// Safety: `part` is dereferenced unconditionally, matching the C (no null check there
/// either); every caller passes a live, already-`part_alloc`'d `Part`.
///
/// `part->state1` (`i16`) is passed as the `u16 index` parameter to `part_explicit_size`/
/// `part_image_size` via `as u16`, matching C's implicit conversion of a signed value to an
/// unsigned parameter of the same width at the call site: it reinterprets the same bit
/// pattern, with no promotion/truncation concerns since both are 16 bits wide.
#[no_mangle]
pub extern "C" fn part_set_size(part: *mut Part) {
    unsafe {
        if (*part).part_type == PartType::Belt as u16 || (*part).part_type == PartType::Rope as u16 {
            (*part).size.x = 0;
            (*part).size.y = 0;
            return;
        }

        if (*part).flags1 & 0x0040 != 0 {
            (*part).size = (*part).size_something2;
            return;
        }

        let mut size = ShortVec { x: 0, y: 0 };

        if part_explicit_size((*part).part_type as c_int, (*part).state1 as u16, &mut size) != 0 {
            (*part).size = size;
            return;
        }

        if part_image_size((*part).part_type as c_int, (*part).state1 as u16, &mut size as *mut ShortVec) != 0 {
            (*part).size = size;
            return;
        }

        (*part).size.x = 0;
        (*part).size.y = 0;
    }
}

#[no_mangle]
pub extern "C" fn part_run(part: &mut Part) {
    let t = PartType::from_u16(part.part_type as u16);
    if let Some(run) = parts::get_def(t).run_fn {
        run(part);
    }
}

#[no_mangle]
pub extern "C" fn part_reset(part: &mut Part) {
    let t = PartType::from_u16(part.part_type as u16);
    if let Some(reset) = parts::get_def(t).reset_fn {
        reset(part);
    }
}

#[no_mangle]
pub extern "C" fn part_bounce(part_type: c_int, part: &mut Part) -> c_int {
    let t = PartType::from_u16(part_type as u16);
    if let Some(bounce) = parts::get_def(t).bounce_fn {
        if bounce(part) {
            1
        } else {
            0
        }
    } else {
        // Default
        1
    }
}

#[no_mangle]
pub extern "C" fn part_flip(part: &mut Part, orientation: c_int) {
    let t = PartType::from_u16(part.part_type as u16);
    if let Some(flip) = parts::get_def(t).flip_fn {
        flip(part, orientation as u16);
    }
}

#[no_mangle]
pub extern "C" fn part_resize(part: &mut Part) {
    let t = PartType::from_u16(part.part_type as u16);
    if let Some(resize) = parts::get_def(t).resize_fn {
        resize(part);
    }
}

#[no_mangle]
pub extern "C" fn part_rope(part_type: c_int, p1: &mut Part, p2: &mut Part, rope_slot: c_int, flags: u16, p1_mass: i16, p1_force: i32) -> c_int {
    let t = PartType::from_u16(part_type as u16);
    if let Some(rope) = parts::get_def(t).rope_fn {
        rope(p1, p2, rope_slot as u8, flags, p1_mass, p1_force) as c_int
    } else {
        // Default
        0
    }
}

#[no_mangle]
pub extern "C" fn part_create_func(part_type: c_int, part: &mut Part) -> c_int {
    let t = PartType::from_u16(part_type as u16);
    let create = parts::get_def(t).create_fn;

    create(part);
    if let Some(reset) = parts::get_def(t).reset_fn {
        reset(part);
    }

    0
}

/// TIMWIN: 1040:197d
///
/// Safety: no pointer is ever touched here. `part_type` is a bare `enum PartType` value
/// passed by value (a bare C enum here is a 4-byte value, not a pointer), matching the C
/// signature exactly, so there is no null path to preserve.
#[no_mangle]
pub extern "C" fn is_low_res_and_specific_part(part_type: c_int) -> bool {
    if unsafe { crate::globals::VALUES_PER_PIXEL } > 256 {
        return false;
    }

    // I'm not really sure what's so special about these parts.
    match PartType::from_u16(part_type as u16) {
        PartType::BrickWall
        | PartType::Incline
        | PartType::MortTheMouseCage
        | PartType::Conveyor
        | PartType::Pulley
        | PartType::LightSwitchOutlet
        | PartType::EyeHook
        | PartType::Fan
        | PartType::MagnifyingGlass
        | PartType::SolarPanels
        | PartType::PipeStraight
        | PartType::PipeCurved
        | PartType::WoodWall
        | PartType::ElectricEngine
        | PartType::Nail
        | PartType::DirtWall
        | PartType::PinballBumper => true,

        _ => false,
    }
}

/// Private port of the `static inline` helper `approx_hypot` in `c_src/tim.h`. The C copy is
/// left in place because other, still-C translation units (`draw_rope.c`) keep calling it.
///
/// Safety: no pointers involved; `x` and `y` are compared and shifted as C promotes `s16` to
/// `int` for `<` and `>>`, with the final result truncated back to `i16` on return exactly as
/// C narrows an `int` return expression to the declared `s16` return type.
fn approx_hypot(x: i16, y: i16) -> i16 {
    if (x as i32) < (y as i32) {
        (((x as i32) >> 2) + ((x as i32) >> 3) + (y as i32)) as i16
    } else {
        (((y as i32) >> 2) + ((y as i32) >> 3) + (x as i32)) as i16
    }
}

/// Private port of the `static inline` helper `part_get_rope_link_index` in `c_src/tim.h`.
/// The C copy is left in place because other, still-C translation units (`part_defs.c`) keep
/// calling it.
///
/// Safety: `from` is dereferenced unconditionally to read `links_to`, exactly matching the C
/// (`from->links_to[0]`, no null check). Every caller passes a live `Part`.
unsafe fn part_get_rope_link_index(target: *mut Part, from: *mut Part) -> i32 {
    if (*from).links_to[0] == target {
        return 0;
    }
    if (*from).links_to[1] == target {
        return 1;
    }
    -1
}

/// TIMWIN: 10a8:42e6
/// Accurate
/// Returns absolute length between the two rope points.
/// out_x and out_y are the signed x/y deltas between the two rope points.
///
/// Safety: `rope` and `part` are dereferenced unconditionally, exactly matching the C (no null
/// checks there either); every caller passes a live rope and one of its two live endpoint
/// parts. `links_to` (read from `part`/`rope->part2`'s `links_to[rope_slot]`) is likewise
/// dereferenced unconditionally, matching the C's unchecked `links_to->type` /
/// `links_to->rope_data[0]->ends_pos[...]`; this relies on the invariant (unchanged from the
/// C) that a part's `links_to` slot used by an active rope always points at a live part, and
/// that when that part is a pulley its primary `rope_data[0]` is always populated. `out_x` and
/// `out_y` are also written through unconditionally, matching the C's unchecked `*out_x = ...`.
///
/// Every intermediate `rope_x_*`/`rope_y_*` addition and subtraction is done in `i32` (mirroring
/// C's promotion of the `s16`/`byte` operands to `int`) and then truncated back to `i16` on
/// assignment, matching C's implicit narrowing conversion back to the `s16` lvalues/parameters
/// -- including the final `abs(...)` results, which C implicitly truncates from `int` down to
/// the `s16` parameters of `approx_hypot`.
#[no_mangle]
pub extern "C" fn distance_to_rope_link(
    rope: *mut RopeData,
    part: *mut Part,
    out_x: *mut i16,
    out_y: *mut i16,
) -> i16 {
    unsafe {
        let rope_x_1: i16;
        let rope_y_1: i16;
        let rope_x_2: i16;
        let rope_y_2: i16;

        if (*rope).part1 == part {
            let rope_slot = (*rope).part1_rope_slot as usize;
            rope_x_1 = ((*part).pos_render.x as i32 + (*part).rope_loc[rope_slot].x as i32) as i16;
            rope_y_1 = ((*part).pos_render.y as i32 + (*part).rope_loc[rope_slot].y as i32) as i16;

            let links_to = (*part).links_to[rope_slot];
            let index = part_get_rope_link_index(part, links_to);
            if (*links_to).part_type == PartType::Pulley as u16 {
                let end = (*(*links_to).rope_data[0]).ends_pos[(1 - index) as usize];
                rope_x_2 = end.x;
                rope_y_2 = end.y;
            } else {
                rope_x_2 = ((*links_to).pos_render.x as i32 + (*links_to).rope_loc[index as usize].x as i32) as i16;
                rope_y_2 = ((*links_to).pos_render.y as i32 + (*links_to).rope_loc[index as usize].y as i32) as i16;
            }
        } else {
            let rope_slot = (*rope).part2_rope_slot as usize;
            let p2 = (*rope).part2;
            rope_x_1 = ((*p2).pos_render.x as i32 + (*p2).rope_loc[rope_slot].x as i32) as i16;
            rope_y_1 = ((*p2).pos_render.y as i32 + (*p2).rope_loc[rope_slot].y as i32) as i16;

            let links_to = (*p2).links_to[rope_slot];
            let index = part_get_rope_link_index(p2, links_to);
            if (*links_to).part_type == PartType::Pulley as u16 {
                let end = (*(*links_to).rope_data[0]).ends_pos[(1 - index) as usize];
                rope_x_2 = end.x;
                rope_y_2 = end.y;
            } else {
                rope_x_2 = ((*links_to).pos_render.x as i32 + (*links_to).rope_loc[index as usize].x as i32) as i16;
                rope_y_2 = ((*links_to).pos_render.y as i32 + (*links_to).rope_loc[index as usize].y as i32) as i16;
            }
        }

        *out_x = (rope_x_1 as i32 - rope_x_2 as i32) as i16;
        *out_y = (rope_y_1 as i32 - rope_y_2 as i32) as i16;

        approx_hypot(
            (rope_x_1 as i32 - rope_x_2 as i32).abs() as i16,
            (rope_y_1 as i32 - rope_y_2 as i32).abs() as i16,
        )
    }
}

/// TIMWIN: 1090:1480
/// Accurate
///
/// Safety: `bucket` is dereferenced unconditionally, exactly matching the C (no null check
/// there either); every caller passes a live `Part`. The `EACH_MOVING_PART` walk is
/// reproduced with `moving_parts_iter_mut`, which follows the same `MOVING_PARTS_ROOT.next` /
/// `->next` chain the C macro does; each yielded `curpart` is a live part linked into that
/// list, so dereferencing it (including writing through it) is sound, matching the C's
/// unchecked `curpart->...` field accesses.
///
/// `curpart_x_center` and `curpart_y_bottom` are computed in `i32` (mirroring C's promotion of
/// the `s16` fields to `int` for `+`/`>>`/unary `-=`) and truncated back to `i16` immediately,
/// matching the C's `s16` locals; the subsequent `BETWEEN_EXCL` comparisons are then done by
/// widening those truncated `i16` values back to `i32` alongside the other `i32` bucket-derived
/// bounds, exactly as C's `<` promotes both `s16` operands to `int`.
#[no_mangle]
pub extern "C" fn bucket_handle_contained_parts(bucket: *mut Part) {
    unsafe {
        if (*bucket).part_type != PartType::Bucket as u16 {
            return;
        }

        (*bucket).interactions = std::ptr::null_mut();

        for curpart in moving_parts_iter_mut() {
            if bucket == curpart {
                continue;
            }
            if (*curpart).flags2 & 0x2000 != 0 {
                continue;
            }
            if (*curpart).part_type == PartType::Cage as u16 {
                continue;
            }

            let curpart_x_center =
                ((*curpart).pos_prev1.x as i32 + (((*curpart).size.x as i32) >> 1)) as i16;
            let mut curpart_y_bottom =
                ((*curpart).pos_prev1.y as i32 + (*curpart).size.y as i32) as i16;
            if (*curpart).part_type == PartType::Rocket as u16 {
                curpart_y_bottom = (curpart_y_bottom as i32 - 12) as i16;
            }

            let bucket_x_lo = (*bucket).pos_prev1.x as i32 + 4;
            let bucket_x_hi = (*bucket).pos_prev1.x as i32 + 32;
            let in_x = bucket_x_lo < curpart_x_center as i32 && (curpart_x_center as i32) < bucket_x_hi;

            let bucket_y_lo = (*bucket).pos_prev1.y as i32 + 20;
            let bucket_y_hi = (*bucket).pos_prev1.y as i32 + (*bucket).size.y as i32 + 4;
            let in_y = bucket_y_lo < curpart_y_bottom as i32 && (curpart_y_bottom as i32) < bucket_y_hi;

            let in_bucket = in_x
                && (((*curpart).bounce_part == bucket && (*curpart).vel_hi_precision.y > 0) || in_y);

            if in_bucket {
                (*curpart).interactions = (*bucket).interactions;
                (*bucket).interactions = curpart;
                (*curpart).flags3 |= 0x0010;

                (*curpart).vel_hi_precision = (*bucket).vel_hi_precision;

                (*curpart).extra1 = ((*bucket).pos.y as i32 + 20) as i16;
            }
        }
    }
}
