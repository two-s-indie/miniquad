//! DPI baseline: see docs/windows-dpi-validation.md for the paired Macroquad example.
//! Log geometry at startup and in callbacks without compensating for backend DPI bugs.

use miniquad::*;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    framebuffer: (f32, f32),
    scale: f32,
}

impl Geometry {
    fn read() -> Self {
        Self {
            framebuffer: window::screen_size(),
            scale: window::dpi_scale(),
        }
    }

    fn logical_size(self) -> (f32, f32) {
        (
            self.framebuffer.0 / self.scale,
            self.framebuffer.1 / self.scale,
        )
    }
}

#[repr(C)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

struct Diagnostic {
    ctx: Box<dyn RenderingBackend>,
    pipeline: Pipeline,
    bindings: Bindings,
    geometry: Geometry,
    mouse: Option<(f32, f32)>,
    started: f64,
    frames: u64,
    fullscreen: bool,
    grabbed: bool,
}

impl Diagnostic {
    fn new() -> Self {
        let mut ctx = window::new_rendering_backend();
        let shader = ctx
            .new_shader(
                match ctx.info().backend {
                    Backend::OpenGl => ShaderSource::Glsl {
                        vertex: VERTEX,
                        fragment: FRAGMENT,
                    },
                    Backend::Metal => ShaderSource::Msl { program: METAL },
                },
                ShaderMeta {
                    images: vec![],
                    uniforms: UniformBlockLayout { uniforms: vec![] },
                },
            )
            .unwrap();
        let pipeline = ctx.new_pipeline(
            &[BufferLayout::default()],
            &[
                VertexAttribute::new("position", VertexFormat::Float2),
                VertexAttribute::new("color", VertexFormat::Float4),
            ],
            shader,
            PipelineParams::default(),
        );
        let bindings = Bindings {
            vertex_buffers: vec![ctx.new_buffer(
                BufferType::VertexBuffer,
                BufferUsage::Stream,
                BufferSource::empty::<Vertex>(24),
            )],
            index_buffer: ctx.new_buffer(
                BufferType::IndexBuffer,
                BufferUsage::Immutable,
                BufferSource::slice(&(0..24u16).collect::<Vec<_>>()),
            ),
            images: vec![],
        };
        let diagnostic = Self {
            ctx,
            pipeline,
            bindings,
            geometry: Geometry::read(),
            mouse: None,
            started: date::now(),
            frames: 0,
            fullscreen: false,
            grabbed: false,
        };
        diagnostic.report("startup");
        diagnostic
    }

    fn report(&self, event: &str) {
        // Read again inside the callback: this exposes stale published state.
        let geometry = Geometry::read();
        println!(
            "t={:.3} frame={} event={} dpi={:.4} framebuffer={:?} logical={:?} mouse_fb={:?} mouse_logical={:?}",
            date::now() - self.started,
            self.frames,
            event,
            geometry.scale,
            geometry.framebuffer,
            geometry.logical_size(),
            self.mouse,
            self.mouse.map(|(x, y)| (x / geometry.scale, y / geometry.scale)),
        );
    }

    fn request_size(&self, width: u32, height: u32) {
        let scale = window::dpi_scale();
        let pixels = (
            (width as f32 * scale).round() as u32,
            (height as f32 * scale).round() as u32,
        );
        println!(
            "request logical={}x{} framebuffer={:?}",
            width, height, pixels
        );
        // Windows runtime requests are already in framebuffer pixels.
        window::set_window_size(pixels.0, pixels.1);
    }
}

impl EventHandler for Diagnostic {
    fn update(&mut self) {
        let geometry = Geometry::read();
        if geometry != self.geometry {
            self.report("geometry_changed");
            self.geometry = geometry;
        }
    }

    fn draw(&mut self) {
        self.frames += 1;
        let (width, height) = self.geometry.logical_size();
        let (left, top) = ((width - 200.0) / 2.0, (height - 120.0) / 2.0);
        let mouse = self
            .mouse
            .map(|(x, y)| (x / self.geometry.scale, y / self.geometry.scale));
        let hovered = mouse.map_or(false, |(x, y)| {
            x >= left && x <= left + 200.0 && y >= top && y <= top + 120.0
        });
        let mut vertices = Vec::with_capacity(24);
        rectangle(
            &mut vertices,
            (width, height),
            [left, top, 200.0, 120.0],
            if hovered {
                [0.95, 0.65, 0.15, 1.0]
            } else {
                [0.2, 0.55, 0.85, 1.0]
            },
        );
        // A changing indicator makes a new draw visible in blocking mode.
        rectangle(
            &mut vertices,
            (width, height),
            [10.0, 10.0, 24.0, 8.0],
            [0.3, if self.frames % 2 == 0 { 0.9 } else { 0.3 }, 0.4, 1.0],
        );
        if let Some((x, y)) = mouse {
            rectangle(
                &mut vertices,
                (width, height),
                [x - 10.0, y - 1.0, 20.0, 2.0],
                [1.0; 4],
            );
            rectangle(
                &mut vertices,
                (width, height),
                [x - 1.0, y - 10.0, 2.0, 20.0],
                [1.0; 4],
            );
        }
        self.ctx.buffer_update(
            self.bindings.vertex_buffers[0],
            BufferSource::slice(&vertices),
        );
        self.ctx
            .begin_default_pass(PassAction::clear_color(0.06, 0.08, 0.1, 1.0));
        self.ctx.apply_pipeline(&self.pipeline);
        self.ctx.apply_bindings(&self.bindings);
        self.ctx.draw(0, vertices.len() as i32, 1);
        self.ctx.end_render_pass();
        self.ctx.commit_frame();
    }

