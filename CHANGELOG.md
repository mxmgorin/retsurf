# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Every chrome icon is now a [Phosphor](https://phosphoricons.com/) glyph (bold
  weight, filled for the "on" half of a pair) instead of whatever icon-ish
  character egui's bundled fonts happened to carry: one optical weight across the
  toolbar, menu, settings and prompts. The hand-painted home button is gone, and
  new icons no longer have to be cmap-checked against three fonts first.
- Input moved onto the [inputbind](https://github.com/mxmgorin/inputbind) crate:
  the gesture machine, `bindings.toml`, gesture capture and the rebinding screen
  now live there instead of in retsurf.
  - The D-pad, L2/R2 and the stick clicks are bindable gestures (`up`, `l2`,
    `l3`, ...) on top of what they already do — the D-pad still drives the aim
    vector, L2/R2 still act as the on-screen keyboard's Shift/Enter. Nothing is
    bound to them by default.
  - **Chords are now ordered**: `select+start` means "hold Select, then press
    Start". The stock layout binds both orders of its two chords, so squeezing
    either way still works, but a hand-written `l1+r1` no longer fires as
    `r1+l1`. Only the leading button's tap is deferred, so the other keeps its
    press edge.
  - The Controls screen is a line per action, opened to show and edit what is
    bound to it, instead of one long flat list. It refuses an edit that would
    leave the gamepad unable to confirm, cancel, or reach settings, and refuses
    a hold or chord on a button whose tap has to fire on the press edge — both
    with a reason, rather than dropping the binding silently at load.
  - Key names come from SDL itself, so any key it can name is bindable
    (`ctrl+f5`, `pagedown`), and a bare modifier can be bound on its own.
  - An empty `[gamepad]` or `[keyboard]` table in a hand-edited `bindings.toml`
    now means "nothing bound" rather than silently falling back to the defaults.
    A missing file still gets the defaults written to it.

- Bookmark and history rows lead with the site name, with the rest of the URL dim
  behind it, instead of the raw `https://...` — the part that distinguishes two
  rows is now the part you read first, and it is the tail that truncates.
- A page that is loading paints an accent line along the toolbar's page-facing
  edge. Static, not a spinner: it costs no repaints of its own.

### Fixed

- A tab could stay "loading" forever — reload greyed out for good — on any page
  whose scripts insert a `<body>` after the load: Servo reports that as
  `LoadStatus::HeadParsed`, with no `Complete` to follow (wikipedia does it). The
  flag now follows the document's ready state only, and loads we start ourselves
  arm it directly, since Servo announces `Started` only for page-initiated ones.
- A download that failed before its file existed came back from
  `downloads.toml` nameless; its row now falls back to the URL's name.
- Downloads' "Clear finished" was a bar button, reachable only with a mouse. It
  is now the list's top row, like History's "Clear all".
- Link-hint badges covered the first characters of the element they label; they
  now sit just above it.
- Every toolbar button sat two pixels above the address bar's center, so the
  reader icon inside the field broke the icon row. egui resolves `Align::Center`
  against the row height known so far, and the field — the tallest item — is
  added last, leaving everything before it top-aligned. The row now declares its
  height up front.
- Text in every single-line field (address bar, start page, pin editor, prompts)
  hugged the top of its box rather than its center: egui's TextEdit defaults to
  the top, and these boxes are sized by the icon slot or the frame's margins, not
  by the line.
- The start page and pin editor drew their placeholder in the default text size
  while typing switched to the field's own, so the line grew on the first
  keystroke. Both now take one font.
- A page click held while the bindings were saved, or while binding capture
  opened, could leave the mouse button stuck down on the page: the press was
  sent and its release never was. The gesture machine now closes what it owes.
- Editing a gamepad tunable in settings (dead zone, trigger threshold, hold
  time) no longer discards a gesture in progress — the machines are retuned in
  place rather than rebuilt.

## [0.4.0] - 2026-08-09

### Added

- Page theme (`[browser] page_theme`, also on the settings Browser tab): `dark`
  tells pages to prefer a dark color scheme, so sites that ship one serve it;
  `forced-dark` inverts every page for sites that don't. `light` stays the
  default. Changing it reloads the open tabs.
- Web Audio output: retsurf now registers its own servo-media backend and plays the
  audio graph through SDL2, so oscillators, gain, filters, panners, analysers and
  JS-filled `AudioBuffer`s make sound. Toggle in settings or `[audio] enabled`.
  `<audio>`/`<video>` elements are still unsupported.
- `decodeAudioData()` decodes mp3, wav, flac, ogg/vorbis and aac/m4a, resampling to
  the context's rate. Sound effects and music that pages load as a file and play
  through Web Audio now work; the promise used to never settle. `[audio]
  max_decode_seconds` (default 300) rejects clips too long to fit in memory.

### Fixed

- Links with the `download` attribute now download: plain http(s) links fetch in
  the background under the attribute's name (Servo ignores the attribute and
  navigated instead), and `data:` links (canvas "save as PNG" exports) are saved.
- The download capture now runs as a Servo user script in every document before
  the page's own code, so files built by scripts that run at page load — or by
  clicks before the page finished loading, or inside same-origin iframes — are
  caught; it used to be injected only after the full page load.
- Downloads honor the name the server picks (`Content-Disposition`, or the file a
  mirror redirect lands on) instead of always naming the file after the link URL.
- A download whose connection goes silent now fails with "stalled" after 60 s
  instead of staying active forever, and cancelling one that is stuck no longer
  leaves it in the list as active. Connecting to a dead server times out promptly.
