# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Web Audio output: retsurf now registers its own servo-media backend and plays the
  audio graph through SDL2, so oscillators, gain, filters, panners, analysers and
  JS-filled `AudioBuffer`s make sound. Toggle in settings or `[audio] enabled`.
  `<audio>`/`<video>` elements are still unsupported.
- `decodeAudioData()` decodes mp3, wav, flac, ogg/vorbis and aac/m4a, resampling to
  the context's rate. Sound effects and music that pages load as a file and play
  through Web Audio now work; the promise used to never settle. `[audio]
  max_decode_seconds` (default 300) rejects clips too long to fit in memory.

### Fixed

- A download whose connection goes silent now fails with "stalled" after 60 s
  instead of staying active forever, and cancelling one that is stuck no longer
  leaves it in the list as active. Connecting to a dead server times out promptly.
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

- The engine is now built from unreleased Servo (`main`) rather than the published
  0.4 crates, picking up its layout, font-shaping and HTTP-cache fixes along with
  several crash fixes. Unreleased upstream is not curated, so expect rough edges;
  `docs/SERVO_WORKFLOW.md` describes how to fall back to the release line.

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

[Unreleased]: https://github.com/mxmgorin/retsurf/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/mxmgorin/retsurf/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/mxmgorin/retsurf/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/mxmgorin/retsurf/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/mxmgorin/retsurf/releases/tag/v0.1.0
