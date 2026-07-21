// For now...
#![allow(dead_code)]

mod image;
mod buffer_snake;
mod debug;
mod decoders;
mod resource_dos;
mod atmosphere;
mod part;
mod parts;
mod math;
mod render;
#[cfg(feature = "gui")]
mod nannou;
pub mod tim_c;
mod level_file_format;
mod level_load;

use level_load::level_load;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    {
        let args: Vec<String> = std::env::args().collect();

        // `opentim <game-dir> --list-resources` dumps the archive index and exits.
        if args.get(2).map(|s| s.as_str()) == Some("--list-resources") {
            let mut resources = resource_dos::from_map(&args[1], "RESOURCE.MAP")?;
            let mut names: Vec<String> = resources.iter_filenames().map(|s| s.into()).collect();
            names.sort();
            println!("{} entries in RESOURCE.MAP", names.len());
            for n in names {
                println!("  {}", n);
            }
            return Ok(());
        }

        // `opentim <game-dir> --extract <NAME> <out-file>` writes a raw archive payload out.
        if args.get(2).map(|s| s.as_str()) == Some("--extract") {
            let mut resources = resource_dos::from_map(&args[1], "RESOURCE.MAP")?;
            let mut tmp_buf = vec![0; 4000000];
            let raw = resources
                .read(&args[3], &mut tmp_buf)
                .ok_or_else(|| format!("no resource entry named {}", &args[3]))?;
            std::fs::write(&args[4], raw)?;
            println!("wrote {} bytes", raw.len());
            return Ok(());
        }

        // `opentim <game-dir> --dump-images <out-dir> [name-filter]` decodes sprites to PPM.
        if args.get(2).map(|s| s.as_str()) == Some("--dump-images") {
            let out_dir = &args[3];
            let filter = args.get(4).cloned();
            std::fs::create_dir_all(out_dir)?;
            let mut count = 0;
            load_images(&mut |filename, slice_idx, width, height, buf| {
                if let Some(f) = &filter {
                    if !filename.contains(f.as_str()) { return; }
                }
                let path = format!("{}/{}.{}.ppm", out_dir, filename, slice_idx);
                debug::write_raster_to_ppm(&path, &buf, width as usize, height as usize, width as usize * 4).unwrap();
                count += 1;
            })?;
            println!("wrote {} images to {}", count, out_dir);
            return Ok(());
        }

        let level_filename = &args[2];

        // A level is either a saved machine on disk (freeform, e.g. CATOMATC.TIM) or a
        // puzzle entry inside RESOURCE.MAP (e.g. L1.LEV).
        let (buf, from_archive): (Vec<u8>, bool) = match std::fs::read(level_filename) {
            Ok(b) => (b, false),
            Err(_) => {
                let mut resources = resource_dos::from_map(&args[1], "RESOURCE.MAP")?;
                let mut tmp_buf = vec![0; 1000000];
                let raw = resources
                    .read(level_filename, &mut tmp_buf)
                    .ok_or_else(|| format!("no file or resource entry named {}", level_filename))?
                    .to_vec();

                // Archive payloads may be compressed. Level magic is 0xACED (TIM), 0xACEE
                // (Toons) or 0xACEF (TIM2); pass any of them through so that unsupported
                // versions are reported as BadMagic rather than as a bogus decode failure.
                let is_raw_level =
                    raw.len() >= 2 && raw[1] == 0xac && (0xed..=0xef).contains(&raw[0]);
                if is_raw_level {
                    (raw, true)
                } else {
                    let mut out = vec![0; 1000000];
                    let decoded = decoders::generic_decode(&raw, &mut out)?.to_vec();
                    (decoded, true)
                }
            }
        };

        let level_opts = level_file_format::GameOptions::Tim { freeform_mode: !from_archive };

        let level = level_file_format::read(&buf, &level_opts)?;
        if let Some(title) = &level.puzzle_title {
            println!("{}", title);
        }
        if let Some(objective) = &level.puzzle_objective {
            println!("{}", objective);
        }

        unsafe {
            tim_c::initialize_llamas();
        }

        println!("Loading level...");
        level_load(&level);

        unsafe {
            tim_c::restore_parts_state_from_design();
        }
        println!("Done loading!");
    }

    let args: Vec<String> = std::env::args().collect();
    match args.get(3).map(|s| s.as_str()) {
        // Open a window and simulate interactively.
        Some("--play") => return render::run(),

        // Render one frame to a PPM after N ticks, for checking the renderer headlessly.
        Some("--screenshot") => {
            let out = args.get(4).cloned().unwrap_or_else(|| "screenshot.ppm".into());
            let ticks: u32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
            let borders = args.iter().any(|a| a == "--borders");
            return render::screenshot(&out, ticks, borders);
        }

        _ => {}
    }

    {
        // Headless: step the simulation and report where the moving parts ended up.
        let ticks: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(60);

        println!("--- parts as loaded ---");
        print_summary();

        unsafe {
            for _ in 0..ticks {
                tim_c::advance_parts();
                tim_c::all_parts_set_prev_vars();
            }
        }

        println!("--- parts after {} ticks ---", ticks);
        print_summary();
    }

    Ok(())
}

