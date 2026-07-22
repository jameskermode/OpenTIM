// Rust/C interop layer

use std::os::raw::{c_int, c_char};
use crate::part::PartType;
use crate::atmosphere;
use crate::parts;

/**** Import C declarations to Rust ****/
extern {
    pub fn initialize_llamas();
    pub fn part_new(part_type: c_int) -> *mut Part;
    pub fn part_init_rope_data_primary(part: *mut Part);
    pub fn part_init_belt_data(part: *mut Part);
    pub fn part_alloc_borders(part: *mut Part, length: u16);
    pub fn part_calculate_border_normals(part: *mut Part);
    pub fn part_set_size_and_pos_render(part: *mut Part);
    pub fn part_clamp_to_terminal_velocity(part: *mut Part);
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
extern "C" {
    fn free(ptr: *mut std::os::raw::c_void);
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

/* TIMWIN: 10a8:1329 */
#[no_mangle]
pub extern "C" fn stub_10a8_1329(_belt: *mut BeltData) -> c_int {
    unimplemented!("stub_10a8_1329")
}

/* TIMWIN: 10a8:28a5 */
#[no_mangle]
pub extern "C" fn stub_10a8_28a5(_part: *mut Part, _unused: c_int) {
    unimplemented!("stub_10a8_28a5")
}

/* TIMWIN: 10a8:0880 */
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
