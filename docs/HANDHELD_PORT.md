# Handheld Port (Knulli / muOS / ROCKNIX)

Status of porting retsurf to run as a PortMaster port on aarch64 handhelds.

## Goal & target

Run retsurf on PortMaster-capable CFWs:

- **Knulli** (Batocera-based), **muOS**, **ROCKNIX**
- aarch64, **bare-kmsdrm** display (no X11/Wayland compositor by default)
- **Mali-G31 / G52** (RK3326 / RK3566), which expose **OpenGL ES 3.2**

Agreed approach: **Path A first (software render), then Path B (GPU-accelerated).**

## Key findings (the constraints)

- Servo/WebRender + egui run fine on **OpenGL ES 3.0+**. WebRender *requires* GLES 3.0
  (instancing, MRT, integer attributes, …).
- **gl4es is a dead end** — it only emulates up to GL 2.x. The port must use the
  device's **native Mali GLES blob** (Mali-G31/G52 give GLES 3.2, which is enough).
- Servo's `RenderingContext` auto-selects GLES 3.0 when surfman reports `GLApi::GLES`.
  surfman's wayland backend honors `SURFMAN_FORCE_GLES=1`; its pure-EGL backend is
  GLES-native; its x11 backend is always desktop GL.
- **Blocker on bare-kmsdrm:** the `sdl2` crate 0.38 exposes **no DRM/GBM raw-window-handle**
  (only Wayland/Xlib/Win32/…). So surfman cannot create its own context from SDL's window
  handle on kmsdrm. Therefore **SDL2 must own the GL context** (it does this via EGL/GBM,
  like every other SDL2 port) and Servo renders *into* it.

## Architecture

```
            ┌──────────────────────── retsurf process ────────────────────────┐
            │                                                                  │
 SDL2  ─────┼──► window + GLES context (EGL/GBM on device)                     │
            │        │                                                         │
            │        ▼                                                         │
 egui  ─────┼──► glow over SDL2's GLES context ──► composites toolbar + page   │
            │        ▲                                  │                       │
            │        │ (browser frame as a texture)     ▼                       │
            │   ┌────┴─────────┐                 SDL2 swap → screen            │
 Servo ─────┼──►│ render target │                                              │
            │   └───────────────┘                                              │
            └──────────────────────────────────────────────────────────────────┘

Path A (done):    Servo render target = SoftwareRenderingContext (offscreen, llvmpipe).
                  Each frame: read_to_image() → upload as egui texture → composite.

Path B (current): Servo render target = an FBO in SDL2's own GL context, via a custom
                  `RenderingContext` impl (src/render.rs). egui draws that FBO's color
                  texture directly. Zero CPU readback, GPU-accelerated, single GL
                  context, no surfman software adapter / llvmpipe.
```

> **Path B turned out simpler than the original "adopt SDL's context via surfman"
> plan.** Since SDL2 owns the only GL context, we implement `servo::RenderingContext`
> ourselves over that context + a self-managed FBO. WebRender renders into whatever
> framebuffer is bound after `prepare_for_rendering`, so we just bind our FBO. No
> surfman context adoption needed.

## Done ✅

Both rendering paths implemented and verified on desktop at **OpenGL ES 3.2** (Mesa),
0 GL errors, page renders correctly (right-side-up) and composites with the toolbar.
**Path B is the current/default implementation**; Path A was the stepping-stone.

### Path B — GPU, shared context (current)

Servo renders into an FBO in SDL2's own GLES context; egui draws that FBO's texture.
No surfman software adapter, no CPU readback, single GL context.

