# Backlog

## Page zoom — designed, ready to build

Real page zoom (`WebView::set_page_zoom`, reflows layout — not the pinch
magnifier; pinch can become a separate action later if a "loupe" is wanted).

- Firefox-style zoom ladder: 50 67 80 90 **100** 110 125 150 175 200 250 300 %.
- Per-tab for free (page zoom lives on the WebView).
- `[browser] page_zoom = 1.0` config default — set `1.25` once and the whole
  web is bigger; arguably the most valuable part on a small screen.
- Three bindable actions: `zoom_in` / `zoom_out` / `zoom_reset`.
  Keyboard defaults `ctrl+=` / `ctrl+-` / `ctrl+0`; gamepad defaults
  `hold:r1` / `hold:l1` (taps stay back/forward, holds are free).
- Toolbar chip "125%" visible only while zoom ≠ 100%; clicking it resets.
- Pieces: bindings actions, `BrowserCommand::Zoom`, ladder step in
  `browser/mod.rs`, toolbar chip, config key, README.

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
- **Open in new tab** as a second link-hints gesture (e.g. hold A on a hint).
- **Color picker / file picker / context menu** — currently dismissed with
  defaults; the prompt-overlay infrastructure (`overlay/prompt.rs`) is ready,
  they just need rendering + slots.
- **Pinch zoom action** — magnifier without reflow (`adjust_pinch_zoom`), as a
  complement to page zoom.
