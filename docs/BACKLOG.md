# Backlog

## Address bar autocomplete

Suggestions from history + bookmarks while typing — every character not typed
on the OSK counts. Data already lives in `data/history.rs` /
`data/bookmarks.rs`; needs a dropdown under the toolbar navigable with the
stick (same Nav routing as other overlays).

## Session restore

Reopen the previous run's tabs on startup. Persistence pattern is already
established (history/bookmarks TOML in the data dir); save tab URLs on clean
shutdown, restore instead of `home_page` when present.

## Auto-hide toolbar on scroll

Slide the toolbar out of the way while scrolling down, bring it back on scroll
up — like mobile Chrome/Safari. Scroll direction is already available (every
scroll funnels through `browser.scroll(dx, dy, …)`; `dy` sign = direction), so
detection + a small threshold/accumulator is cheap.

The real work is *not* reflowing the page on every show/hide. Collapsing the
top panel changes the `CentralPanel` height → `browser.resize` → full reflow
(janky on Mali). Instead keep the Servo viewport at full window height and draw
the toolbar as a floating overlay that slides in/out over the page. That means
reworking `webview_top` from a hardcoded layout offset into an overlay offset,
and re-checking the input-mapping / overlay-anchor call sites (`ui/mod.rs`
cursor hit-test + click mapping, `hints.rs`, `home.rs`). Shares its root cause
with a configurable top/bottom bar position — both want `webview_top`
generalized into a proper webview rect.

## Smaller items

- **Favicons** in tabs/menu — delegate hook noted in `browser/delegate.rs` docs.
- **Color picker / file picker / context menu** — currently dismissed with
  defaults; the prompt-overlay infrastructure (`overlay/prompt.rs`) is ready,
  they just need rendering + slots.
- **Pinch zoom action** — magnifier without reflow (`adjust_pinch_zoom`), as a
  complement to page zoom.
