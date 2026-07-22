// Software renderer for the 640x480 playfield.
//
// The original is a palette-indexed 2D sprite blitter, so this draws into a plain RGB
// framebuffer rather than going through a GPU stack. Sprite pixels that the SCN decoder
// never plotted are left at alpha 0, which is what makes them transparent here.
//
// The scene-building logic (layering, wall tiling, rope sag, belt lines, the border debug
// overlay) is ported from src/nannou.rs.

use std::collections::HashMap;

use crate::part::PartType;
use crate::parts;
use crate::tim_c;

pub const SCREEN_WIDTH: usize = 640;
pub const SCREEN_HEIGHT: usize = 480;

/// TIM's teal playfield background.
const BACKGROUND: (u8, u8, u8) = (0, 160, 160);
const ROPE_COLOR: (u8, u8, u8) = (240, 176, 0);
const BLACK: (u8, u8, u8) = (0, 0, 0);

/// Number of draw layers. Layer 0 is drawn last, i.e. on top.
const LAYERS: usize = 6;

#[derive(Hash, Eq, PartialEq, Debug)]
pub enum ImageId {
    Part(u32, usize),
    PartIcon(u32),
    /// Index into NEWMOUSE.BMP. 0 is the default arrow.
    Mouse(u8),
    Misc(String, usize),
}

impl ImageId {
    pub fn new(s: &str, slice_idx: usize) -> Self {
        if s == "ICONS.BMP" {
            return ImageId::PartIcon(slice_idx as u32);
        }
        if s == "NEWMOUSE.BMP" {
            return ImageId::Mouse(slice_idx as u8);
        }
        if s.starts_with("PART") && s.ends_with(".BMP") {
            let int_s = &s[4..(s.len() - 4)];
            if let Ok(n) = int_s.parse() {
                return ImageId::Part(n, slice_idx);
            }
        }
        ImageId::Misc(s.into(), slice_idx)
    }
}

#[derive(Copy, Clone)]
pub enum Flip {
    None,
    Vertical,
    Horizontal,
    Both,
}

pub enum RenderItem {
    Image { id: ImageId, x: i32, y: i32, flip: Flip },
    Rope { x1: i32, y1: i32, x2: i32, y2: i32, sag: i32 },
    Belt { x1: i32, y1: i32, width1: i32, x2: i32, y2: i32, width2: i32 },
}

pub struct Sprite {
    pub w: i32,
    pub h: i32,
    /// RGBA8, row-major. Alpha 0 means "not plotted", i.e. transparent.
    pub rgba: Vec<u8>,
}

pub struct Sprites {
    map: HashMap<ImageId, Sprite>,
}

impl Sprites {
    pub fn load(resources: &mut crate::resource_dos::Resources) -> Result<Self, Box<dyn std::error::Error>> {
        let mut map = HashMap::new();
        crate::load_images(resources, &mut |filename, slice_idx, width, height, buf| {
            map.insert(
                ImageId::new(filename, slice_idx),
                Sprite { w: width as i32, h: height as i32, rgba: buf },
            );
        })?;
        Ok(Sprites { map })
    }

    pub fn get(&self, id: &ImageId) -> Option<&Sprite> {
        self.map.get(id)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    /// 0RGB, the layout minifb expects.
    pub px: Vec<u32>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Canvas { width, height, px: vec![0; width * height] }
    }

    pub fn clear(&mut self, (r, g, b): (u8, u8, u8)) {
        let v = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
        for p in self.px.iter_mut() {
            *p = v;
        }
    }

