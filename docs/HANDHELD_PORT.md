# Handheld Port (Knulli / muOS / ROCKNIX)

Notes on running retsurf as a PortMaster port on aarch64 handhelds.

## Goal and target

Run retsurf on PortMaster-capable custom firmwares:

- Knulli (Batocera-based), muOS, and ROCKNIX
- aarch64, with a bare kmsdrm display (no X11 or Wayland compositor by default)
- Mali-G31 / G52 GPUs (RK3326 / RK3566), which expose OpenGL ES 3.2

The approach was to get a software renderer working first (Path A), then move to GPU
acceleration (Path B).

## Constraints we ran into

Servo/WebRender and egui both run on OpenGL ES 3.0 and up. WebRender needs at least
GLES 3.0 for instancing, MRT, integer attributes, and so on.

gl4es is a dead end here: it only emulates up to GL 2.x. The port has to use the
device's native Mali GLES blob, and Mali-G31/G52 give us GLES 3.2, which is enough.

Servo's `RenderingContext` auto-selects GLES 3.0 when surfman reports `GLApi::GLES`. The
wayland backend honors `SURFMAN_FORCE_GLES=1`, the pure-EGL backend is GLES-native, and
the x11 backend is always desktop GL.

The real blocker on bare kmsdrm is that the `sdl2` crate (0.38) exposes no DRM/GBM
raw-window-handle, only Wayland/Xlib/Win32 and friends. So surfman can't create its own
context from SDL's window handle on kmsdrm. That means SDL2 has to own the GL context
itself (it does this over EGL/GBM, like every other SDL2 port) and Servo renders into it.

## How it works

SDL2 owns the window and the single GL/GLES context. Servo renders each page into an
offscreen framebuffer (FBO) in that context, and egui then composites the page texture
with the toolbar and presents the frame through SDL2. Keeping everything on one GLES
context, with no compositor and no CPU readback, is what lets it run on bare handheld
hardware.

## Architecture

Path A (done): Servo's render target is a `SoftwareRenderingContext` (offscreen,
llvmpipe). Each frame calls `read_to_image()`, uploads the result as an egui texture, and
composites.

Path B (current): Servo's render target is an FBO in SDL2's own GL context, via a custom
`RenderingContext` impl in `src/platform/render.rs`. egui draws that FBO's color texture
directly. No CPU readback, GPU-accelerated, a single GL context, and no surfman software
adapter or llvmpipe.

Path B ended up simpler than the original "adopt SDL's context via surfman" plan. Since
SDL2 owns the only GL context, we just implement `servo::RenderingContext` ourselves over
that context plus a self-managed FBO. WebRender renders into whatever framebuffer is bound
after `prepare_for_rendering`, so we bind our FBO and that's it. No surfman context
adoption needed.

## Rendering paths

Both rendering paths are implemented and verified on desktop at OpenGL ES 3.2 (Mesa):
0 GL errors, the page renders right-side-up, and it composites with the toolbar. Path B
is the current default; Path A was the stepping stone. Path B is also verified on device
(Knulli, Mali, EGL 1.4) after the surfman-optional fix described below, and a native arm64
GHA build produces a working binary.

### Path B: GPU, shared context (current)

Servo renders into an FBO in SDL2's own GLES context, and egui draws that FBO's texture.
No surfman software adapter, no CPU readback, one GL context.

| File | Change |
|------|--------|
| `src/platform/render.rs` *(new)* | `SdlRenderingContext`: implements `servo::RenderingContext` over SDL2's GL context + a self-managed FBO (color texture + depth renderbuffer). `prepare_for_rendering` binds the FBO; `read_to_image` via `glReadPixels`; `resize` reallocates; `connection()` returns a surfman `Connection` (Servo requires it for WebGL); exposes the color texture for egui. |
| `src/platform/window.rs`  | SDL2 owns the GL/GLES context; builds `glow` + `gleam` GL from SDL's proc loader and constructs the `SdlRenderingContext`; exposes it + its color texture; `bind_default_framebuffer`; `present` via `gl_swap_window`. |
| `src/browser.rs` | Takes the shared `Rc<dyn RenderingContext>`; `resize()` resizes the context + webview. |
| `src/ui.rs`      | Registers the FBO color texture once (`register_native_texture`) and draws it (V-flipped) in the central panel; drives browser viewport size from the central rect. |
| `src/app.rs`     | Loop: `browser.paint()` (Servo to FBO), then `ui.update`, then `ui.draw` (egui composites and presents). Resizes reactive. `process::exit(0)` on shutdown. |
| `src/config.rs`  | `InterfaceConfig.use_gles` toggle. |
| `src/main.rs`    | `mod render`; `RETSURF_GLES=0/1` override; auto-sets `SURFMAN_FORCE_GLES=1` when GLES is on; aligns SDL to the Wayland driver on a Wayland desktop (see pitfall 4). |
| `Cargo.toml`     | Added `gleam`, `glow`, `surfman` direct deps. |

### Path A: software render (earlier milestone)

`SoftwareRenderingContext` (offscreen llvmpipe) plus a per-frame `read_to_image()` and
egui texture upload. It's kept in git history as `c2c5059`; Path B superseded it because
it needs llvmpipe on the device and does a CPU copy every frame.

### Pitfalls during Path A (the two-GL-context era)

