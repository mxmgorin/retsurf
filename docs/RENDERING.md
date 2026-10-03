# Rendering

How retsurf gets a page on screen, on every platform: the GPU path over SDL2's GL context,
what WebGL needs from it, and the software path for devices with no GPU.

## Why SDL owns the context

Servo/WebRender and egui both run on OpenGL ES 3.0 and up. WebRender needs at least
GLES 3.0 for instancing, MRT, integer attributes, and so on.

gl4es is a dead end: it only emulates up to GL 2.x. A handheld has to use its native GLES
blob, and the Mali-G31/G52 ones give GLES 3.2, which is enough.

Servo's `RenderingContext` auto-selects GLES 3.0 when surfman reports `GLApi::GLES`. The
wayland backend honors `SURFMAN_FORCE_GLES=1`, the pure-EGL backend is GLES-native, and
the x11 backend is always desktop GL.

The `sdl2` crate (0.38) exposes a raw-window-handle only for Wayland/Xlib/Win32 and
friends — nothing for DRM/GBM, and nothing for a firmware's own video backend, which is
what these devices actually run. So surfman can't create its own context from SDL's
window handle: SDL2 owns the GL context (over EGL, like every other SDL2 port) and Servo
renders into it.

## How it works

SDL2 owns the window and the single GL/GLES context. Servo renders each page into an
offscreen framebuffer (FBO) in that context, through a custom `RenderingContext` in
`src/platform/render/sdl.rs`, and egui then composites the FBO's color texture with the
chrome and presents the frame through SDL2. One GLES context, no compositor and no CPU
readback is what lets it run on bare handheld hardware.

WebRender renders into whatever framebuffer is bound after `prepare_for_rendering`, so
binding our FBO there is all it takes; surfman never has to adopt SDL's context for the
page.

| File | Role |
|------|------|
| `src/platform/render/sdl.rs` | `SdlRenderingContext`: implements `servo::RenderingContext` over SDL2's GL context + a self-managed FBO (color texture + depth renderbuffer). `prepare_for_rendering` binds the FBO; `read_to_image` via `glReadPixels`; `resize` reallocates; `connection()` returns the surfman `Connection` WebGL needs, or `None` where SDL is not on EGL. |
| `src/platform/window/` | SDL2 owns the GL/GLES context; builds `glow` + `gleam` GL from SDL's proc loader and constructs the `SdlRenderingContext`; registers its color texture with egui once (`register_native_texture`); `present` via `gl_swap_window`. |
| `src/browser/` | Takes the shared `Rc<dyn RenderingContext>`; `resize()` resizes the context + webview. |
| `src/ui/mod.rs` | Draws the FBO color texture (V-flipped) in the central panel; drives the browser viewport size from the central rect. |
| `src/app/mod.rs` | Loop: `browser.paint()` (Servo to FBO), then the UI pass (egui composites and presents). `process::exit(0)` on shutdown. |
| `src/config/display.rs` | `[display] use_gles` toggle. |
| `src/platform/startup.rs` | `RETSURF_GLES=0/1` override; sets `SURFMAN_FORCE_GLES=1` when GLES is on; aligns SDL to the Wayland driver on a Wayland desktop. |

### Sharing the context with surfman

WebGL still goes through surfman, which wraps SDL's context and display rather than
opening its own (`src/platform/render/webgl.rs`). The two GL stacks have to agree:

- **On the API.** SDL sets the thread's EGL API to ES, and a surfman context on desktop GL
  fails to create, so `SURFMAN_FORCE_GLES=1` is set whenever GLES is on.
- **On the display server.** surfman picks its backend from the environment (Wayland when
  `WAYLAND_DISPLAY` is set), independent of SDL, and on a Wayland desktop SDL often
  defaults to x11. When `WAYLAND_DISPLAY` is set and `SDL_VIDEODRIVER` is not,
  `src/platform/startup.rs` forces SDL to the wayland driver. Without `WAYLAND_DISPLAY`
  SDL takes the firmware's own backend; an explicit `SDL_VIDEODRIVER` always wins.
- **On what is current.** Servo leaves its context current and its FBO bound after a
  paint, so the window makes SDL's context current and binds the default framebuffer
  again before egui draws (`src/platform/window/gl.rs`).

### EGL 1.4 and the surfman connection

surfman's `Connection::new()` needs `eglGetPlatformDisplay`, an EGL 1.5 function, on every
Linux backend. The Mali blobs are EGL 1.4 (`libEGL.so.1.4.0`, a ~6 KB dispatch stub), so
it is not there, and the first on-device run panicked with `egl function was not loaded`.

So retsurf never calls it: `src/platform/render/webgl.rs` builds the connection from SDL's
own `EGLDisplay`, and where SDL is not on EGL, `connection()` is `None`. Servo treats a
missing connection as "no WebGL" (servo/servo#47803) rather than panicking. This carries
WebGL on EGL 1.4 too — measured on a Mali-G31 blob, with the surfman fork supplying the
GLES config bit — so every aarch64 build ships `webgl` (retsurf's own feature, which
enables `servo/webgl`); only the armhf/software targets turn it off, having no EGL at all.

### EGL 1.4 on PowerVR: missing entry points

The TrimUI Smart Pro (PowerVR, EGL 1.4) left two function lookups empty:

- SDL resolves `egl*` functions only from the GLES library, which on PowerVR exports none
  of them. `src/platform/render/webgl.rs` looks them up in `libEGL` when SDL returns null.
- surfman loads all of GL through `eglGetProcAddress`, which EGL 1.4 only has to answer
  for extensions. PowerVR returns null for core GL, and surfman panicked with
  `called glGetString but it was not loaded`. Our surfman fork looks them up in
  `libGLESv2` instead.

## Rendering without a GPU

The `software` cargo feature (on for the armhf build, off everywhere else) replaces both
renderers with CPU ones, so no GL driver is needed at all:

- the page is rasterized by [swgl](https://crates.io/crates/swgl), WebRender's own software
  backend — the same version as the `webrender` in our graph, because WebRender selects its
  software paths off the renderer name string rather than a build flag;
- the chrome is drawn by SDL's 2D renderer into an offscreen surface, over the page frame,
  and the composed frame reaches the panel as one texture copy — the only presentation path
  the Miyoo's `mmiyoo` driver shows.

swgl's `GL_RGBA8` framebuffer is BGRA in memory, which is SDL's `ARGB8888`, so the page
crosses into the composition surface as a plain row copy; only the row order is reversed,
because WebRender still draws bottom-up. There is no vsync on this path, so the main loop
caps itself at 30 fps.

`[display] software_render` (or `RETSURF_SOFTWARE=1`) forces it. A build that has the
feature also falls back to it on its own when no GL context can be created, so a device with
no driver lands there without being told to. `[debug] frame_timing` logs the per-frame cost,
split into the page and everything after it.

## Choosing the path at runtime

```sh
# Desktop: just works, no env vars needed (auto-selects Wayland + GLES).
cargo run
# Desktop-GL fallback for debugging:
RETSURF_GLES=0 cargo run
# Force a specific SDL backend (overrides the auto-alignment):
SDL_VIDEODRIVER=wayland cargo run
```