    #[inline]
    fn plot(&mut self, x: i32, y: i32, (r, g, b): (u8, u8, u8)) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let v = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
        self.px[y as usize * self.width + x as usize] = v;
    }

    /// Blit a sprite, skipping fully transparent pixels.
    pub fn blit(&mut self, sprite: &Sprite, x: i32, y: i32, flip: Flip) {
        for sy in 0..sprite.h {
            for sx in 0..sprite.w {
                let off = (sy * sprite.w + sx) as usize * 4;
                let a = sprite.rgba[off + 3];
                if a == 0 {
                    continue;
                }
                let (dx, dy) = match flip {
                    Flip::None => (sx, sy),
                    Flip::Horizontal => (sprite.w - 1 - sx, sy),
                    Flip::Vertical => (sx, sprite.h - 1 - sy),
                    Flip::Both => (sprite.w - 1 - sx, sprite.h - 1 - sy),
                };
                let c = (sprite.rgba[off], sprite.rgba[off + 1], sprite.rgba[off + 2]);
                self.plot(x + dx, y + dy, c);
            }
        }
    }

    /// Bresenham, thickened by stamping a square of `weight` pixels.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: (u8, u8, u8), weight: i32) {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        let half = weight / 2;

        loop {
            for oy in -half..=half {
                for ox in -half..=half {
                    self.plot(x + ox, y + oy, color);
                }
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    pub fn polyline(&mut self, pts: &[(f32, f32)], color: (u8, u8, u8), weight: i32) {
        for w in pts.windows(2) {
            self.line(
                w[0].0 as i32, w[0].1 as i32,
                w[1].0 as i32, w[1].1 as i32,
                color, weight,
            );
        }
    }

    /// Write the framebuffer out as a binary PPM (P6).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_ppm(&self, path: &str) -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
        write!(&mut f, "P6\n{} {}\n255\n", self.width, self.height)?;
        for p in self.px.iter() {
            f.write_all(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8])?;
        }
        Ok(())
    }
}

#[inline(always)]
fn lerp(a: f32, b: f32, p: f32) -> f32 {
    (b - a) * p + a
}

/// Return points along a quadratic bezier curve.
fn quad_bezier_curve(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), iters: u32) -> Vec<(f32, f32)> {
    (0..=iters)
        .map(|i| {
            let t = (i as f32) / (iters as f32);
            (
                lerp(lerp(p0.0, p1.0, t), lerp(p1.0, p2.0, t), t),
                lerp(lerp(p0.1, p1.1, t), lerp(p1.1, p2.1, t), t),
            )
        })
        .collect()
}

/// Walk the part lists and build the draw list, back to front.
pub fn build_render_items() -> Vec<RenderItem> {
    let mut layers: Vec<Vec<RenderItem>> = (0..LAYERS).map(|_| vec![]).collect();

    let iter = unsafe { tim_c::static_parts_iter().chain(tim_c::moving_parts_iter()) };
    for part in iter {
        let part_type = PartType::from_u16(part.part_type);
        let layer = (parts::get_def(part_type).goobers.0 as usize).min(LAYERS - 1);

        match part_type {
            PartType::Belt => {
                if let Some(((x1, y1, width1), (x2, y2, width2))) = part.belt_section() {
                    layers[layer].push(RenderItem::Belt {
                        x1: x1 as i32, y1: y1 as i32, width1: width1 as i32,
                        x2: x2 as i32, y2: y2 as i32, width2: width2 as i32,
                    });
                }
            }

            PartType::Rope => {
                if let Some(sections) = part.rope_sections() {
                    for ((x1, y1), (x2, y2), sag) in sections.into_iter() {
                        layers[layer].push(RenderItem::Rope {
                            x1: x1 as i32, y1: y1 as i32,
                            x2: x2 as i32, y2: y2 as i32,
                            sag: sag as i32,
                        });
                    }
                }
            }

            PartType::BrickWall | PartType::DirtWall | PartType::WoodWall | PartType::PipeStraight => {
                // Walls are tiled from 16px end caps plus two alternating middle slices.
                let start_x = part.pos_render.x as i32;
                let start_y = part.pos_render.y as i32;
                let t = part.part_type as u32;

                if part.size.x == 16 {
                    let count = (part.size.y / 16) as i32;
                    layers[layer].push(RenderItem::Image { id: ImageId::Part(t, 4), x: start_x, y: start_y, flip: Flip::None });
                    for i in 0..count - 2 {
                        let image = if i % 2 == 0 { 5 } else { 6 };
                        layers[layer].push(RenderItem::Image { id: ImageId::Part(t, image), x: start_x, y: start_y + (i + 1) * 16, flip: Flip::None });
                    }
                    layers[layer].push(RenderItem::Image { id: ImageId::Part(t, 7), x: start_x, y: start_y + (count - 1) * 16, flip: Flip::None });
                } else {
                    let count = (part.size.x / 16) as i32;
                    layers[layer].push(RenderItem::Image { id: ImageId::Part(t, 0), x: start_x, y: start_y, flip: Flip::None });
                    for i in 0..count - 2 {
                        let image = if i % 2 == 0 { 1 } else { 2 };
                        layers[layer].push(RenderItem::Image { id: ImageId::Part(t, image), x: start_x + (i + 1) * 16, y: start_y, flip: Flip::None });
                    }
                    layers[layer].push(RenderItem::Image { id: ImageId::Part(t, 3), x: start_x + (count - 1) * 16, y: start_y, flip: Flip::None });
                }
            }

            _ => {
                if part.flags2 & 0x2000 == 0 {
                    let flip = match ((part.flags2 & 0x10) != 0, (part.flags2 & 0x20) != 0) {
                        (false, false) => Flip::None,
                        (true, false) => Flip::Horizontal,
                        (false, true) => Flip::Vertical,
                        (true, true) => Flip::Both,
                    };

                    let def = parts::get_def(part_type);
                    if let Some(&part_images) = def.render_images.and_then(|l| l.get(part.state1 as usize)) {
                        // This part renders several images together.
                        let part_x = part.pos.x as i32;
                        let part_y = part.pos.y as i32;
                        for &(goober, index, x, y) in part_images {
                            // TODO - flipping. use size_something to figure positions out.
                            let l = (goober as usize).min(LAYERS - 1);
                            layers[l].push(RenderItem::Image {
                                id: ImageId::Part(part.part_type as u32, index as usize),
                                x: part_x + x as i32,
                                y: part_y + y as i32,
                                flip,
                            });
                        }
                    } else {
                        // This part renders a single image.
                        layers[layer].push(RenderItem::Image {
                            id: ImageId::Part(part.part_type as u32, part.state1 as usize),
                            x: part.pos_render.x as i32,
                            y: part.pos_render.y as i32,
                            flip,
                        });
                    }
                }
            }
        }
    }

    // Layer 0 sits above everything, so it has to be drawn last.
    let mut render_items = vec![];
    for layer_items in layers.iter_mut().rev() {
        render_items.append(layer_items);
    }
    render_items
}