| File | Change |
|------|--------|
| `src/render.rs` *(new)* | `SdlRenderingContext`: implements `servo::RenderingContext` over SDL2's GL context + a self-managed FBO (color texture + depth renderbuffer). `prepare_for_rendering` binds the FBO; `read_to_image` via `glReadPixels`; `resize` reallocates; `connection()` returns a surfman `Connection` (Servo requires it for WebGL); exposes the color texture for egui. |
| `src/window.rs`  | SDL2 owns the GL/GLES context; builds `glow` + `gleam` GL from SDL's proc loader and constructs the `SdlRenderingContext`; exposes it + its color texture; `bind_default_framebuffer`; `present` via `gl_swap_window`. |
| `src/browser.rs` | Takes the shared `Rc<dyn RenderingContext>`; `resize()` resizes the context + webview. |
| `src/ui.rs`      | Registers the FBO color texture once (`register_native_texture`) and draws it (V-flipped) in the central panel; drives browser viewport size from the central rect. |
| `src/app.rs`     | Loop: `browser.paint()` (Servo → FBO) → `ui.update` → `ui.draw` (egui composites + present). Resizes reactive. `process::exit(0)` on shutdown. |
| `src/config.rs`  | `InterfaceConfig.use_gles` toggle. |
| `src/main.rs`    | `mod render`; `RETSURF_GLES=0/1` override; auto-sets `SURFMAN_FORCE_GLES=1` when GLES is on; aligns SDL to the Wayland driver on a Wayland desktop (see pitfall 4). |
| `Cargo.toml`     | Added `gleam`, `glow`, `surfman` direct deps. |

### Path A — software render (prior milestone)

`SoftwareRenderingContext` (offscreen llvmpipe) + per-frame `read_to_image()` → egui
texture upload. Kept in git history as `c2c5059`; superseded by Path B because it needs
llvmpipe on the device and does a CPU copy every frame.

### Pitfalls hit during Path A (two-GL-context era)

These came up while Path A ran SDL's context *and* surfman's context in one thread.
Path B uses a single context, so #2 no longer applies and #1/#3 are precautionary;
**#4 still applies** because `connection()` still calls `surfman::Connection::new()`.

1. **eglBindAPI clash** — SDL's GLES context vs surfman's desktop-GL software context
   caused a startup panic. Fixed by forcing `SURFMAN_FORCE_GLES=1` so both stacks are GLES.
2. **SDL make-current cache** — surfman changed the thread's current EGL context behind
   SDL's back, so SDL skipped the real `eglMakeCurrent` and egui drew into an *undefined*
   framebuffer (`GL_FRAMEBUFFER_UNDEFINED`, thousands of `GL_INVALID_FRAMEBUFFER_OPERATION`).
   Fixed by clearing SDL's cache (`SDL_GL_MakeCurrent(window, NULL)`) before rebinding.
3. **Teardown panic** — Servo's `SoftwareRenderingContext` doesn't destroy its surfman
   context on drop; `process::exit(0)` skips the bad destructor.
4. **SDL/surfman on different display servers** — surfman picks its backend from the
   environment (Wayland when `WAYLAND_DISPLAY` is set), independent of SDL. On a Wayland
   desktop SDL still often defaults to **x11**, so the two GL stacks land on different
   display servers and surfman's context creation fails → startup panic
   (`Contexts must be destroyed explicitly`). Symptom: plain `cargo run` panics while
   `SDL_VIDEODRIVER=wayland cargo run` works. Fixed in `main.rs`: when `WAYLAND_DISPLAY`
   is set and `SDL_VIDEODRIVER` is unset, force SDL to the `wayland` driver so both agree.
   On the handheld (no `WAYLAND_DISPLAY`) this is skipped and SDL uses kmsdrm as intended;
   an explicit `SDL_VIDEODRIVER` always wins.

### Run / verify

```sh
# Desktop: just works now (no env vars needed) — auto-selects Wayland + GLES.
cargo run
# Desktop-GL fallback for debugging:
RETSURF_GLES=0 cargo run
# Force a specific SDL backend (overrides the auto-alignment):
SDL_VIDEODRIVER=wayland cargo run
```

## Remaining

### 1. Path B — GPU-accelerated (real target) ✅ done on desktop
- [x] Custom `RenderingContext` over SDL2's GL context + FBO (`src/render.rs`).
- [x] Servo renders into the FBO; egui composites the texture; no CPU readback.
- [x] Verified on desktop at GLES 3.2, 0 GL errors.
- [ ] **Verify on device** — needs the aarch64 build (below); first device run surfaced
      the EGL 1.4 issue (now fixed).

### EGL 1.4 vs surfman — the device blocker (fixed)

