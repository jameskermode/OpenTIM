//! Load several levels in one process and report the part counts, to check that loading a
//! level replaces the previous world rather than adding to it.
//!
//! Usage: cargo run --example reload -- <game-dir> <level> [level...]

use opentim::{load_level, read_level_bytes, resource_dos, tim_c};

fn counts() -> (usize, usize, usize) {
    unsafe {
        (
            tim_c::static_parts_iter().count(),
            tim_c::moving_parts_iter().count(),
            tim_c::PartsIterator::new(tim_c::PARTS_BIN_ROOT.next).count(),
        )
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut resources = resource_dos::from_map(&args[1], "RESOURCE.MAP")?;

    let ticks: u32 = std::env::var("RELOAD_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);

    for name in &args[2..] {
        let bytes = read_level_bytes(&mut resources, name)?;
        load_level(&bytes, false)?;
        let (s, m, b) = counts();
        println!("after loading {:<10} static={:<4} moving={:<4} bin={}", name, s, m, b);
    }

    // Simulate the final level and dump it, so a reloaded world can be compared against a
    // freshly loaded one.
    for _ in 0..ticks {
        opentim::tick();
    }
    if ticks > 0 {
        print!("{}", opentim::parts_summary());
    }

    Ok(())
}