pub fn draw_scene(canvas: &mut Canvas, sprites: &Sprites, items: &[RenderItem]) {
    canvas.clear(BACKGROUND);

    for item in items {
        match item {
            RenderItem::Image { id, x, y, flip } => {
                if let Some(s) = sprites.get(id) {
                    canvas.blit(s, *x, *y, *flip);
                }
            }
            &RenderItem::Rope { x1, y1, x2, y2, sag } => {
                let pts = quad_bezier_curve(
                    (x1 as f32, y1 as f32),
                    (((x1 + x2) / 2) as f32, ((y1 + y2) / 2 + sag) as f32),
                    (x2 as f32, y2 as f32),
                    10,
                );
                canvas.polyline(&pts, BLACK, 4);
                canvas.polyline(&pts, ROPE_COLOR, 2);
            }
            &RenderItem::Belt { x1, y1, width1, x2, y2, width2 } => {
                let (x1, y1, x2, y2) = (x1 as f32, y1 as f32, x2 as f32, y2 as f32);
                let (w1, w2) = (width1 as f32, width2 as f32);

                let angle = f32::atan2(y2 - y1, x2 - x1);
                let ss = f32::sin(angle);
                let cc = -f32::cos(angle);
                let (x1c, y1c) = (x1 + w1 / 2.0, y1 + w1 / 2.0);
                let (x2c, y2c) = (x2 + w2 / 2.0, y2 + w2 / 2.0);

                canvas.line(
                    (x1c - ss * w1 / 2.0) as i32, (y1c - cc * w1 / 2.0) as i32,
                    (x2c - ss * w2 / 2.0) as i32, (y2c - cc * w2 / 2.0) as i32,
                    BLACK, 2,
                );
                canvas.line(
                    (x1c + ss * w1 / 2.0) as i32, (y1c + cc * w1 / 2.0) as i32,
                    (x2c + ss * w2 / 2.0) as i32, (y2c + cc * w2 / 2.0) as i32,
                    BLACK, 2,
                );
            }
        }
    }
}