First on-device run (Knulli, Mali) panicked: `surfman .../egl_bindings.rs: egl function
was not loaded`. Root cause: **surfman 0.12 requires `eglGetPlatformDisplay` (EGL 1.5)**,
loaded via `dlsym`, on *every* Linux backend (wayland/x11/surfaceless). The device's Mali
blob is **EGL 1.4** (`libEGL.so.1.4.0`, a ~6 KB dispatch stub), so that symbol is absent.
Servo's `register_rendering_context` hard-`expect()`s a surfman `Connection`, but that
connection is only ever used for **WebGL/WebGPU** external images.

Fix (two parts):
- `src/render.rs`: `connection()` is now optional — `surfman::Connection::new()` is wrapped
  in `catch_unwind` (surfman *panics* rather than returning `Err` on missing EGL symbols).
  Capable platforms (desktop, EGL 1.5) keep a real connection **and WebGL**; EGL 1.4 devices
  get `None`.
- `vendor/servo-paint/paint.rs` (vendored + `[patch.crates-io]`): `register_rendering_context`
  treats the connection as optional instead of `.expect()`-ing it. **WebGL is disabled when
  the connection is absent; all other rendering is unaffected.**

Revisit later for WebGL on EGL 1.4: a surfman patch to fall back to `eglGetDisplay`
(EGL 1.0) / wrap SDL's current EGL display.

### 2. Build for aarch64 — via the official PortMaster Docker builder (qemu)

Decision: build inside PortMaster's prebuilt **aarch64 builder image** under qemu
emulation (no hand-rolled sysroot; the image ships the recommended toolchain/libs/SDL2
with broad-compatibility glibc). Ref: <https://portmaster.games/docker.html>

```bash
# one-time: register qemu binfmt so arm64 containers run on x86
docker run --rm --privileged multiarch/qemu-user-static --reset -p yes

docker pull --platform=linux/arm64 \
  ghcr.io/monkeyx-net/portmaster-build-templates/portmaster-builder:aarch64-latest

docker run -it --name builder_aarch64 -v "$(pwd)":/workspace --platform=linux/arm64 \
  ghcr.io/monkeyx-net/portmaster-build-templates/portmaster-builder:aarch64-latest
```

- [ ] Inside the container: install Rust (rustup) + Servo's native build deps
      (clang, cmake, python3, gperf, libssl/dbus/freetype/harfbuzz/glib/udev dev, etc.)
      — likely a small setup script layered on top of the PM image, since Servo needs
      far more than a typical C/SDL port (esp. `mozjs_sys` / `mozangle`).
- [ ] `cargo build --release` runs as a **native arm64 build under qemu** → slow
      (SpiderMonkey/ANGLE are the long poles); expect a long first build.
- [ ] Confirm the PM image's SDL2 has the **kmsdrm** video driver (it targets handhelds,
      so it should).
- [ ] `libGLESv2`/`libEGL` (Mali blob) resolve at runtime on device — don't bundle them.

### 3. Input — gamepad ✅ in-app (needs on-device tuning)
Implemented in `src/event/gamepad.rs` (SDL GameController), no gptokeyb needed:
- Left stick / D-pad → cursor · **A** → click · right stick → scroll
- **B** / **L** → back · **R** → forward · **Start** → reload
- [ ] Tune deadzone / cursor speed / scroll speed on real hardware.
- [ ] Text entry (URL bar) still needs an on-screen keyboard or gptokeyb keymap.

### 4. PortMaster packaging
- [ ] Port directory + launcher `.sh` (sets `SDL_VIDEODRIVER=kmsdrm`, `RETSURF_GLES=1`,
      library paths, runs the binary).
- [ ] Pick runtime: bare kmsdrm vs a PortMaster runtime (e.g. WestonPack) per device.
- [ ] Test matrix: Knulli, muOS, ROCKNIX on RK3326 and RK3566.

### 5. Nice-to-have / cleanup
- [ ] Fix the first-frame black strip at the bottom (viewport settles after one resize).
- [ ] Silence harmless DuckDuckGo CSS-parse warnings in logs (or lower log level).
- [ ] Quiet the ClientStorage sqlite warning (point storage at a writable dir).

## Open decisions
- Path B context-adoption details need on-device testing (can't be verified on an
  x86/desktop box).
- Whether to ship the llvmpipe-based Path A as a fallback for GPU-less/edge devices.
