// For now...
#![allow(dead_code)]

pub mod image;
pub mod buffer_snake;
pub mod debug;
pub mod decoders;
pub mod resource_dos;
pub mod atmosphere;
pub mod part;
pub mod parts;
pub mod math;
pub mod render;
pub mod tim_c;
pub mod globals;
pub mod level_file_format;
pub mod level_load;

#[cfg(target_arch = "wasm32")]
pub mod wasm_libc;

#[cfg(target_arch = "wasm32")]
pub mod web;

use resource_dos::Resources;

/// Decode every sprite in the archive, calling `on_image` with
/// (filename, slice index, width, height, RGBA8 buffer).
///
/// Pixels the SCN decoder never plots are left at alpha 0, which is what makes them
/// transparent when blitted.
pub fn load_images(
    resources: &mut Resources,
    on_image: &mut dyn FnMut(&str, usize, u32, u32, Vec<u8>),
) -> Result<(), Box<dyn std::error::Error>> {
    // Scratch buffer for reading files
    // (our way to avoid dynamic allocations)
    let mut tmp_buf = vec![0; 1000000];

    let mut tim_pal_buf = [[0, 0, 0, 0]; 256];
    let tim_pal = resources
        .read("TIM.PAL", &mut tmp_buf)
        .and_then(|x| resource_dos::parse_vga_palette_as_rgba(x, &mut tim_pal_buf))
        .ok_or("TIM.PAL missing or unreadable")?
        .clone();

    let mut filenames: Vec<String> = resources.iter_filenames().map(|s| s.into()).collect();
    filenames.sort();

    for filename in filenames.iter() {
        if let Some(bmp) = resources.read(&filename, &mut tmp_buf).and_then(resource_dos::parse_bmp_scn) {
            for (i, b) in bmp.into_iter().enumerate() {
                let (_, (max_x, max_y)) = image::bmp_scn::decode_bounds(b.scn).unwrap();

                let width = max_x as u32 + 1;
                let height = max_y as u32 + 1;
                let stride = width as usize * 4;

                let mut buf = vec![0; (width * height * 4) as usize];
                image::bmp_scn::decode_rgba8(b.scn, &mut buf, stride, &tim_pal).unwrap();

                on_image(filename, i, width, height, buf);
            }
        }
    }

    Ok(())
}

/// Read a level's bytes out of the archive, decompressing if needed.
///
/// Levels are stored raw when they start with the magic 0xACED (TIM), 0xACEE (Toons) or
/// 0xACEF (TIM 2); anything else is a compressed payload.
pub fn read_level_bytes(resources: &mut Resources, name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut tmp_buf = vec![0; 1000000];
    let raw = resources
        .read(name, &mut tmp_buf)
        .ok_or_else(|| format!("no resource entry named {}", name))?
        .to_vec();

    let is_raw_level = raw.len() >= 2 && raw[1] == 0xac && (0xed..=0xef).contains(&raw[0]);
    if is_raw_level {
        Ok(raw)
    } else {
        let mut out = vec![0; 1000000];
        Ok(decoders::generic_decode(&raw, &mut out)?.to_vec())
    }
}

/// Free every part in the world and empty the three part lists.
///
/// The C core keeps the world in global linked lists which nothing else ever empties, so
/// without this a second `load_level` adds to the first instead of replacing it.
///
/// Freeing is left to `part_free`, which knows the ownership rules: border points always
/// belong to the part, belt data belongs to it unless `F2_0001` marks the copy as shared,
/// and `rope_data[0]` is only owned by ropes and pulleys because everything else merely
/// points at a rope owned elsewhere.
pub fn clear_level() {
    unsafe {
        for root in [
            std::ptr::addr_of_mut!(tim_c::STATIC_PARTS_ROOT),
            std::ptr::addr_of_mut!(tim_c::MOVING_PARTS_ROOT),
            std::ptr::addr_of_mut!(tim_c::PARTS_BIN_ROOT),
        ] {
            let mut cur = (*root).next;
            while !cur.is_null() {
                let next = (*cur).next;
                tim_c::part_free(cur);
                cur = next;
            }
            (*root).next = std::ptr::null_mut();
            (*root).prev = std::ptr::null_mut();
        }
    }
}

/// Parse a level and install it into the simulation's global part lists.
///
/// The C core keeps the world in global linked lists, so only one level can be loaded at
/// a time and this replaces whatever was there.
pub fn load_level(buf: &[u8], freeform: bool) -> Result<level_file_format::Level, Box<dyn std::error::Error>> {
    let level_opts = level_file_format::GameOptions::Tim { freeform_mode: freeform };
    let level = level_file_format::read(buf, &level_opts)?;

    // Discard the previous world first; the part lists are global and persist otherwise.
    clear_level();

    unsafe {
        tim_c::initialize_llamas();
    }
    level_load::level_load(&level);
    unsafe {
        tim_c::restore_parts_state_from_design();
    }

    Ok(level)
}

