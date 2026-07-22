# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/mxmgorin/retsurf/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/mxmgorin/retsurf/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/mxmgorin/retsurf/releases/tag/v0.1.0
