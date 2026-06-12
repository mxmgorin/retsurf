# Backlog

## Home page (speed dial) — designed, ready to build

Internal page on a custom protocol, NOT an egui overlay (chrome = egui,
content = pages; see the menu discussion).

- Implement `servo::protocol_handler::ProtocolHandler` for `retsurf://`,
  register via `ServoBuilder::protocol_registry`. servoshell uses the same
  mechanism for its internal pages.
- `retsurf://home` serves generated HTML: dark tile grid of bookmarks
  (+ maybe a recent-history row), inline CSS sized for 640×480.
- Rendered by Servo itself, so cursor / link hints / clicks / Back work with
  zero new input code; the address bar shows a clean `retsurf://home`.
- Handler runs on net threads: read `bookmarks.toml` from the data dir at
  request time instead of sharing state with the UI — always fresh, no
  synchronization.
- Cheap to extend later: `retsurf://history`, error pages, about.
- Lives in a new `browser/home.rs`.

## Address bar autocomplete

Suggestions from history + bookmarks while typing — every character not typed
on the OSK counts. Data already lives in `data/history.rs` /
`data/bookmarks.rs`; needs a dropdown under the toolbar navigable with the
stick (same Nav routing as other overlays).

## Session restore

Reopen the previous run's tabs on startup. Persistence pattern is already
established (history/bookmarks TOML in the data dir); save tab URLs on clean
shutdown, restore instead of `home_page` when present.

## Smaller items

- **Favicons** in tabs/menu — delegate hook noted in `browser/delegate.rs` docs.
- **Color picker / file picker / context menu** — currently dismissed with
  defaults; the prompt-overlay infrastructure (`overlay/prompt.rs`) is ready,
  they just need rendering + slots.
- **Pinch zoom action** — magnifier without reflow (`adjust_pinch_zoom`), as a
  complement to page zoom.
