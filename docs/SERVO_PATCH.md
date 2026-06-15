# The vendored Servo patch

retsurf carries one small change to Servo so it can start on handhelds whose GL
driver is EGL 1.4. The change lives in two places:

- `vendor/servo-paint/paint.rs`, pinned via `[patch.crates-io]` in `Cargo.toml`
- `src/platform/render.rs`, in retsurf's own `connection()`

## What was done

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

## Why

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

## Cost

The vendor dir + `[patch.crates-io]` pin retsurf to a specific Servo version and
must be re-vendored on every Servo bump. See `docs/HANDHELD_PORT.md` for the
broader GLES port and the related dual-GL-context pitfalls.
