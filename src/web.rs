// Browser front end.
//
// Talks to a <canvas> through web-sys rather than going via minifb. The framebuffer that
// render::Canvas already produces is exactly an ImageData, so drawing is a straight blit.
//
// The important structural difference from the desktop build is the loop: a browser tab
// cannot run `while window.is_open()`, because blocking means never yielding to the event
// loop, so nothing would ever repaint and no input would arrive. Instead each frame is a
// requestAnimationFrame callback.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use js_sys::{Object, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

use crate::render::{self, Canvas, Sprites, SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::resource_dos;

/// Simulation rate, matching the desktop build's 30Hz target.
///
/// Ticking once per animation frame would tie simulation speed to the display: a 120Hz
/// screen ran the game at four times the intended rate. Instead we accumulate elapsed
/// time and run whole ticks out of it.
const TICK_MS: f64 = 1000.0 / 30.0;

/// Cap on the time one frame may contribute, so returning to a backgrounded tab does not
/// try to catch up thousands of ticks at once.
const MAX_FRAME_MS: f64 = 250.0;

fn window() -> web_sys::Window {
    web_sys::window().expect("no global window")
}

/// The loaded game, held across animation frames.
struct State {
    sprites: Sprites,
    canvas_buf: Canvas,
    ctx: CanvasRenderingContext2d,
    running: bool,
    show_borders: bool,
    /// Timestamp of the previous animation frame, and left-over time not yet ticked.
    last_time: f64,
    accumulator: f64,
}

thread_local! {
    static STATE: RefCell<Option<State>> = RefCell::new(None);
}

/// Handle to the archive, kept between `Game::new` and `start`.
#[wasm_bindgen]
pub struct Game {
    resources: resource_dos::Resources,
}

#[wasm_bindgen]
impl Game {
    /// Build from the user's own game files: a JS object mapping upper-case filename to a
    /// Uint8Array, which must include RESOURCE.MAP and every RESOURCE.* it references.
    #[wasm_bindgen(constructor)]
    pub fn new(files: &Object) -> Result<Game, JsValue> {
        console_error_panic_hook::set_once();

        let mut map: HashMap<String, Vec<u8>> = HashMap::new();
        let entries = Object::entries(files);
        for entry in entries.iter() {
            let pair: js_sys::Array = entry.into();
            let name = pair.get(0).as_string().ok_or("file key was not a string")?;
            let bytes = Uint8Array::new(&pair.get(1)).to_vec();
            map.insert(name.to_uppercase(), bytes);
        }

        let map_bytes = map
            .get("RESOURCE.MAP")
            .ok_or("RESOURCE.MAP not among the supplied files")?
            .clone();

        let resources = resource_dos::from_map_bytes(&map_bytes, |name| {
            map.get(&name.to_uppercase()).cloned()
        })
        .map_err(|e| JsValue::from_str(&format!("could not read archive: {}", e)))?;

        Ok(Game { resources })
    }

    /// Names of everything in the archive, for populating a level picker.
    pub fn resource_names(&self) -> Vec<JsValue> {
        let mut names: Vec<String> = self.resources.iter_filenames().map(|s| s.to_string()).collect();
        names.sort();
        names.into_iter().map(|s| JsValue::from_str(&s)).collect()
    }

    /// Load a level from the archive and install it into the simulation.
    /// Returns the puzzle objective, when the level has one.
    pub fn load_level(&mut self, name: &str) -> Result<Option<String>, JsValue> {
        let bytes = crate::read_level_bytes(&mut self.resources, name)
            .map_err(|e| JsValue::from_str(&format!("{}", e)))?;
        let level = crate::load_level(&bytes, false)
            .map_err(|e| JsValue::from_str(&format!("{}", e)))?;
        Ok(level.puzzle_objective)
    }

    /// Advance the simulation by `n` ticks without rendering.
    pub fn tick_n(&mut self, n: u32) {
        for _ in 0..n {
            crate::tick();
        }
    }

    /// One line per part. Identical in form to the CLI's headless dump, so the two can be
    /// diffed to check the engine behaves the same compiled to wasm.
    pub fn parts_summary(&self) -> String {
        crate::parts_summary()
    }

    /// Set whether the simulation is advancing.
    pub fn set_running(&self, running: bool) {
        STATE.with(|s| {
            if let Some(state) = s.borrow_mut().as_mut() {
                state.running = running;
            }
        });
    }

    /// Decode the sprites, attach a canvas to `container_id`, and start the render loop.
    ///
    /// Takes &mut self rather than self so the Game stays alive and `load_level` can swap
    /// levels underneath a running loop.
    pub fn start(&mut self, container_id: &str) -> Result<(), JsValue> {
        let resources = &mut self.resources;
        let sprites = Sprites::load(resources)
            .map_err(|e| JsValue::from_str(&format!("could not decode sprites: {}", e)))?;

        let document = window().document().ok_or("no document")?;
        let container = document
            .get_element_by_id(container_id)
            .ok_or_else(|| JsValue::from_str(&format!("no element with id '{}'", container_id)))?;

        let canvas: HtmlCanvasElement = document
            .create_element("canvas")?
            .dyn_into::<HtmlCanvasElement>()?;
        canvas.set_width(SCREEN_WIDTH as u32);
        canvas.set_height(SCREEN_HEIGHT as u32);
        // Scale up without smoothing; the art is 1992 pixel art and must stay crisp.
        canvas
            .style()
            .set_property("image-rendering", "pixelated")?;
        canvas.style().set_property("width", &format!("{}px", SCREEN_WIDTH * 2))?;
        canvas.style().set_property("height", &format!("{}px", SCREEN_HEIGHT * 2))?;
        // Needed for the canvas to receive key events.
        canvas.set_tab_index(0);
        container.append_child(&canvas)?;

        let ctx = canvas
            .get_context("2d")?
            .ok_or("no 2d context")?
            .dyn_into::<CanvasRenderingContext2d>()?;

        install_input(&canvas)?;

        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                sprites,
                canvas_buf: Canvas::new(SCREEN_WIDTH, SCREEN_HEIGHT),
                ctx,
                running: false,
                show_borders: false,
                last_time: 0.0,
                accumulator: 0.0,
            });
        });

        let _ = canvas.focus();
        start_animation_loop();
        Ok(())
    }
}

