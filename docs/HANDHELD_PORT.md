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

Path A (now):  Servo render target = SoftwareRenderingContext (offscreen, llvmpipe).
               Each frame: read_to_image() → upload as egui texture → composite.

Path B (next): Servo render target = surfman context that ADOPTS SDL2's hardware
               GLES context (create_context_from_native_context). Zero readback,
               GPU-accelerated.
```

## Done — Path A (software render) ✅

Implemented and verified on desktop running on **OpenGL ES 3.2** (Mesa), 0 GL errors,
page renders correctly and composites with the toolbar.

| File | Change |
|------|--------|
| `src/window.rs`  | SDL2 owns the GL/GLES context; egui `glow` context built from SDL2's proc loader; `make_current` clears SDL's stale cache before rebinding; `bind_default_framebuffer`; `present` via `gl_swap_window`. |
| `src/browser.rs` | Servo renders into `SoftwareRenderingContext`; added `read_image()` and a `resize()` that resizes both the context and the webview. |
| `src/ui.rs`      | egui uploads Servo's frame as a `TextureHandle` and draws it in the central panel; drives browser viewport size from the central rect. |
| `src/app.rs`     | New loop: render → read → composite → present. Resizes handled reactively. Clean `process::exit(0)` on shutdown. |
| `src/config.rs`  | `InterfaceConfig.use_gles` toggle. |
| `src/main.rs`    | `RETSURF_GLES=0/1` override; auto-sets `SURFMAN_FORCE_GLES=1` when GLES is on; aligns SDL to the Wayland driver on a Wayland desktop (see pitfall 4). |

### Bugs fixed along the way (two-GL-context pitfalls)

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

### Path A caveat for the device

`SoftwareRenderingContext` needs a Mesa **software adapter (llvmpipe)** present and is
**CPU-rendered (slow)**. OK on Knulli (Batocera ships Mesa); **may be stripped on some
muOS/ROCKNIX images**. This is the motivation for Path B.

## Remaining

### 1. Path B — GPU-accelerated (real target)
- [ ] SDL2 creates the GLES context on the device (kmsdrm/EGL). ✅ already the case.
- [ ] Grab the current EGL context/surface/display (`eglGetCurrent*`).
- [ ] Feed them to surfman via `create_context_from_native_context` (confirmed to exist).
- [ ] Implement Servo's `RenderingContext` trait over that adopted context (custom impl
      in retsurf, since the public `WindowRenderingContext` won't take a pre-made context).
- [ ] Remove the per-frame readback/texture-upload; render Servo straight into the shared FB.

### 2. Cross-compile for aarch64
- [ ] Target `aarch64-unknown-linux-gnu` against an **old glibc** (the CFWs ship ~2.3x),
      mirroring the existing x86 `Dockerfile` but for ARM (sysroot or container).
- [ ] Vendor/link the device's `libGLESv2`/`libEGL` (Mali blob) — link against stubs,
      resolve at runtime on device.
- [ ] Confirm SDL2 is built with the **kmsdrm** video driver on each target.

### 3. PortMaster packaging
- [ ] Port directory + launcher `.sh` (sets `SDL_VIDEODRIVER=kmsdrm`, `RETSURF_GLES=1`,
      library paths, runs the binary).
- [ ] Controls via `gptokeyb` (map handheld buttons → mouse/keys/scroll).
- [ ] Pick runtime: bare kmsdrm vs a PortMaster runtime (e.g. WestonPack) per device.
- [ ] Test matrix: Knulli, muOS, ROCKNIX on RK3326 and RK3566.

### 4. Nice-to-have / cleanup
- [ ] Fix the first-frame black strip at the bottom (viewport settles after one resize).
- [ ] Silence harmless DuckDuckGo CSS-parse warnings in logs (or lower log level).
- [ ] Quiet the ClientStorage sqlite warning (point storage at a writable dir).

## Open decisions
- Path B context-adoption details need on-device testing (can't be verified on an
  x86/desktop box).
- Whether to ship the llvmpipe-based Path A as a fallback for GPU-less/edge devices.
