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

## Toolbar auto-hide: gamepad scroll smoothness (open)

Auto-hide on scroll works (`display.toolbar_autohide`; top reflows, bottom
overlays). Open issue: **gamepad scrolling downward (while the bar hides) is
less smooth than with auto-hide off** — touch/wheel are fine. Tried, issue
persists:

- Skip rendering the bottom overlay `Area` while hidden (no per-frame
  tessellation) — `ui/mod.rs`, the `if toolbar_shown` guard.
- Made the hide **instant** (no slide animation) — kept, but did *not* fix the
  choppiness, so the slide wasn't the cause.
- Low-pass the gamepad `dt` (`App::scroll_dt` EMA in `app/router.rs`) so a
  hitched frame doesn't jump the `speed * dt` scroll. Gamepad is dt-scaled;
  touch/wheel move by raw delta, hence gamepad-only.

Since instant hide didn't help, the jitter isn't the bar's transition — it's
something per-frame in the scroll/render path while the bar is hidden (or the
servo reflow if testing the top path). Next: confirm which `toolbar_position`
reproduces it; bisect by stubbing out `notify_page_scroll`; profile the frame
during a downward gamepad scroll to see what actually spikes.

## Smaller items

- **Favicons** in tabs/menu — delegate hook noted in `browser/delegate.rs` docs.
- **Color picker / file picker / context menu** — currently dismissed with
  defaults; the prompt-overlay infrastructure (`overlay/prompt.rs`) is ready,
  they just need rendering + slots.
- **Pinch zoom action** — magnifier without reflow (`adjust_pinch_zoom`), as a
  complement to page zoom.
