# The vendored Servo patches

retsurf carries two small changes to Servo, each in a crate vendored under
`vendor/` and pinned via `[patch.crates-io]` in `Cargo.toml`.

## 1. `servo-paint`: optional surfman connection

Lets Servo start on handhelds whose GL driver is EGL 1.4. The change lives in
two places:

- `vendor/servo-paint/paint.rs`
- `src/platform/render.rs`, in retsurf's own `connection()`

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

## 2. `servo-layout`: containing-block walk hangs on boxless ancestors

Fixes a hard freeze on pages that combine `IntersectionObserver` with
`display: contents` — reddit among them. The change is in
`vendor/servo-layout/query.rs`, in `containing_block_for_node`.

### What was done

The walk up the flat tree skipped ancestors with no layout box without
advancing the cursor:

```rust
while let Some(ancestor) = unsafe { current_ancestor.dangerous_flat_tree_parent() } {
    let Some((ancestor_style, ancestor_flags)) = style_and_flags_for_node(&ancestor) else {
        continue;   // current_ancestor unchanged -> same parent forever
    };
```

`style_and_flags_for_node` returns `None` for a `LayoutBox::DisplayContents`,
so any `display: contents` ancestor made the loop spin forever. The patch moves
`current_ancestor = ancestor` to the top of the loop body, which is what the
near-identical walk in `process_scroll_container_query` already does.

### Why

- **It is an infinite loop in the script thread.** Not slow — stuck. The tab
  never paints again and one core spins at 100%.
- **Only `IntersectionObserver` reaches it.** `containing_block_for_node` backs
  `query_containing_block` and `query_containing_block_is_descendant`, whose
  only callers are the intersection-computation steps. That is why the freeze
  appears exactly when the IntersectionObserver experimental feature is on, and
  why turning it off "fixes" reddit at the cost of lazy-loading everywhere.
- **`display: contents` is ordinary.** Web-component-heavy sites wrap content in
  it constantly; it is not an edge case worth living with.
- **A boxless element can never be a containing block**, so skipping it and
  continuing the walk is also the correct answer, not just a livelock guard.

Upstream `main` still has the same code (checked 2026-07-28).

### Reproducing

`tests/pages/io-stress.html` builds a reddit-shaped feed of observed posts;
`contents=1` puts a `display: contents` wrapper in each target's ancestor chain:

```
python3 tests/serve.py 8099
# home_page = "http://127.0.0.1:8099/io-stress.html?posts=20&depth=4&contents=1"
```

Before the patch that page never renders (no beacons, `Script#1` pegged);
after it, it runs at the same frame rate as `contents=0`.

## Cost

The vendor dirs + `[patch.crates-io]` pin retsurf to a specific Servo version
and must be re-vendored on every Servo bump. See `docs/HANDHELD_PORT.md` for the
broader GLES port and the related dual-GL-context pitfalls.
