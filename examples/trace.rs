//! Trace one part's internal state per tick, to find where two builds first disagree.
//!
//! Usage: cargo run --example trace -- <game-dir> <level> <ticks> <part-type-number>

use opentim::{load_level, part::PartType, read_level_bytes, resource_dos, tick, tim_c};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let dir = &args[1];
    let level = &args[2];
    let ticks: u32 = args[3].parse()?;
    let want: u16 = args[4].parse()?;

    let mut resources = resource_dos::from_map(dir, "RESOURCE.MAP")?;
    let bytes = read_level_bytes(&mut resources, level)?;
    load_level(&bytes, false)?;

    for t in 0..=ticks {
        for (i, part) in unsafe { tim_c::moving_parts_iter() }.enumerate() {
            if part.part_type != want {
                continue;
            }
            println!(
                "t={:3} #{} {:?} pos=({},{}) hi=({},{}) vel=({},{}) force={} mass={} extra=({},{}) f1={:04x} f2={:04x} f3={:04x} state1={} borders={}",
                t, i,
                PartType::from_u16(part.part_type),
                part.pos.x, part.pos.y,
                part.pos_x_hi_precision, part.pos_y_hi_precision,
                part.vel_hi_precision.x, part.vel_hi_precision.y,
                part.force, part.mass,
                part.extra1, part.extra2,
                part.flags1, part.flags2, part.flags3,
                part.state1,
                part.num_borders,
            );
        }
        if t < ticks {
            tick();
        }
    }
    Ok(())
}
