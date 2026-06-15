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

## Smaller items

- **Favicons** in tabs/menu — delegate hook noted in `browser/delegate.rs` docs.
- **Color picker / file picker / context menu** — currently dismissed with
  defaults; the prompt-overlay infrastructure (`overlay/prompt.rs`) is ready,
  they just need rendering + slots.
- **Pinch zoom action** — magnifier without reflow (`adjust_pinch_zoom`), as a
  complement to page zoom.