/// Debug overlay: bounding boxes, collision borders and border normals.
pub fn draw_borders(canvas: &mut Canvas) {
    canvas.clear(BACKGROUND);

    let iter = unsafe { tim_c::static_parts_iter().chain(tim_c::moving_parts_iter()) };
    for part in iter {
        let part_x = part.pos_x_hi_precision as f32 / 512.0;
        let part_y = part.pos_y_hi_precision as f32 / 512.0;

        // Shape origin
        canvas.line(part_x as i32 - 2, part_y as i32, part_x as i32 + 2, part_y as i32, BLACK, 1);
        canvas.line(part_x as i32, part_y as i32 - 2, part_x as i32, part_y as i32 + 2, BLACK, 1);

        // pos_render + size box (red)
        let ox = part.pos_render.x as i32;
        let oy = part.pos_render.y as i32;
        let (bx, by) = (ox + part.size.x as i32, oy + part.size.y as i32);
        canvas.polyline(
            &[(ox as f32, oy as f32), (bx as f32, oy as f32), (bx as f32, by as f32), (ox as f32, by as f32), (ox as f32, oy as f32)],
            (255, 0, 0), 1,
        );

        // hi-precision pos + size_something2 box (blue)
        let (cx, cy) = (part_x + part.size_something2.x as f32, part_y + part.size_something2.y as f32);
        canvas.polyline(
            &[(part_x, part_y), (cx, part_y), (cx, cy), (part_x, cy), (part_x, part_y)],
            (0, 0, 255), 1,
        );

        // Collision border
        let border: Vec<(f32, f32)> = part
            .border_points()
            .iter()
            .map(|p| (part_x + p.x as f32, part_y + p.y as f32))
            .collect();
        if border.len() > 1 {
            let mut closed = border.clone();
            closed.push(border[0]);
            canvas.polyline(&closed, (0, 0, 0), 1);
        }

        // Border normals
        let pts = part.border_points();
        for (i, a) in pts.iter().enumerate() {
            let b = &pts[(i + 1) % pts.len()];
            let normal = a.normal_angle as f32 / 65536.0;
            let ox = part_x + (a.x as f32 + b.x as f32) / 2.0;
            let oy = part_y + (a.y as f32 + b.y as f32) / 2.0;
            let s = f32::sin(normal * std::f32::consts::PI * 2.0) * 3.0;
            let c = f32::cos(normal * std::f32::consts::PI * 2.0) * 3.0;
            canvas.line(ox as i32, oy as i32, (ox - s) as i32, (oy + c) as i32, (255, 255, 255), 1);
        }
    }
}

/// Render a single frame after `ticks` simulation steps and write it to a PPM.
#[cfg(not(target_arch = "wasm32"))]
pub fn screenshot(resources: &mut crate::resource_dos::Resources, path: &str, ticks: u32, show_borders: bool) -> Result<(), Box<dyn std::error::Error>> {
    let sprites = Sprites::load(resources)?;
    println!("loaded {} sprites", sprites.len());

    unsafe {
        for _ in 0..ticks {
            tim_c::advance_parts();
            tim_c::all_parts_set_prev_vars();
        }
    }

    let mut canvas = Canvas::new(SCREEN_WIDTH, SCREEN_HEIGHT);
    if show_borders {
        draw_borders(&mut canvas);
    } else {
        let items = build_render_items();
        draw_scene(&mut canvas, &sprites, &items);
    }
    canvas.write_ppm(path)?;
    println!("wrote {} after {} ticks", path, ticks);
    Ok(())
}

/// Open a desktop window and run the simulation.
#[cfg(not(target_arch = "wasm32"))]
pub fn run(resources: &mut crate::resource_dos::Resources) -> Result<(), Box<dyn std::error::Error>> {
    use minifb::{Key, Window, WindowOptions};

    let sprites = Sprites::load(resources)?;
    println!("loaded {} sprites", sprites.len());

    let mut window = Window::new(
        "OpenTIM - space: run/pause, b: borders, s: screenshot, esc: quit",
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        WindowOptions { scale: minifb::Scale::X2, ..WindowOptions::default() },
    )?;

    // The original ran the simulation at roughly 30Hz.
    window.set_target_fps(30);

    let mut canvas = Canvas::new(SCREEN_WIDTH, SCREEN_HEIGHT);
    let mut running = false;
    let mut show_borders = false;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        for key in window.get_keys_pressed(minifb::KeyRepeat::No) {
            match key {
                Key::Space => running = !running,
                Key::B => show_borders = !show_borders,
                Key::G => {
                    crate::debug::dump_level_to_graphviz_file("out.gv").ok();
                    println!("wrote out.gv");
                }
                Key::S => {
                    canvas.write_ppm("screenshot.ppm").ok();
                    println!("wrote screenshot.ppm");
                }
                _ => {}
            }
        }

        if running {
            unsafe {
                tim_c::advance_parts();
                tim_c::all_parts_set_prev_vars();
            }
        }

        if show_borders {
            draw_borders(&mut canvas);
        } else {
            let items = build_render_items();
            draw_scene(&mut canvas, &sprites, &items);
        }

        window.update_with_buffer(&canvas.px, SCREEN_WIDTH, SCREEN_HEIGHT)?;
    }

    Ok(())
}
