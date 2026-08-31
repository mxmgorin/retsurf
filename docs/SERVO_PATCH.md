# The Servo patches

retsurf carries seven small changes to Servo. They live as commits on the
`retsurf-main-0.7` branch of our fork (`mxmgorin/servo`) — one branch per retsurf
minor, rebased onto upstream `main` as it moves — which `[patch.crates-io]` in
`Cargo.toml` pins by `rev`, so the engine retsurf builds is Servo's unreleased
`main` plus exactly these seven fixes. Every rev an older release pinned is kept
reachable by a tag named after that release (`retsurf-v0.4.0`, `retsurf-v0.5.1`),
and `patches/` in this repo mirrors the diff as plain files so the change is
readable without fetching the fork.

Two further patches — WebRender kept off the paths swgl does not implement, and
a painter removable without surfman details — are needed only by the software
renderer, so they sit on `retsurf-swgl` (these seven plus those two) rather than
on the line above. Nothing here depends on them.

The two below have a design worth writing down. Patches 3 to 7 are short fixes
whose commit messages carry the reasoning, with the diffs in
`patches/0003..0007`:

- **3. `components/script`: drop a script message whose event loop is gone.**
- **4. `components/paint`: drop a gone pipeline's display list.** Nothing else
  took it out of the WebRender scene, so every navigation left one behind.
- **5. `components/config`: let the malloc heap's GC thresholds be set by pref.**
  SpiderMonkey's own default (38 MB) assumes a desktop; the memory tiers want it
  lower.
- **6. `components/script`: install the SpiderMonkey testing functions under the
  internals pref.** `dumpHeap` and friends, for measuring a release build.
- **7. `components/script`: drop a dying document's rooted callbacks and
  promises.** Rust-owned GC roots (event listeners, `fonts.ready`) kept a
  navigated-away document's JS heap alive.

## 1. `components/paint`: optional surfman connection

Lets Servo start on handhelds whose GL driver is EGL 1.4. The change lives in
two places:

- `components/paint/paint.rs` in the fork
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

## 2. `components/layout`: containing-block walk hangs on boxless ancestors

Fixes a hard freeze on pages that combine `IntersectionObserver` with
`display: contents` — reddit and MDN among them. The change is in
`components/layout/query.rs`, in `containing_block_for_node`.

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

Upstream `main` still has the same code (checked 2026-08-17).

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

`[patch.crates-io]` pins retsurf to one Servo revision, and a fresh clone can no
longer build offline — cargo needs the fork (~1.7 GB, cached once per machine).
Each Servo bump means rebasing the fork branch and moving the pinned `rev`, and a
patch is dropped once it lands upstream — when nothing is left to carry,
`[patch.crates-io]` goes away entirely. See `docs/HANDHELD_PORT.md` for the
broader GLES port and the related dual-GL-context pitfalls.
