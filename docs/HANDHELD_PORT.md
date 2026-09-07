# Handheld Port (Knulli / muOS / ROCKNIX)

Notes on running retsurf as a PortMaster port on aarch64 handhelds.

## Goal

Run retsurf on PortMaster-capable custom firmwares:

- Knulli (Batocera-based), muOS, and ROCKNIX
- aarch64, with a bare kmsdrm display (no X11 or Wayland compositor by default)
- Mali-G31 / G52 GPUs (RK3326 / RK3566), which expose OpenGL ES 3.2

The approach was to get a software renderer working first (Path A), then move to GPU
acceleration (Path B).

## Constraints

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
`RenderingContext` impl in `src/platform/render/sdl.rs`. egui draws that FBO's color texture
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
| `src/platform/render/sdl.rs` *(new)* | `SdlRenderingContext`: implements `servo::RenderingContext` over SDL2's GL context + a self-managed FBO (color texture + depth renderbuffer). `prepare_for_rendering` binds the FBO; `read_to_image` via `glReadPixels`; `resize` reallocates; `connection()` returns a surfman `Connection` (Servo requires it for WebGL); exposes the color texture for egui. |
| `src/platform/window/`  | SDL2 owns the GL/GLES context; builds `glow` + `gleam` GL from SDL's proc loader and constructs the `SdlRenderingContext`; exposes it + its color texture; `bind_default_framebuffer`; `present` via `gl_swap_window`. |
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

- `src/platform/render/sdl.rs`: `connection()` is now optional. `surfman::Connection::new()`
  is wrapped in `catch_unwind`, since surfman panics rather than returning `Err` on
  missing EGL symbols. Capable platforms (desktop, EGL 1.5) keep a real connection and
  WebGL; EGL 1.4 devices get `None`.
- `components/paint/paint.rs` in our Servo fork (pinned via `[patch.crates-io]`,
  see `docs/SERVO_PATCH.md`): `register_rendering_context` treats the connection as
  optional instead of calling `.expect()`. WebGL is disabled when the connection is
  absent, but everything else renders fine.

WebGL on EGL 1.4 would need a surfman patch to fall back to `eglGetDisplay` (EGL 1.0), or
to wrap SDL's current EGL display.

Since 2026-08-17 Servo also has a `webgl` cargo feature, so the handheld build leaves the
engine's WebGL out of the binary instead of shipping a WebGL that can never get a
connection: retsurf's own `webgl` feature (default on, off under `--no-default-features`)
enables `servo/webgl`. The patch above still matters for the *default* build on a device
whose driver can't provide a connection — an Android GPU, or a desktop build run on
EGL 1.4.

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

## Building for armhf (Miyoo Mini)

A different device family — SSD202D, armv7, no GPU at all. The renderer for it is the
`software` feature below, and `allium/` and `onionos/` are the device-side packages
(`allium/README.md`, `onionos/README.md`) — one binary, two card layouts.

```sh
tools/armhf/build.sh              # prints the binary's path
RETSURF_ARM_LTO=thin tools/armhf/build.sh   # lighter link when RAM is short
tools/armhf/package-miyoo.sh -n   # both zips around the binary that is already built
```

It cross-compiles from x86_64 in a container, unlike the aarch64 build above, which runs
natively under qemu. `.github/workflows/build-linux-armhf.yml` is the same recipe in CI,
kept as its own workflow so it shares nothing with the aarch64 one.

**The compiler and the userland come from different places, and that is the whole design.**
The Miyoo Mini community toolchain supplies only its sysroot — glibc 2.28, the floor the
device's loader sets — while the compiler is Ubuntu's cross GCC 10, because SpiderMonkey 153
requires GCC 10.1 and `_GLIBCXX_RELEASE >= 10` where that toolchain's own GCC is 8.3. GCC 12
is packaged too and cannot be used: its libstdc++ references `__libc_single_threaded`, a
glibc 2.32 symbol absent from 2.28. So libc is taken from the sysroot, libstdc++ from the
compiler and linked statically, and bindgen runs against libclang 19 — `tools/armhf/Dockerfile`
records each of those choices and why zig is still out.

Four things about this target cost a build each to find, and all four fail quietly rather
than loudly:

- **The sysroot must not reach the library search path whole.** `--sysroot` alone leaves the
  link resolving libc from the build host, taking the binary over the floor; adding
  `$SYSROOT/lib` instead answers `-static-libstdc++` with the toolchain's libstdc++ 8.3,
  older than the headers the code compiled against. Only `$SYSROOT/usr/lib` goes on it,
  which has libc's linker script and no libstdc++ at all.

- **`-mtune=cortex-a7`, never `-mcpu`.** cc-rs passes `-march=armv7-a` of its own, GCC warns
  that `-mcpu` conflicts with it, and cc-rs treats *any* stderr from a flag probe as "flag
  unsupported" — so one warning silently drops every `flag_if_supported` flag in the graph.
  `mozjs_sys` asks for `-fno-rtti` and `-fno-sized-deallocation` that way; losing the first
  breaks the link against SpiderMonkey, losing the second is an ABI mismatch on
  `operator delete` that would only show up as heap corruption on the device.
- **NEON is off unless asked for twice, and the FPU is off unless the driver carries it.**
  `-C target-feature=+neon` for the Rust half — the rustc target spec disables NEON outright
  and `-C target-cpu` does not undo that — and `-mfpu=neon-vfpv4` for the C/C++ half, which
  goes on the compiler driver rather than in `CFLAGS`: SpiderMonkey's configure probes with a
  bare `-march=armv7-a` that resets the FPU choice, and `-mfloat-abi=hard` then has nothing
  to use. That shim also settles cc-rs's `-mfpu=vfpv3-d16`, which has no NEON.
- **`HOST_CC`/`HOST_CXX` must be set.** SpiderMonkey builds tools that run on the build
  machine, and its configure otherwise falls back to the cross compiler and rejects it. They
  name GCC: 153 wants clang 19 or newer for a compiler, and the distro's is older.

The sysroot in the image carries SDL2 and fontconfig from Debian for the link step only; the
toolchain's own sysroot has neither. On the device both have to come from somewhere else —
its SDL2 is a Miyoo-specific build.

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
