[![Linux ARM](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml/badge.svg)](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml)
[![Windows](https://github.com/mxmgorin/retsurf/actions/workflows/build-windows.yml/badge.svg)](https://github.com/mxmgorin/retsurf/actions/workflows/build-windows.yml)
[![macOS](https://github.com/mxmgorin/retsurf/actions/workflows/build-macos.yml/badge.svg)](https://github.com/mxmgorin/retsurf/actions/workflows/build-macos.yml)
[![Linux](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux.yml/badge.svg)](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux.yml)
[![Android](https://github.com/mxmgorin/retsurf/actions/workflows/build-android.yml/badge.svg)](https://github.com/mxmgorin/retsurf/actions/workflows/build-android.yml)
[![Dependencies](https://deps.rs/repo/github/mxmgorin/retsurf/status.svg)](https://deps.rs/repo/github/mxmgorin/retsurf)

## retsurf

A lightweight, experimental web browser written in **Rust**, using [**Servo**](https://github.com/servo/servo) as the rendering engine, **SDL2** for windowing and input, and **egui** for the UI.

It is designed to run **without X11 or Wayland** — rendering through **OpenGL ES** on bare KMS/DRM — with **gamepad support**, targeting PortMaster-compatible Linux handhelds (**Knulli, muOS, ROCKNIX**), as well as regular desktops. It also runs on **Android** (touch + system keyboard).

> 🛠️ **Work in progress.** Early development — experimental and bugs are expected.

## Gallery

<table>
  <tr>
    <td align="center"><img src="docs/images/retsurf-trimui-smart-pro.jpg" alt="retsurf on a TrimUI Smart Pro" width="260"></td>
    <td align="center"><img src="docs/images/retsurf-rgb30.jpg" alt="retsurf on a Powkiddy RGB30" width="260"></td>
    <td align="center"><img src="docs/images/retsurf-rg35xx-sp.jpg" alt="retsurf on an Anbernic RG35XX SP" width="260"></td>
  </tr>
</table>

## Why?

Handheld Linux distros (Knulli, muOS, ROCKNIX) lack a usable browser. Lightweight options can't render modern JS-heavy sites; desktop browsers assume a windowing setup and pointer input these devices don't have. `retsurf` targets that gap with a modern rendering engine, native gamepad navigation, and no compositor dependency.

## Features

**Gamepad support**
- Virtual **cursor** (left stick / D-pad) that can click page links *and* toolbar buttons
- **Link hints** (Y or L3) — Vimium adapted for a gamepad: clickable elements get highlighted, the stick hops between them spatially, A clicks (hold A / Enter on a link to open it in a background tab)
- **On-screen keyboard** with symbols, caps, shift, and switchable layouts (QWERTY + ЙЦУКЕН built in, picked via config) for typing URLs and searches
- Full-screen **menu** (Select) with **Tabs**, **Bookmarks**, **History**, and **Downloads** sections
- **Rebindable controls**: remap any gamepad gesture in-app from the **Controls** settings section, or edit `bindings.toml` directly — gamepad gestures (tap, hold, two-button chords) and keyboard shortcuts over the same actions, plus a D-pad cursor to scroll toggle for devices without analog sticks
- Defaults: right-stick scroll · A = click/select · B = back / close (hold: home) · X = keyboard (hold: reader mode) · Y = link hints (hold: bookmark) · L1/R1 = back / forward (hold: zoom out / in; both together: reset zoom) · L2/R2 = switch tabs · L3 = link hints · R3 = settings · Start = D-pad scroll toggle (hold: reload) · Select = menu (hold: settings) · Select+Start = settings (press again to quit)

**Page zoom**
- Real zoom (reflows the layout, not a magnifier), stepping Firefox's 50–300% ladder, per tab
- `[browser] page_zoom` in the config scales every tab by default — set `1.25` once and the whole web fits a small screen better
- Hold R1/L1, `ctrl+=`/`ctrl+-`/`ctrl+0`, or the bindable `zoom_in`/`zoom_out`/`zoom_reset` actions

**Reader mode**
- Strip a page down to its article (Mozilla's [Readability](https://github.com/mozilla/readability), the Firefox Reader View engine) with a dark, narrow-column layout sized for small screens
- Runs in place — no refetch, so it works on logged-in and dynamic pages; toggling off reloads
- Toggle via the icon toolbar button, R3 (or hold X on stickless devices), `ctrl+e`, or the bindable `reader` action

**Downloads**
- Navigating to a file link downloads it in the background instead of rendering it
- Progress, cancel, and history of finished downloads in the menu's **Downloads** section; a ⬇ toolbar chip shows what's in flight
- Saves into the system download folder (`XDG_DOWNLOAD_DIR` / `~/Downloads`) or any configured directory

**Ad blocking**
- Network-level ad & tracker blocking with [Brave's adblock-rust](https://github.com/brave/adblock-rust) engine (EasyList + EasyPrivacy by default)
- Filter lists are fetched in the background, compiled, and cached locally — warm starts are instant and work offline
- Fully configurable: toggle it off, change the lists, or change the refresh interval

**Start page**
- A built-in start page (the default `home_page`, `retsurf:home`): a search/URL field over a speed-dial grid of your saved pins
- Drawn natively with egui, so it's fully controller-navigable — D-pad/stick move the selection, A opens a tile or the keyboard to type, just like the other overlays

**Rendering**
- Real web rendering via the **Servo** engine (WebRender)
- Runs on **OpenGL ES 3.x**; no X11/Wayland required (works on bare KMS/DRM)
- Single GL context, zero CPU readback — Servo renders straight into the on-screen context

## Building & running

You need Servo's build dependencies. On Debian/Ubuntu:

```sh
sudo apt-get install -y build-essential clang cmake curl git gperf pkg-config python3 \
  libssl-dev libdbus-1-dev libfreetype6-dev libglib2.0-dev \
  libgl1-mesa-dev libegl1-mesa-dev libgles2-mesa-dev \
  libharfbuzz-dev liblzma-dev libudev-dev libunwind-dev libsdl2-dev
```

Then:

```sh
cargo run
```

On a Wayland desktop, retsurf auto-selects SDL's Wayland driver and a GLES context.

**Environment variables** override paths (config, data dir, downloads) and control
logging at launch — see [Configuration & bindings](docs/CONFIGURATION.md#environment-variables).

### Android

retsurf builds an APK: SDL2 loads the Rust code as a cdylib and the existing
GLES/FBO render path carries over, with touch input and the system soft keyboard.
With the Android SDK/NDK installed, one command cross-compiles and assembles it:

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk --locked
./android/scripts/build.sh release   # android/app/build/outputs/apk/release/app-release.apk
adb install -r android/app/build/outputs/apk/release/app-release.apk
```

Use a **release** build on device (a debug cdylib doesn't drive the initial page
load). See [Android notes](docs/ANDROID_PORT.md) for the toolchain, how the pieces
fit, and current status.

## Configuration

Settings live in `config.toml` and gamepad/keyboard mappings in `bindings.toml`,
both in the user data dir (`SDL_GetPrefPath`, e.g.
`~/.local/share/mxmgorin/retsurf/` on Linux). Templates with the defaults are
written on first run, and most settings are editable in-app from the ⚙ overlay.

See **[Configuration & bindings](docs/CONFIGURATION.md)** for every option — the
annotated `config.toml` (browser, display, OSK, performance/memory profile,
history, downloads, ad blocker, input) and the full `bindings.toml` reference.

## References

- [Configuration & bindings](docs/CONFIGURATION.md) — every `config.toml` / `bindings.toml` option
- [Handheld notes](docs/HANDHELD_PORT.md) — how it works, architecture, porting status
- [Android notes](docs/ANDROID_PORT.md) — build/packaging, storage, touch, lifecycle, status
- [The Servo Book](https://book.servo.org/title-page.html)