These showed up while Path A ran SDL's context and surfman's context together in one
thread. Path B uses a single context, so #2 no longer applies and #1 and #3 are
precautionary. #4 still applies, because `connection()` still calls
`surfman::Connection::new()`.

1. eglBindAPI clash. SDL's GLES context versus surfman's desktop-GL software context
   caused a startup panic. Fixed by forcing `SURFMAN_FORCE_GLES=1` so both stacks are GLES.
2. SDL make-current cache. surfman changed the thread's current EGL context behind SDL's
   back, so SDL skipped the real `eglMakeCurrent` and egui drew into an undefined
   framebuffer (`GL_FRAMEBUFFER_UNDEFINED`, thousands of
   `GL_INVALID_FRAMEBUFFER_OPERATION`). Fixed by clearing SDL's cache with
   `SDL_GL_MakeCurrent(window, NULL)` before rebinding.
3. Teardown panic. Servo's `SoftwareRenderingContext` doesn't destroy its surfman context
   on drop; `process::exit(0)` skips the bad destructor.
4. SDL and surfman on different display servers. surfman picks its backend from the
   environment (Wayland when `WAYLAND_DISPLAY` is set), independent of SDL. On a Wayland
   desktop SDL still often defaults to x11, so the two GL stacks land on different display
   servers and surfman's context creation fails with a startup panic (`Contexts must be
   destroyed explicitly`). The symptom is that plain `cargo run` panics while
   `SDL_VIDEODRIVER=wayland cargo run` works. Fixed in `main.rs`: when `WAYLAND_DISPLAY`
   is set and `SDL_VIDEODRIVER` is unset, force SDL to the wayland driver so the two agree.
   On the handheld there's no `WAYLAND_DISPLAY`, so this is skipped and SDL uses kmsdrm as
   intended; an explicit `SDL_VIDEODRIVER` always wins.

### EGL 1.4 versus surfman: the device blocker (fixed)

The first on-device run (Knulli, Mali) panicked with `surfman .../egl_bindings.rs: egl
function was not loaded`. The root cause: surfman 0.12 requires `eglGetPlatformDisplay`
(EGL 1.5), loaded via `dlsym`, on every Linux backend (wayland, x11, surfaceless). The
device's Mali blob is EGL 1.4 (`libEGL.so.1.4.0`, a ~6 KB dispatch stub), so that symbol
just isn't there. Servo's `register_rendering_context` hard-`expect()`s a surfman
`Connection`, but that connection is only ever used for WebGL/WebGPU external images.

The fix has two parts:

- `src/platform/render.rs`: `connection()` is now optional. `surfman::Connection::new()`
  is wrapped in `catch_unwind`, since surfman panics rather than returning `Err` on
  missing EGL symbols. Capable platforms (desktop, EGL 1.5) keep a real connection and
  WebGL; EGL 1.4 devices get `None`.
- `vendor/servo-paint/paint.rs` (vendored via `[patch.crates-io]`):
  `register_rendering_context` treats the connection as optional instead of calling
  `.expect()`. WebGL is disabled when the connection is absent, but everything else
  renders fine.

WebGL on EGL 1.4 would need a surfman patch to fall back to `eglGetDisplay` (EGL 1.0), or
to wrap SDL's current EGL display.

## Running it

```sh
# Desktop: just works, no env vars needed (auto-selects Wayland + GLES).
cargo run
# Desktop-GL fallback for debugging:
RETSURF_GLES=0 cargo run
# Force a specific SDL backend (overrides the auto-alignment):
SDL_VIDEODRIVER=wayland cargo run
```

## Building for aarch64

The build runs inside PortMaster's prebuilt aarch64 builder image under qemu emulation,
rather than a hand-rolled sysroot. The image ships the recommended toolchain, libs, and
SDL2 with a broad-compatibility glibc. See <https://portmaster.games/docker.html>.

```bash
# one-time: register qemu binfmt so arm64 containers run on x86
docker run --rm --privileged multiarch/qemu-user-static --reset -p yes

docker pull --platform=linux/arm64 \
  ghcr.io/monkeyx-net/portmaster-build-templates/portmaster-builder:aarch64-latest

docker run -it --name builder_aarch64 -v "$(pwd)":/workspace --platform=linux/arm64 \
  ghcr.io/monkeyx-net/portmaster-build-templates/portmaster-builder:aarch64-latest
```

On top of the PM image the build needs Rust (rustup) and Servo's native build deps (clang,
cmake, python3, gperf, the libssl/dbus/freetype/harfbuzz/glib/udev dev packages, and so on),
which is more than a typical C/SDL port pulls in, especially `mozjs_sys` and `mozangle`.
`cargo build --release` runs as a native arm64 build under qemu, so the first build is slow,
with SpiderMonkey and ANGLE the long poles. `libGLESv2` and `libEGL` (the Mali blob) resolve
at runtime on the device, so they aren't bundled.

## Gamepad input

Implemented in `src/event/gamepad.rs` using SDL's GameController API, no gptokeyb needed:

- Left stick or D-pad moves the cursor, A clicks, the right stick scrolls.
- B or L go back, R goes forward, Start reloads.
- Y opens the on-screen keyboard (`src/overlay/osk.rs`); the D-pad selects, A types into
  the address bar (which also searches), Go loads, and B closes.

Events are drained per-frame with vsync so the cursor isn't laggy on device. The on-screen
keyboard currently targets the address bar only, not focused page fields.
