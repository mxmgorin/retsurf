# Rendering

How a page reaches the screen: the GPU path over SDL2's GL context, what WebGL needs from
it, and the software path for devices without a GPU.

## Why SDL owns the context

WebRender and egui need OpenGL ES 3.0 or later, so a handheld uses its native GLES blob
(gl4es stops at GL 2.x). The `sdl2` crate exposes a raw window handle only for Wayland,
Xlib, Win32 and the like, not for DRM/GBM or a firmware's own video backend, so surfman
cannot open a context on SDL's window. SDL2 owns the GL context and Servo renders into it.

## How it works

Servo renders each page into an offscreen framebuffer (FBO) in SDL's context, through a
custom `RenderingContext`; egui composites the FBO's texture with the chrome and SDL
presents it. One context, no compositor, no CPU readback.

| File | Role |
|------|------|
| `src/platform/render/sdl.rs` | `SdlRenderingContext`: the FBO, resize, readback, and the surfman `Connection` for WebGL (`None` where SDL is not on EGL) |
| `src/platform/window/` | the SDL GL context, GL loaders, the FBO texture registered with egui, presenting |
| `src/ui/mod.rs` | draws the FBO texture and sizes the browser viewport from it |
| `src/app/mod.rs` | the loop: Servo paints, then egui composites and presents |
| `src/platform/startup.rs` | `RETSURF_GLES`, `SURFMAN_FORCE_GLES`, the Wayland alignment |

### Sharing the context with surfman

WebGL goes through surfman, which wraps SDL's context and display
(`src/platform/render/webgl.rs`). The two have to agree:

- **API**: `SURFMAN_FORCE_GLES=1` whenever GLES is on, since SDL binds the ES API.
- **Display server**: surfman picks Wayland when `WAYLAND_DISPLAY` is set, so SDL is forced
  to the wayland driver then, unless `SDL_VIDEODRIVER` says otherwise.
- **Current context**: Servo leaves its own current, so the window rebinds SDL's context and
  the default framebuffer before egui draws.

### EGL 1.4 and the surfman connection

surfman's `Connection::new()` needs `eglGetPlatformDisplay` from EGL 1.5, which the Mali
blobs (EGL 1.4) lack. retsurf builds the connection from SDL's own `EGLDisplay` instead, and
Servo treats a missing connection as no WebGL rather than a panic (servo/servo#47803). With
the surfman fork's GLES config fix, WebGL works on EGL 1.4 Mali, so every aarch64 build has
the `webgl` feature.

### EGL 1.4 on PowerVR: missing entry points

On the TrimUI Smart Pro (PowerVR, EGL 1.4) two lookups come back empty:

- SDL looks up `egl*` functions only in the GLES library, which on PowerVR has none, so
  `src/platform/render/webgl.rs` falls back to `libEGL`.
- surfman loads GL only through `eglGetProcAddress`, which EGL 1.4 need not answer for core
  functions, so the surfman fork falls back to `libGLESv2`.

## Rendering without a GPU

The `software` feature (the armhf build) needs no GL driver:

- [swgl](https://crates.io/crates/swgl), WebRender's software backend, rasterizes the page.
  It must match our `webrender` version, which picks its software paths by renderer name.
- SDL's 2D renderer draws the chrome over the page into one surface, presented as a single
  texture copy, the only path the Miyoo's `mmiyoo` driver shows.

swgl's framebuffer is already SDL's `ARGB8888` layout, so the page is copied row by row,
flipped. With no vsync, the loop caps at `[performance] max_fps`.

`[display] software_render` or `RETSURF_SOFTWARE=1` forces it, and a build with the feature
falls back to it when no GL context can be created. `[debug] frame_timing` logs the frame
cost.

## Choosing the path at runtime

```sh
cargo run                            # GLES, on Wayland when available
RETSURF_GLES=0 cargo run             # desktop GL, for debugging
SDL_VIDEODRIVER=x11 cargo run        # force an SDL backend
RETSURF_SOFTWARE=1 cargo run         # CPU rendering (software builds)
```
