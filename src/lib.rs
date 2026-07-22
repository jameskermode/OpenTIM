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

/// One line per part: type, position, size, state and flags.
///
/// Shared by the CLI and the web build so the two can be compared directly, which is how
/// the wasm engine is checked against the native one.
pub fn parts_summary() -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (label, iter) in &mut [
        ("static", unsafe { tim_c::static_parts_iter() }),
        ("moving", unsafe { tim_c::moving_parts_iter() }),
    ] {
        for part in iter {
            let _ = writeln!(
                out,
                "  {} {:?} pos=({},{}) size=({},{}) state1={} flags1={:04x} flags2={:04x}",
                label,
                part::PartType::from_u16(part.part_type),
                part.pos.x, part.pos.y,
                part.size.x, part.size.y,
                part.state1,
                part.flags1, part.flags2,
            );
        }
    }
    out
}