/// Whether every part in a level is implemented, i.e. whether the level will load rather
/// than panic. Parses the level but does not install it.
pub fn level_is_supported(buf: &[u8]) -> bool {
    let opts = level_file_format::GameOptions::Tim { freeform_mode: false };
    let level = match level_file_format::read(buf, &opts) {
        Ok(l) => l,
        Err(_) => return false,
    };

    level
        .static_parts
        .iter()
        .chain(level.moving_parts.iter())
        .chain(level.bin_parts.iter().flatten())
        .all(|p| {
            part::PartType::try_from_u16(p.part_type).map_or(false, parts::is_implemented)
        })
}

/// Advance the simulation by one tick.
pub fn tick() {
    unsafe {
        tim_c::advance_parts();
        tim_c::all_parts_set_prev_vars();
    }
}

/// One line per part, dumping every simulation-relevant field of `Part` plus the contents
/// of any attached `RopeData`/`BeltData`.
///
/// Shared by the CLI and the web build so the two can be compared directly, which is how
/// the wasm engine is checked against the native one, and is what the 28 golden baselines
/// under `tests/baselines/` are captured from.
///
/// Deliberately excludes every raw pointer VALUE (`next`, `prev`, `links_to`,
/// `links_to_design`, `plug_parts`, `goober_parts`, `rope_data`, `belt_data`,
/// `interactions`, `bounce_part`, `borders_data`): these are heap addresses that differ
/// between runs and between native and wasm, so printing them would make every baseline
/// diff look like a real regression. Where a pointer's identity matters for the
/// simulation, a stable derived fact is recorded instead: `resolve()` below turns a part
/// pointer into `<list>:<index>` (its position within the static/moving/bin list it
/// currently lives in, exactly the position it -- or, for bin parts, would -- appear at in
/// this same dump), `null` if the pointer is null, or `ext` if it points at a part this
/// dump cannot find (should not happen given the invariants elsewhere in this codebase,
/// but is a safe fallback rather than a panic). `rope_data`/`belt_data` themselves are
/// dereferenced to print their scalar contents (endpoint positions, rope slots, widths);
/// only the *pointers inside* those structs are resolved rather than printed raw.
///
/// `field_0x14`, `field_0x15`, `field_0x7A` and `field_0x7C` on `Part` are deliberately
/// left out: nothing in the port (Rust or the still-C code) reads or writes them, `Part`s
/// are always `alloc_zeroed`, and nothing ever mutates them, so they are guaranteed to
/// print as `0` forever and would only add noise. If a future port step gives one of them
/// a name, add it here.
pub fn parts_summary() -> String {
    use std::fmt::Write;
    use std::collections::HashMap;

    let fmt_sv = |v: tim_c::ShortVec| format!("({},{})", v.x, v.y);
    let fmt_bv = |v: tim_c::ByteVec| format!("({},{})", v.x, v.y);

    // Every part currently in any of the three lists, keyed by address, so pointer fields
    // that link to another part can be recorded as "which part" (by stable list position)
    // rather than as an address. Built once up front and reused for every part printed
    // below -- the world does not change while this function runs.
    let bin_root_next = unsafe { (*std::ptr::addr_of!(tim_c::PARTS_BIN_ROOT)).next };
    let mut index: HashMap<usize, String> = HashMap::new();
    for (label, iter) in [
        ("static", unsafe { tim_c::static_parts_iter() }),
        ("moving", unsafe { tim_c::moving_parts_iter() }),
        ("bin", unsafe { tim_c::PartsIterator::new(bin_root_next) }),
    ] {
        for (i, part) in iter.enumerate() {
            index.insert(part as *const tim_c::Part as usize, format!("{}:{}", label, i));
        }
    }
    let resolve = |p: *const tim_c::Part| -> String {
        if p.is_null() {
            "null".to_string()
        } else {
            index.get(&(p as usize)).cloned().unwrap_or_else(|| "ext".to_string())
        }
    };

    let fmt_rope = |r: &tim_c::RopeData| -> String {
        format!(
            "{{owner={} part1={} part2={} orig_part1={} orig_part2={} slots=({},{}) orig_slots=({},{}) unk={}/{}/{} ends=[{},{}] ends_prev1=[{},{}] ends_prev2=[{},{}]}}",
            resolve(r.rope_or_pulley_part), resolve(r.part1), resolve(r.part2),
            resolve(r.original_part1), resolve(r.original_part2),
            r.part1_rope_slot, r.part2_rope_slot,
            r.original_part1_rope_slot, r.original_part2_rope_slot,
            r.rope_unknown, r.rope_unknown_prev1, r.rope_unknown_prev2,
            fmt_sv(r.ends_pos[0]), fmt_sv(r.ends_pos[1]),
            fmt_sv(r.ends_pos_prev1[0]), fmt_sv(r.ends_pos_prev1[1]),
            fmt_sv(r.ends_pos_prev2[0]), fmt_sv(r.ends_pos_prev2[1]),
        )
    };
    let fmt_belt = |b: &tim_c::BeltData| -> String {
        format!(
            "{{unk0={} owner={} part1={} part2={} pos=[{},{},{},{}] prev1=[{},{},{},{}] prev2=[{},{},{},{}]}}",
            b.field_0x00, resolve(b.belt_part), resolve(b.part1), resolve(b.part2),
            fmt_sv(b.pos1), fmt_sv(b.pos2), fmt_sv(b.pos3), fmt_sv(b.pos4),
            fmt_sv(b.pos1_prev1), fmt_sv(b.pos2_prev1), fmt_sv(b.pos3_prev1), fmt_sv(b.pos4_prev1),
            fmt_sv(b.pos1_prev2), fmt_sv(b.pos2_prev2), fmt_sv(b.pos3_prev2), fmt_sv(b.pos4_prev2),
        )
    };

    let mut out = String::new();
    for (label, iter) in [
        ("static", unsafe { tim_c::static_parts_iter() }),
        ("moving", unsafe { tim_c::moving_parts_iter() }),
    ] {
        for part in iter {
            let _ = write!(
                out,
                "  {} {:?} pos={} pos_prev1={} pos_prev2={} pos_render={} pos_render_prev1={} pos_render_prev2={} pos_hi=({},{}) vel_hi={}",
                label,
                part::PartType::from_u16(part.part_type),
                fmt_sv(part.pos), fmt_sv(part.pos_prev1), fmt_sv(part.pos_prev2),
                fmt_sv(part.pos_render), fmt_sv(part.pos_render_prev1), fmt_sv(part.pos_render_prev2),
                part.pos_x_hi_precision, part.pos_y_hi_precision,
                fmt_sv(part.vel_hi_precision),
            );
            let _ = write!(
                out,
                " size={} size_prev1={} size_prev2={} size_something={} size_something2={} mass={} force={}",
                fmt_sv(part.size), fmt_sv(part.size_prev1), fmt_sv(part.size_prev2),
                fmt_sv(part.size_something), fmt_sv(part.size_something2),
                part.mass, part.force,
            );
            let _ = write!(
                out,
                " state1={} state1_prev1={} state1_prev2={} state2={} extra1={} extra1_prev1={} extra1_prev2={} extra2={} extra2_prev1={} extra2_prev2={}",
                part.state1, part.state1_prev1, part.state1_prev2, part.state2,
                part.extra1, part.extra1_prev1, part.extra1_prev2,
                part.extra2, part.extra2_prev1, part.extra2_prev2,
            );
            let _ = write!(
                out,
                " flags1={:04x} flags2={:04x} flags3={:04x}",
                part.flags1, part.flags2, part.flags3,
            );
            let _ = write!(
                out,
                " orig_pos=({},{}) orig_state1={} orig_state2={} orig_flags2={:04x}",
                part.original_pos_x, part.original_pos_y,
                part.original_state1, part.original_state2, part.original_flags2,
            );
            let _ = write!(
                out,
                " belt_loc={} belt_width={} rope_loc=[{},{}] fuse_loc={} plug_choose={} goober={}",
                fmt_bv(part.belt_loc), part.belt_width,
                fmt_bv(part.rope_loc[0]), fmt_bv(part.rope_loc[1]),
                fmt_bv(part.fuse_loc), part.plug_choose, part.goober,
            );
            let _ = write!(
                out,
                " num_borders={} bounce_side_flags=({},{}) bounce_angle={} bounce_border_index={}",
                part.num_borders,
                part.bounce_field_0x86[0], part.bounce_field_0x86[1],
                part.bounce_angle, part.bounce_border_index,
            );
            let _ = write!(
                out,
                " links_to=[{},{}] links_to_design=[{},{}] plug_parts=[{},{}] goober_parts=[{},{}] bounce_part={} interactions={}",
                resolve(part.links_to[0]), resolve(part.links_to[1]),
                resolve(part.links_to_design[0]), resolve(part.links_to_design[1]),
                resolve(part.plug_parts[0]), resolve(part.plug_parts[1]),
                resolve(part.goober_parts[0]), resolve(part.goober_parts[1]),
                resolve(part.bounce_part), resolve(part.interactions),
            );
            let _ = write!(
                out,
                " belt={}",
                if part.belt_data.is_null() { "none".to_string() } else { fmt_belt(unsafe { &*part.belt_data }) },
            );
            for (i, rd) in part.rope_data.iter().enumerate() {
                let _ = write!(
                    out,
                    " rope{}={}",
                    i,
                    if rd.is_null() { "none".to_string() } else { fmt_rope(unsafe { &**rd }) },
                );
            }
            let _ = writeln!(out);
        }
    }
    out
}
