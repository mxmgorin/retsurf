# The Servo patches

retsurf carries six small changes to Servo. They live as commits on the
`retsurf-main-0.8` branch of our fork (`mxmgorin/servo`) — one branch per retsurf
minor, rebased onto upstream `main` as it moves — which `[patch.crates-io]` in
`Cargo.toml` pins by `rev`, so the engine retsurf builds is Servo's unreleased
`main` plus exactly these six fixes. Every rev an older release pinned is kept
reachable by a tag named after that release (`retsurf-v0.4.0`, `retsurf-v0.5.1`),
and `patches/` in this repo mirrors the diff as plain files so the change is
readable without fetching the fork.

The set was eight. The containing-block walk (servo/servo#47693) and the
script-message unwrap (#47686) landed upstream and are gone; so did the
pipeline-exit half of the display-list fix (#47651), leaving patch 6 below.

One further patch sits on `retsurf-swgl` (these six plus it): the SpiderMonkey
testing functions (`dumpHeap` and friends, for measuring a release build)
installed under the internals pref. It takes `js::DefineTestingFunctions` by its
Itanium-mangled symbol name, which MSVC does not produce — it fails to link on
Windows, so it stays off the line every platform builds.

The three below have a design worth writing down. Patches 2, 3 and 6 are short
fixes whose commit messages carry the reasoning, with the diffs in
`patches/0002`, `patches/0003` and `patches/0006`:

- **2. `components/config`: let the malloc heap's GC thresholds be set by pref.**
  SpiderMonkey's own default (38 MB) assumes a desktop; the memory tiers want it
  lower.
- **3. `components/script`: drop a dying document's rooted callbacks and
  promises.** Rust-owned GC roots (event listeners, `fonts.ready`) kept a
  navigated-away document's JS heap alive.
- **6. `components/paint`: drop a removed webview's display lists.** Upstream
  takes a pipeline's scene state out on its final exit, but a WebView removed
  while pipelines are still on it gets no such message, so those display lists
  stayed in the scene.

## 1. `components/paint`: optional surfman connection

Lets Servo start on handhelds whose GL driver is EGL 1.4. The change lives in
two places:

- `components/paint/paint.rs` in the fork
- `src/platform/render/sdl.rs`, in retsurf's own `connection()`

### What was done

`Paint::register_rendering_context()` (in servo-paint) hard-`expect()`s a
surfman `Connection` and adapter:

```rust
let connection = rendering_context.connection().expect("Failed to get connection");
let adapter = connection.create_adapter().expect("Failed to create adapter");
```

The patch makes both optional: when the connection/adapter is unavailable, it
skips inserting into `painter_surfman_details_map` instead of panicking. WebGL/
WebGPU is then disabled for that painter; everything else renders normally.

The matching half is in retsurf: `surfman::Connection::new()` *panics* (rather
than returning `Err`) when EGL symbols are missing, so `render.rs` wraps it in
`catch_unwind` and returns `None` on failure.

### Why

- **The API already models absence.** `RenderingContext::connection()` returns
  `Option`, and `PainterSurfmanDetailsMap::get()` returns `Option` — the WebGL
  machinery already handles a missing entry. Only the registration site
  panicked, out of step with the API around it.
- **The connection is only used for WebGL/WebGPU external images.** No other
  rendering depends on it, so disabling it costs nothing on devices that can't
  provide it.
- **Real devices need it.** EGL 1.4 driver blobs (e.g. Mali on Knulli / muOS /
  ROCKNIX handhelds) lack `eglGetPlatformDisplay` (an EGL 1.5 symbol), so
  surfman can't create a `Connection` at all. Without the patch the engine
  panics at startup on those devices even though it renders fine otherwise.

## 4. `components/paint`: keep WebRender off the paths swgl does not implement

Two `WebRenderOptions` Servo hardcodes are wrong for a software rasterizer, and
both kill the process rather than degrading. They are now `!is_software_webrender`,
which is the same renderer-name test WebRender itself uses to pick its software
paths (`Software WebRender`, a string only swgl returns).

### What was done

- **`clear_caches_with_quads`** defaults to `true` and clears picture-cache tiles
  by drawing a quad instead of calling `glClear`. That needs
  `glDepthFunc(GL_ALWAYS)`, which swgl asserts on — and with asserts compiled out
  silently treats as `GL_LESS`, so the depth clear never happens and the tiles
  come back wrong. The option exists only as a driver workaround (`glClear`
  crashes on some Mali-T parts), so a software rasterizer has no business there.
- **`enable_dithering`** is likewise hardcoded `true`. swgl builds its shaders
  from `get_shader_features(GL | DUAL_SOURCE_BLENDING | ADVANCED_BLEND_EQUATION |
  DEBUG)` — no `DITHERING` — so WebRender asks for `ps_quad_gradient DITHERING`,
  finds no program, and aborts in `BindAttribLocation`. Any CSS gradient did it.

### Why

Servo has no software-rendering target of its own, so nothing upstream exercises
these. Both are one-line guards and neither changes anything for a GL renderer:
Mali, Adreno, desktop GL and even llvmpipe (`llvmpipe (LLVM ...)`) all fail the
name test and keep today's behaviour.

## 5. `components/shared/paint`: removing a painter that registered no details

`PainterSurfmanDetailsMap::remove` asserted the entry existed — but patch 1
deliberately does not insert one when the rendering context has no surfman
connection, so the assert fires at shutdown. Removal no longer asserts.

### Why

This is patch 1's missing half, and it is **not** software-only: the aarch64
handheld builds are `--no-default-features`, so `webgl` is off, `connection()`
returns `None`, nothing is inserted, and the same panic is waiting there. It
went unnoticed because `panic = "abort"` turns it into an exit code at the very
end of a run, after the window is already gone.

## Cost

`[patch.crates-io]` pins retsurf to one Servo revision, and a fresh clone can no
longer build offline — cargo needs the fork (~1.7 GB, cached once per machine).
Each Servo bump means rebasing the fork branch and moving the pinned `rev`, and a
patch is dropped once it lands upstream — when nothing is left to carry,
`[patch.crates-io]` goes away entirely. See `docs/HANDHELD_PORT.md` for the
broader GLES port and the related dual-GL-context pitfalls.