- Downloads send the browser's User-Agent and the linking page as Referer, so
  hosts that reject unknown clients or hotlinking serve the file.
- Turning audio off no longer leaves `decodeAudioData()` hanging: the decoder is
  registered even with no output, so the promise resolves and pages stay silent
  instead of waiting forever.
- A sink with nowhere to send audio (audio off, or a `MediaStreamAudioDestinationNode`)
  no longer keeps Servo's audio render thread busy rendering blocks that are thrown
  away.
- Download buttons that build the file in JavaScript (a `fetch` plus a blob URL,
  as on gettestfiles.com) now save it. Servo has no `download` attribute support,
  so the click used to navigate to the blob and land on a blank error page; the
  file is captured from the page instead and appears in the downloads list.

### Changed

- The engine is now built from unreleased Servo (`main`, past the 0.5 release)
  rather than the published 0.4 crates, picking up its layout, font-shaping and
  HTTP-cache fixes along with several crash fixes — plus, since 0.5, fetch
  `Request` navigation flags, HTTP/2 upload fixes and further layout and GC
  hazard fixes. Unreleased upstream is not curated, so expect rough edges; the
  released line is kept as a fallback.
- Bumped `base64` to 0.23 (the version Servo itself uses) and `egui-sdl2` to 0.8,
  which brings egui 0.35.

## [0.3.1] - 2026-07-28

### Fixed

- Freeze on pages that use `IntersectionObserver` together with
  `display: contents`, reddit among them: Servo's containing-block walk never
  advanced past a boxless ancestor and spun forever in the script thread.
  Patched in `vendor/servo-layout` (see `docs/SERVO_PATCH.md`).
- Crash on pages where a blocked subresource carries an `integrity` attribute
  (e.g. gbatemp.net, whose Cloudflare beacon script is ad-blocked): the blocked
  load now completes with an empty body instead of no body at all.
- Update channel switched in settings is now used by a "Check for updates" made
  in the same visit, instead of only after the overlay is closed.
- Android APK no longer reports a stale `0.1.0` / versionCode 1: both are derived
  from `Cargo.toml`.

## [0.3.0] - 2026-07-27

### Added

- Experimental web-feature toggles, configurable individually or through presets
  (Balanced by default), to enable or disable engine features.
- Per-page image limit (`[data_saving] max_images_per_page`, default 48) to avoid
  freezing on image-heavy pages (e.g. PortMaster).
- Update channel (release, beta, or CI) is now selectable directly in settings.
- Toolbar icon indicating when an update is available.

## [0.2.0] - 2026-07-22

### Added

- In-app self-update with selectable release, beta, and CI channels.
- Automatic update check at startup, with release notes shown in-app.
- Surf wave on the home-screen wordmark.
- Support section in the README.

### Changed

- Bumped `sha2` to 0.11 and `zip` to 8.

### Fixed

- Free disk space before macOS DMG packaging so the release build no longer runs out of space.

## [0.1.0] - 2026-07-18

Initial release. A Servo-based web browser built for handheld and gamepad-first
use (Knulli, muOS, ROCKNIX), with desktop and Android builds.

### Added

- Servo web engine (0.4), multiple tabs, and opening `target=_blank` /
  `window.open` links in new tabs.
- Per-tab real page zoom with a Firefox-style zoom ladder and configurable default.
- Configurable user agent (desktop, mobile, iOS keywords, or a custom string).
- Reader mode via a vendored readability.js with a dark small-screen layout.
- Ad blocker (adblock-rust) with a config toggle, plus a content filter.
- File downloads with a downloads menu section and configurable download directory.
- Native egui start page with speed dial and search, pinned speed-dial tiles,
  and a standalone speed-dial editor.
- Bookmarks, history, and settings overlays with gamepad, mouse, and keyboard
  navigation.
- Vimium-like link-hint navigation, including typed combo hints using a gamepad
  button alphabet, keyboard hint entry, and auto-scroll at the viewport edge.
- Gamepad-driven virtual cursor that can click toolbar UI, auto-hides when idle,
  and is clamped to the web view.
- On-screen keyboard with switchable en/ru layouts, symbols, shift hints, and
  gamepad button shortcuts.
- Rebindable gamepad buttons (with hold and chord gestures) and rebindable
  keyboard shortcuts over shared actions, editable in settings.
- Toolbar position (top or bottom) and auto-hide on scroll.
- Modal overlays for select pickers and JavaScript alert / confirm / prompt dialogs.
- Opt-in usage memory overlay and a `memory_profile` option for Servo engine tuning.
- Persistent site data (cookies, localStorage) across restarts, an organized
  data directory, and a `RETSURF_DATA_DIR` override.
- Brand icon and wordmark (rs monogram with surf wave) and window icon.
- Builds for Linux, Linux ARM, Windows, macOS (DMG), and Android, with
  PortMaster packaging.

### Performance

- LTO, single codegen unit, and target-cpu tuning; Servo thread counts sized to
  available cores.
- Deferred history writes (dirty flag with flush on close, throttle, and shutdown).
- Color-only FBO with in-place readback flip and NEAREST composite.

[Unreleased]: https://github.com/mxmgorin/retsurf/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/mxmgorin/retsurf/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/mxmgorin/retsurf/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/mxmgorin/retsurf/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/mxmgorin/retsurf/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/mxmgorin/retsurf/releases/tag/v0.1.0