/// Space toggles running, B toggles the collision-border overlay.
fn install_input(canvas: &HtmlCanvasElement) -> Result<(), JsValue> {
    let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        let key = e.key();
        STATE.with(|s| {
            if let Some(state) = s.borrow_mut().as_mut() {
                match key.as_str() {
                    " " => {
                        state.running = !state.running;
                        e.prevent_default();
                    }
                    "b" | "B" => state.show_borders = !state.show_borders,
                    _ => {}
                }
            }
        });
    });
    canvas.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref())?;
    on_key.forget();
    Ok(())
}

/// Drive the simulation and repaint, one animation frame at a time.
fn start_animation_loop() {
    let callback = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
    let handle = callback.clone();

    *callback.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::new(move |now: f64| {
        STATE.with(|s| {
            if let Some(state) = s.borrow_mut().as_mut() {
                // Run simulation ticks from elapsed wall-clock time, so the rate does not
                // depend on the display's refresh rate.
                if state.last_time == 0.0 {
                    state.last_time = now;
                }
                let elapsed = (now - state.last_time).clamp(0.0, MAX_FRAME_MS);
                state.last_time = now;

                if state.running {
                    state.accumulator += elapsed;
                    while state.accumulator >= TICK_MS {
                        crate::tick();
                        state.accumulator -= TICK_MS;
                    }
                } else {
                    state.accumulator = 0.0;
                }

                if state.show_borders {
                    render::draw_borders(&mut state.canvas_buf);
                } else {
                    let items = render::build_render_items();
                    render::draw_scene(&mut state.canvas_buf, &state.sprites, &items);
                }

                paint(state);
            }
        });

        // Queue the next frame. The borrow is bound so the temporary Ref outlives the call.
        let next = handle.borrow();
        if let Some(cb) = next.as_ref() {
            request_animation_frame(cb);
        }
    }));

    let first = callback.borrow();
    if let Some(cb) = first.as_ref() {
        request_animation_frame(cb);
    }
}

fn request_animation_frame(cb: &Closure<dyn FnMut(f64)>) {
    window()
        .request_animation_frame(cb.as_ref().unchecked_ref())
        .expect("requestAnimationFrame failed");
}

/// Convert the 0RGB framebuffer to RGBA and blit it to the canvas.
fn paint(state: &mut State) {
    let mut rgba = Vec::with_capacity(state.canvas_buf.px.len() * 4);
    for p in state.canvas_buf.px.iter() {
        rgba.push((p >> 16) as u8);
        rgba.push((p >> 8) as u8);
        rgba.push(*p as u8);
        rgba.push(255);
    }

    if let Ok(image) = ImageData::new_with_u8_clamped_array_and_sh(
        Clamped(&rgba),
        SCREEN_WIDTH as u32,
        SCREEN_HEIGHT as u32,
    ) {
        let _ = state.ctx.put_image_data(&image, 0.0, 0.0);
    }
}
