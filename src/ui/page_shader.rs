//! The game drawn through a RetroArch shader as an egui paint callback, so the
//! chrome stays unfiltered: the game is extracted at its own resolution, the
//! page drawn plain around it, then the shader over the game's rect.

use crate::browser::GameGeometry;
use crate::platform::render::shaders;
use egui_sdl2::egui;
use egui_sdl2::egui_glow::{self, glow, glow::HasContext};
use std::cell::Cell;
use std::sync::{Arc, Mutex};

/// The unit quad as a triangle strip.
const QUAD: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
/// Attribute slots: the internal passes' `a_pos` and RetroArch's `VertexCoord`
/// share the first, `TexCoord` takes the second.
const A_POS: u32 = 0;
const A_TEX: u32 = 1;
/// RetroArch's default precision on GLES, for shaders that declare none;
/// redeclaring it is legal.
const GLES_PRECISION: &str = "#ifdef GL_ES\nprecision mediump float;\n#endif\n";
/// Column-major ortho projection taking the unit quad to clip space.
const MVP: [f32; 16] = [
    2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0,
];

/// What was built for `id`; `passes` is `None` when not even the internal
/// passes compiled.
struct Built {
    id: String,
    passes: Option<Passes>,
}

/// The page texture and what is built for the current shader.
pub(super) struct PageShader {
    texture: glow::NativeTexture,
    built: Arc<Mutex<Option<Built>>>,
    /// Whether a frame has drawn through a shader since the last [`Self::reset`].
    live: Cell<bool>,
}

/// Where the game is and what it is, in output px from the bottom-left corner.
#[derive(Clone, Copy)]
struct Frame {
    size: (f32, f32),
    /// The game's own resolution.
    source_size: (f32, f32),
    /// Where the game is drawn: x, y, w, h.
    source_rect: [f32; 4],
}

impl Frame {
    /// From the page's `geometry`, else as if the whole page were a game of
    /// [`shaders::guessed_period`]-sized pixels.
    fn new((w, h): (f32, f32), geometry: Option<GameGeometry>) -> Self {
        match geometry {
            Some(GameGeometry {
                source,
                rect: [x, top, rw, rh],
            }) => Self {
                size: (w, h),
                source_size: source,
                // The page reports its top edge; GL counts from the bottom.
                source_rect: [x, h - top - rh, rw, rh],
            },
            None => {
                let period = shaders::guessed_period(h);
                Self {
                    size: (w, h),
                    source_size: ((w / period).round(), (h / period).round()),
                    source_rect: [0.0, 0.0, w, h],
                }
            }
        }
    }
}

impl PageShader {
    pub(super) fn new(texture: glow::NativeTexture) -> Self {
        Self {
            texture,
            built: Arc::default(),
            live: Cell::new(false),
        }
    }

    /// Make the next [`Self::paint`] rebuild, reloading the file. That paint
    /// deletes the old programs, as only it runs with the GL context current.
    pub(super) fn reset(&self) {
        if !self.live.replace(false) {
            return;
        }
        let mut built = self
            .built
            .lock()
            .expect("the paint callback never panics holding it");
        if let Some(built) = built.as_mut() {
            built.id.clear();
        }
    }

    /// Draw the page over `rect` and the game through shader `id`;
    /// [`shaders::OFF`] is the caller's to draw as a plain image.
    pub(super) fn paint(
        &self,
        ui: &egui::Ui,
        rect: egui::Rect,
        id: &str,
        geometry: Option<GameGeometry>,
    ) {
        self.live.set(true);
        let texture = self.texture;
        let built = self.built.clone();
        let id = id.to_string();
        let callback = egui_glow::CallbackFn::new(move |info, painter| {
            let gl = painter.gl();
            let mut built = built
                .lock()
                .expect("the paint callback never panics holding it");
            if built.as_ref().is_none_or(|b| b.id != id) {
                if let Some(Built {
                    passes: Some(old), ..
                }) = built.take()
                {
                    unsafe { old.delete(gl) };
                }
                let passes = unsafe { Passes::build(gl, &id) };
                *built = Some(Built {
                    id: id.clone(),
                    passes,
                });
            }
            let Some(Built {
                passes: Some(passes),
                ..
            }) = built.as_mut()
            else {
                return;
            };
            let vp = info.viewport_in_pixels();
            let frame = Frame::new((vp.width_px as f32, vp.height_px as f32), geometry);
            unsafe { passes.draw(gl, texture, &frame, (vp.left_px, vp.from_bottom_px)) };
        });
        ui.painter().add(egui::PaintCallback {
            rect,
            callback: Arc::new(callback),
        });
    }
}