    fn resize_event(&mut self, width: f32, height: f32) {
        println!("resize_event arguments=({width}, {height})");
        self.report("resize_event");
        window::schedule_update();
    }

    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        self.mouse = Some((x, y));
        self.report("mouse_motion");
        window::schedule_update();
    }

    fn mouse_button_down_event(&mut self, _: MouseButton, x: f32, y: f32) {
        self.mouse = Some((x, y));
        self.report("mouse_down");
        window::schedule_update();
    }

    fn key_down_event(&mut self, key: KeyCode, _: KeyMods, repeat: bool) {
        if repeat {
            return;
        }
        match key {
            KeyCode::Escape => window::order_quit(),
            KeyCode::R => self.request_size(800, 600),
            KeyCode::O => self.request_size(801, 601),
            KeyCode::F => {
                self.fullscreen = !self.fullscreen;
                window::set_fullscreen(self.fullscreen);
            }
            KeyCode::C => {
                self.grabbed = !self.grabbed;
                window::set_cursor_grab(self.grabbed);
            }
            KeyCode::P => self.report("snapshot"),
            _ => {}
        }
        window::schedule_update();
    }

    fn window_minimized_event(&mut self) {
        self.report("minimized");
    }
    fn window_restored_event(&mut self) {
        self.report("restored");
    }
}

fn rectangle(vertices: &mut Vec<Vertex>, size: (f32, f32), rect: [f32; 4], color: [f32; 4]) {
    let [x, y, w, h] = rect;
    for (x, y) in [
        (x, y),
        (x + w, y),
        (x + w, y + h),
        (x, y),
        (x + w, y + h),
        (x, y + h),
    ] {
        vertices.push(Vertex {
            position: [2.0 * x / size.0 - 1.0, 1.0 - 2.0 * y / size.1],
            color,
        });
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args
        .iter()
        .any(|arg| !["--low-dpi", "--blocking", "--odd-size"].contains(&arg.as_str()))
    {
        eprintln!("Usage: dpi_diagnostic [--low-dpi] [--blocking] [--odd-size]");
        return;
    }
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    let odd = has("--odd-size");
    let conf = conf::Conf {
        window_title: "Miniquad DPI diagnostic".into(),
        window_width: if odd { 801 } else { 800 },
        window_height: if odd { 601 } else { 600 },
        high_dpi: !has("--low-dpi"),
        platform: conf::Platform {
            blocking_event_loop: has("--blocking"),
            ..Default::default()
        },
        ..Default::default()
    };
    println!(
        "source={} requested={}x{} high_dpi={} blocking={}",
        env!("CARGO_MANIFEST_DIR"),
        conf.window_width,
        conf.window_height,
        conf.high_dpi,
        conf.platform.blocking_event_loop
    );
    println!("R: request 800x600 logical; O: 801x601; F: fullscreen; C: cursor grab; P: log snapshot; Esc: quit");
    miniquad::start(conf, || Box::new(Diagnostic::new()));
}

const VERTEX: &str = r#"#version 100
attribute vec2 position;
attribute vec4 color;
varying lowp vec4 vertex_color;
void main() { gl_Position = vec4(position, 0.0, 1.0); vertex_color = color; }
"#;

const FRAGMENT: &str = r#"#version 100
varying lowp vec4 vertex_color;
void main() { gl_FragColor = vertex_color; }
"#;

const METAL: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct Vertex { float2 position [[attribute(0)]]; float4 color [[attribute(1)]]; };
struct Raster { float4 position [[position]]; float4 color; };
vertex Raster vertexShader(Vertex v [[stage_in]]) {
    Raster out; out.position = float4(v.position, 0.0, 1.0); out.color = v.color; return out;
}
fragment float4 fragmentShader(Raster in [[stage_in]]) { return in.color; }
"#;
