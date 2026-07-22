// The CLI is desktop-only. The browser entry point is src/web.rs, driven from JavaScript.
#[cfg(not(target_arch = "wasm32"))]
use opentim::{debug, load_images, load_level, render, resource_dos, tick};

/// wasm builds produce the cdylib; this binary exists only to satisfy the bin target.
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn usage() -> ! {
    eprintln!(
        "usage:
  opentim <game-dir> --list-resources
  opentim <game-dir> --extract <NAME> <out-file>
  opentim <game-dir> --dump-images <out-dir> [name-filter]
  opentim <game-dir> <level> [ticks]
  opentim <game-dir> <level> --play
  opentim <game-dir> <level> --screenshot <out.ppm> [ticks] [--borders]

<level> is either a saved machine on disk (CATOMATC.TIM) or the name of a puzzle
inside RESOURCE.MAP (L6.LEV)."
    );
    std::process::exit(2)
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        usage();
    }
    let root_directory = &args[1];

    match args[2].as_str() {
        "--list-resources" => {
            let resources = resource_dos::from_map(root_directory, "RESOURCE.MAP")?;
            let mut names: Vec<String> = resources.iter_filenames().map(|s| s.into()).collect();
            names.sort();
            println!("{} entries in RESOURCE.MAP", names.len());
            for n in names {
                println!("  {}", n);
            }
            return Ok(());
        }

        "--extract" => {
            if args.len() < 5 {
                usage();
            }
            let mut resources = resource_dos::from_map(root_directory, "RESOURCE.MAP")?;
            let mut tmp_buf = vec![0; 4000000];
            let raw = resources
                .read(&args[3], &mut tmp_buf)
                .ok_or_else(|| format!("no resource entry named {}", &args[3]))?;
            std::fs::write(&args[4], raw)?;
            println!("wrote {} bytes", raw.len());
            return Ok(());
        }

        "--dump-images" => {
            if args.len() < 4 {
                usage();
            }
            let mut resources = resource_dos::from_map(root_directory, "RESOURCE.MAP")?;
            let out_dir = &args[3];
            let filter = args.get(4).cloned();
            std::fs::create_dir_all(out_dir)?;
            let mut count = 0;
            load_images(&mut resources, &mut |filename, slice_idx, width, height, buf| {
                if let Some(f) = &filter {
                    if !filename.contains(f.as_str()) {
                        return;
                    }
                }
                let path = format!("{}/{}.{}.ppm", out_dir, filename, slice_idx);
                debug::write_raster_to_ppm(&path, &buf, width as usize, height as usize, width as usize * 4)
                    .unwrap();
                count += 1;
            })?;
            println!("wrote {} images to {}", count, out_dir);
            return Ok(());
        }

        _ => {}
    }

    // Loading a level. On disk means a freeform saved machine; otherwise it is a puzzle
    // inside the archive.
    let level_filename = &args[2];
    let mut resources = resource_dos::from_map(root_directory, "RESOURCE.MAP")?;

    let (buf, from_archive): (Vec<u8>, bool) = match std::fs::read(level_filename) {
        Ok(b) => (b, false),
        Err(_) => (opentim::read_level_bytes(&mut resources, level_filename)?, true),
    };

    let level = load_level(&buf, !from_archive)?;
    if let Some(title) = &level.puzzle_title {
        println!("{}", title);
    }
    if let Some(objective) = &level.puzzle_objective {
        println!("{}", objective);
    }
    println!("Done loading!");

    match args.get(3).map(|s| s.as_str()) {
        Some("--play") => return render::run(&mut resources),

        Some("--screenshot") => {
            let out = args.get(4).cloned().unwrap_or_else(|| "screenshot.ppm".into());
            let ticks: u32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
            let borders = args.iter().any(|a| a == "--borders");
            return render::screenshot(&mut resources, &out, ticks, borders);
        }

        _ => {}
    }

    // Headless: step the simulation and report where the moving parts ended up.
    let ticks: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(60);

    println!("--- parts as loaded ---");
    print!("{}", opentim::parts_summary());

    for _ in 0..ticks {
        tick();
    }

    println!("--- parts after {} ticks ---", ticks);
    print!("{}", opentim::parts_summary());

    Ok(())
}