/// A linked internal pass and its uniforms.
#[derive(Clone, Copy)]
struct Internal {
    program: glow::Program,
    u_page: Option<glow::UniformLocation>,
    u_size: Option<glow::UniformLocation>,
    u_source_size: Option<glow::UniformLocation>,
    u_source_rect: Option<glow::UniformLocation>,
}

/// A linked RetroArch shader and the uniforms it may declare.
#[derive(Clone, Copy)]
struct Retro {
    program: glow::Program,
    mvp: Option<glow::UniformLocation>,
    texture: Option<glow::UniformLocation>,
    texture_size: Option<glow::UniformLocation>,
    input_size: Option<glow::UniformLocation>,
    output_size: Option<glow::UniformLocation>,
    frame_count: Option<glow::UniformLocation>,
    frame_direction: Option<glow::UniformLocation>,
}

/// The game at its own resolution, rendered to for the shader to sample.
struct Target {
    fbo: glow::Framebuffer,
    texture: glow::Texture,
    size: (i32, i32),
}

/// Everything one shader draws with.
struct Passes {
    vbo: glow::Buffer,
    /// `None` on GLES 2 / GL 2, where attributes are set per draw instead.
    vao: Option<glow::VertexArray>,
    plain: Internal,
    extract: Internal,
    /// `None` when the shader failed to build: the page is drawn plain.
    retro: Option<Retro>,
    target: Option<Target>,
    /// Whether the shader samples the target bilinearly.
    linear: bool,
    frame_count: i32,
}

impl Passes {
    /// The internal passes plus shader `id`; `None` only when an internal
    /// pass fails.
    unsafe fn build(gl: &glow::Context, id: &str) -> Option<Self> {
        let version = egui_glow::ShaderVersion::get(gl);
        let prefix = format!(
            "{}\n#define NEW_SHADER_INTERFACE {}\n",
            version.version_declaration(),
            u8::from(version.is_new_shader_interface())
        );
        let internal =
            |fragment: &str, name: &str| match unsafe { Internal::build(gl, &prefix, fragment) } {
                Ok(pass) => Some(pass),
                Err(err) => {
                    log::error!("page shader pass `{name}`: {err}");
                    None
                }
            };
        let plain = internal(shaders::PLAIN, "plain")?;
        let extract = internal(shaders::EXTRACT, "extract")?;
        let source = shaders::source(id);
        let linear = source.as_ref().is_some_and(|s| s.linear);
        let retro = match source {
            Some(source) => {
                match unsafe { Retro::build(gl, version.version_declaration(), &source.text) } {
                    Ok(retro) => {
                        log::info!("page shader `{id}`: built");
                        Some(retro)
                    }
                    Err(err) => {
                        log::error!("page shader `{id}`: {err}; drawing the page plain");
                        None
                    }
                }
            }
            None => {
                log::error!("page shader `{id}`: no such shader; drawing the page plain");
                None
            }
        };
        unsafe {
            let vbo = gl.create_buffer().ok()?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let bytes: Vec<u8> = QUAD.iter().flat_map(|v| v.to_ne_bytes()).collect();
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STATIC_DRAW);
            // GL 3 core draws nothing without a bound VAO; GLES 2 has none.
            let vao = match gl.version().major >= 3 {
                true => {
                    let vao = gl.create_vertex_array().ok()?;
                    gl.bind_vertex_array(Some(vao));
                    bind_quad(gl, vbo);
                    gl.bind_vertex_array(None);
                    Some(vao)
                }
                false => None,
            };
            Some(Self {
                vbo,
                vao,
                plain,
                extract,
                retro,
                target: None,
                linear,
                frame_count: 0,
            })
        }
    }

    /// Draw `frame` into the viewport egui set, whose bottom-left corner is `origin`.
    unsafe fn draw(
        &mut self,
        gl: &glow::Context,
        page: glow::NativeTexture,
        frame: &Frame,
        origin: (i32, i32),
    ) {
        unsafe {
            match self.vao {
                Some(vao) => gl.bind_vertex_array(Some(vao)),
                None => bind_quad(gl, self.vbo),
            }
            gl.active_texture(glow::TEXTURE0);
            if let Some(retro) = self.retro {
                if let Some(target) = self.extract(gl, page, frame, origin) {
                    self.plain.draw(gl, page, frame);
                    let [x, y, w, h] = frame.source_rect;
                    gl.viewport(
                        origin.0 + x.round() as i32,
                        origin.1 + y.round() as i32,
                        w.round() as i32,
                        h.round() as i32,
                    );
                    retro.draw(gl, target, frame, self.frame_count);
                    self.frame_count = self.frame_count.wrapping_add(1);
                }
            } else {
                self.plain.draw(gl, page, frame);
            }
            if self.vao.is_some() {
                gl.bind_vertex_array(None);
            }
        }
    }

    /// Render the game at its own resolution into the target, then put back
    /// egui's framebuffer, viewport, scissor and blending. Returns the texture.
    unsafe fn extract(
        &mut self,
        gl: &glow::Context,
        page: glow::NativeTexture,
        frame: &Frame,
        origin: (i32, i32),
    ) -> Option<glow::Texture> {
        let size = (
            frame.source_size.0.round().max(1.0) as i32,
            frame.source_size.1.round().max(1.0) as i32,
        );
        unsafe {
            if self.target.as_ref().is_none_or(|t| t.size != size) {
                if let Some(old) = self.target.take() {
                    old.delete(gl);
                }
                self.target = Some(Target::new(gl, size, self.linear)?);
            }
            let target = self.target.as_ref()?;
            let restore_fbo = gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
            let scissor = gl.is_enabled(glow::SCISSOR_TEST);
            let blend = gl.is_enabled(glow::BLEND);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.fbo));
            gl.viewport(0, 0, size.0, size.1);
            self.extract.draw(gl, page, frame);
            gl.bind_framebuffer(glow::FRAMEBUFFER, restore_fbo);
            let (w, h) = frame.size;
            gl.viewport(origin.0, origin.1, w as i32, h as i32);
            if scissor {
                gl.enable(glow::SCISSOR_TEST);
            }
            if blend {
                gl.enable(glow::BLEND);
            }
            Some(target.texture)
        }
    }

    unsafe fn delete(self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.plain.program);
            gl.delete_program(self.extract.program);
            if let Some(retro) = self.retro {
                gl.delete_program(retro.program);
            }
            if let Some(target) = self.target {
                target.delete(gl);
            }
            gl.delete_buffer(self.vbo);
            if let Some(vao) = self.vao {
                gl.delete_vertex_array(vao);
            }
        }
    }
}