/// Compact one-line-per-part dump, for the headless runner.
fn print_summary() {
    use part::PartType;

    for (label, iter) in &mut [
        ("static", unsafe { tim_c::static_parts_iter() }),
        ("moving", unsafe { tim_c::moving_parts_iter() }),
    ] {
        for part in iter {
            println!(
                "  {} {:?} pos=({},{}) size=({},{}) state1={} flags1={:04x} flags2={:04x}",
                label,
                PartType::from_u16(part.part_type),
                part.pos.x, part.pos.y,
                part.size.x, part.size.y,
                part.state1,
                part.flags1, part.flags2,
            );
        }
    }
}

pub fn load_images(on_image: &mut dyn FnMut(&str, usize, u32, u32, Vec<u8>)) -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let root_directory = &args[1];

    let mut resources = resource_dos::from_map(root_directory, "RESOURCE.MAP")?;
    
    // Scratch buffer for reading files
    // (our way to avoid dynamic allocations)
    let mut tmp_buf  = vec![0; 1000000];

    let mut tim_pal_buf     = [[0,0,0,0];256];
    // let mut dynamix_pal_buf = [[0,0,0,0];256];
    // let mut sierra_pal_buf  = [[0,0,0,0];256];
    let tim_pal     = resources.read("TIM.PAL",    &mut tmp_buf).and_then(|x| resource_dos::parse_vga_palette_as_rgba(x, &mut tim_pal_buf)).unwrap();
    // let dynamix_pal = resources.read("DYNAMIX.PAL", &mut tmp_buf).and_then(|x| resource_dos::parse_vga_palette_as_rgba(x, &mut dynamix_pal_buf)).unwrap();
    // let sierra_pal  = resources.read("SIERRA.PAL", &mut tmp_buf).and_then(|x| resource_dos::parse_vga_palette_as_rgba(x, &mut sierra_pal_buf)).unwrap();

    #[allow(unused_mut)]
    let mut filenames: Vec<String> = resources.iter_filenames().map(|s| s.into()).collect();

    filenames.sort();

    for f in filenames.iter() {
        // println!("{}", f);
    }

    for filename in filenames.iter() {
        if let Some(bmp) = resources.read(&filename, &mut tmp_buf).and_then(resource_dos::parse_bmp_scn) {
            for (i, b) in bmp.into_iter().enumerate() {
                let (_, (max_x, max_y)) = image::bmp_scn::decode_bounds(b.scn).unwrap();

                let width = max_x as u32 + 1;
                let height = max_y as u32 + 1;

                let stride = width as usize * 4;

                let mut buf = vec![0; (width*height*4) as usize];
                image::bmp_scn::decode_rgba8(b.scn, &mut buf, stride, &tim_pal).unwrap();

                on_image(filename, i, width, height, buf);
            }
        }
    
    }

    Ok(())
}