impl Internal {
    unsafe fn build(gl: &glow::Context, prefix: &str, fragment: &str) -> Result<Self, String> {
        let vertex = format!("{prefix}{}", shaders::VERTEX);
        let fragment = format!("{prefix}{}{fragment}", shaders::HEADER);
        unsafe {
            let program = link(gl, &vertex, &fragment, &[(A_POS, "a_pos")])?;
            Ok(Self {
                program,
                u_page: gl.get_uniform_location(program, "u_page"),
                u_size: gl.get_uniform_location(program, "u_size"),
                u_source_size: gl.get_uniform_location(program, "u_source_size"),
                u_source_rect: gl.get_uniform_location(program, "u_source_rect"),
            })
        }
    }

    unsafe fn draw(&self, gl: &glow::Context, page: glow::NativeTexture, frame: &Frame) {
        unsafe {
            gl.use_program(Some(self.program));
            gl.bind_texture(glow::TEXTURE_2D, Some(page));
            gl.uniform_1_i32(self.u_page.as_ref(), 0);
            gl.uniform_2_f32(self.u_size.as_ref(), frame.size.0, frame.size.1);
            let (sw, sh) = frame.source_size;
            gl.uniform_2_f32(self.u_source_size.as_ref(), sw, sh);
            let [x, y, w, h] = frame.source_rect;
            gl.uniform_4_f32(self.u_source_rect.as_ref(), x, y, w, h);
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
        }
    }
}

impl Retro {
    /// Compile `text` once per stage under `#define VERTEX` / `FRAGMENT`, trying
    /// the file's own `#version`, the legacy GLSL most RetroArch shaders are
    /// written in, then `fallback`: a desktop version means nothing to GLES.
    unsafe fn build(gl: &glow::Context, fallback: &str, text: &str) -> Result<Self, String> {
        let own = text
            .lines()
            .find(|l| l.trim_start().starts_with("#version"));
        let body: String = match own {
            // Commented out rather than removed, so error line numbers hold.
            Some(_) => text
                .lines()
                .map(|l| match l.trim_start().starts_with("#version") {
                    true => format!("// {l}\n"),
                    false => format!("{l}\n"),
                })
                .collect(),
            None => text.to_string(),
        };
        let legacy = match fallback.contains(" es") {
            true => "#version 100",
            false => "#version 120",
        };
        let versions = own.map(str::trim).into_iter().chain([legacy, fallback]);
        let mut last = String::new();
        for version in versions {
            let stage = |name: &str| format!("{version}\n#define {name}\n{GLES_PRECISION}{body}");
            let linked = unsafe {
                link(
                    gl,
                    &stage("VERTEX"),
                    &stage("FRAGMENT"),
                    &[(A_POS, "VertexCoord"), (A_TEX, "TexCoord")],
                )
            };
            match linked {
                Ok(program) => unsafe {
                    let at = |name| gl.get_uniform_location(program, name);
                    return Ok(Self {
                        program,
                        mvp: at("MVPMatrix"),
                        texture: at("Texture"),
                        texture_size: at("TextureSize"),
                        input_size: at("InputSize"),
                        output_size: at("OutputSize"),
                        frame_count: at("FrameCount"),
                        frame_direction: at("FrameDirection"),
                    });
                },
                Err(err) => last = err,
            }
        }
        Err(last)
    }

    /// Draw over the current viewport, which is the game's rect.
    unsafe fn draw(&self, gl: &glow::Context, source: glow::Texture, frame: &Frame, count: i32) {
        let [.., w, h] = frame.source_rect;
        let (sw, sh) = frame.source_size;
        unsafe {
            gl.use_program(Some(self.program));
            gl.bind_texture(glow::TEXTURE_2D, Some(source));
            gl.uniform_matrix_4_f32_slice(self.mvp.as_ref(), false, &MVP);
            gl.uniform_1_i32(self.texture.as_ref(), 0);
            gl.uniform_2_f32(self.texture_size.as_ref(), sw.round(), sh.round());
            gl.uniform_2_f32(self.input_size.as_ref(), sw.round(), sh.round());
            gl.uniform_2_f32(self.output_size.as_ref(), w, h);
            gl.uniform_1_i32(self.frame_count.as_ref(), count);
            gl.uniform_1_i32(self.frame_direction.as_ref(), 1);
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
        }
    }
}

impl Target {
    unsafe fn new(gl: &glow::Context, (w, h): (i32, i32), linear: bool) -> Option<Self> {
        unsafe {
            let texture = gl.create_texture().ok()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                w,
                h,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            // Nearest unless asked, as RetroArch samples with smoothing off;
            // clamping keeps a non-power-of-two texture legal on GLES 2.
            let filter = match linear {
                true => glow::LINEAR,
                false => glow::NEAREST,
            };
            for (name, value) in [
                (glow::TEXTURE_MIN_FILTER, filter),
                (glow::TEXTURE_MAG_FILTER, filter),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, name, value as i32);
            }
            let fbo = gl.create_framebuffer().ok()?;
            let restore = gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            let complete =
                gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
            gl.bind_framebuffer(glow::FRAMEBUFFER, restore);
            if !complete {
                log::error!("page shader: {w}x{h} target framebuffer incomplete");
                gl.delete_framebuffer(fbo);
                gl.delete_texture(texture);
                return None;
            }
            Some(Self {
                fbo,
                texture,
                size: (w, h),
            })
        }
    }

    unsafe fn delete(self, gl: &glow::Context) {
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.texture);
        }
    }
}

/// Point both attribute slots at the quad's buffer.
unsafe fn bind_quad(gl: &glow::Context, vbo: glow::Buffer) {
    unsafe {
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        for slot in [A_POS, A_TEX] {
            gl.vertex_attrib_pointer_f32(slot, 2, glow::FLOAT, false, 0, 0);
            gl.enable_vertex_attrib_array(slot);
        }
    }
}

/// Compile both stages and link them, each attribute bound to its slot.
unsafe fn link(
    gl: &glow::Context,
    vertex: &str,
    fragment: &str,
    attributes: &[(u32, &str)],
) -> Result<glow::Program, String> {
    unsafe {
        let program = gl.create_program()?;
        let mut stages = Vec::new();
        for (kind, src) in [
            (glow::VERTEX_SHADER, vertex),
            (glow::FRAGMENT_SHADER, fragment),
        ] {
            let stage = gl.create_shader(kind)?;
            gl.shader_source(stage, src);
            gl.compile_shader(stage);
            if !gl.get_shader_compile_status(stage) {
                let log = gl.get_shader_info_log(stage);
                gl.delete_shader(stage);
                for stage in stages {
                    gl.delete_shader(stage);
                }
                gl.delete_program(program);
                return Err(log);
            }
            gl.attach_shader(program, stage);
            stages.push(stage);
        }
        for &(slot, name) in attributes {
            gl.bind_attrib_location(program, slot, name);
        }
        gl.link_program(program);
        for stage in stages {
            gl.detach_shader(program, stage);
            gl.delete_shader(stage);
        }
        match gl.get_program_link_status(program) {
            true => Ok(program),
            false => {
                let log = gl.get_program_info_log(program);
                gl.delete_program(program);
                Err(log)
            }
        }
    }
}
