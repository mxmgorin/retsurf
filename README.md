<h1 align="center">
  <img src="docs/images/retsurf-wordmark.png" alt="retsurf" width="320">
</h1>

<p align="center">A gamepad-native web browser.</p>

<div align="center">
  <a href="https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml"><img src="https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml/badge.svg" alt="Linux ARM"></a>
  <a href="https://github.com/mxmgorin/retsurf/actions/workflows/build-windows.yml"><img src="https://github.com/mxmgorin/retsurf/actions/workflows/build-windows.yml/badge.svg" alt="Windows"></a>
  <a href="https://github.com/mxmgorin/retsurf/actions/workflows/build-macos.yml"><img src="https://github.com/mxmgorin/retsurf/actions/workflows/build-macos.yml/badge.svg" alt="macOS"></a>
  <a href="https://github.com/mxmgorin/retsurf/actions/workflows/build-linux.yml"><img src="https://github.com/mxmgorin/retsurf/actions/workflows/build-linux.yml/badge.svg" alt="Linux"></a>
  <a href="https://github.com/mxmgorin/retsurf/actions/workflows/build-android.yml"><img src="https://github.com/mxmgorin/retsurf/actions/workflows/build-android.yml/badge.svg" alt="Android"></a>
  <a href="https://deps.rs/repo/github/mxmgorin/retsurf"><img src="https://deps.rs/repo/github/mxmgorin/retsurf/status.svg" alt="Dependencies"></a>
</div>

retsurf (**ret**ro + **surf**ing) is an experimental web browser written in Rust: [Servo](https://github.com/servo/servo) renders, SDL2 handles windowing and input, egui draws the UI.

It runs **without X11 or Wayland** — OpenGL ES straight on KMS/DRM — with full gamepad control, targeting PortMaster-compatible Linux handhelds (Knulli, muOS, ROCKNIX), regular desktops, and Android (touch + system keyboard).

> **Work in progress.** Early development — expect bugs.

## Gallery

<table>
  <tr>
    <td align="center"><img src="docs/images/retsurf-trimui-smart-pro.jpg" alt="retsurf on a TrimUI Smart Pro" width="260"></td>
    <td align="center"><img src="docs/images/retsurf-rgb30.jpg" alt="retsurf on a Powkiddy RGB30" width="260"></td>
    <td align="center"><img src="docs/images/retsurf-rg35xx-sp.jpg" alt="retsurf on an Anbernic RG35XX SP" width="260"></td>
  </tr>
</table>

## Why?

Handheld Linux distros lack a usable browser: lightweight options can't render modern JS-heavy sites, and desktop browsers assume a windowing setup and pointer input these devices don't have. retsurf targets that gap — a modern engine, native gamepad navigation, no compositor.

## Features

- **Gamepad-native navigation** — a virtual cursor (stick / D-pad), Vimium-style link hints, and an on-screen keyboard (QWERTY + ЙЦУКЕН). Every gesture is rebindable in-app or in [`bindings.toml`](docs/CONFIGURATION.md#bindings-bindingstoml), with a D-pad scroll mode for stickless devices.
- **Tabs, bookmarks, history, downloads** — in one full-screen menu. Files download in the background with progress, cancel, and a ⬇ toolbar chip.
- **Real page zoom** — reflows the layout (not a magnifier) along Firefox's 50–300% ladder, per tab, so the whole web fits a small screen.
- **Reader mode** — strips a page to its article with Mozilla's [Readability](https://github.com/mozilla/readability). Runs in place, so logged-in and dynamic pages work too.
- **Ad & tracker blocking** — network-level via [Brave's adblock-rust](https://github.com/brave/adblock-rust) (EasyList + EasyPrivacy), compiled and cached locally so warm starts are instant and work offline.
- **Native start page** — a search/URL field over a speed-dial grid of pins (`retsurf:home`), controller-navigable like every other overlay.
- **In-app updates** — checks GitHub, shows release notes inline, and installs in place on PortMaster handhelds and Linux desktops (elsewhere it opens the release page). Stable, beta, and dev channels.
- **Modern rendering** — Servo's WebRender on OpenGL ES 3.x: a single GL context and zero CPU readback, Servo drawing straight into the on-screen framebuffer.

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

On a Wayland desktop, retsurf auto-selects SDL's Wayland driver and a GLES context. Environment variables override the config, data, and download paths and set logging — see [Configuration](docs/CONFIGURATION.md#environment-variables).

### Android

retsurf also builds an APK: SDL2 loads the Rust code as a cdylib and the GLES render
path carries over, with touch input and the system soft keyboard. With the Android
SDK/NDK installed:

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk --locked
./android/scripts/build.sh release   # android/app/build/outputs/apk/release/app-release.apk
adb install -r android/app/build/outputs/apk/release/app-release.apk
```

## Configuration

`config.toml` (settings) and `bindings.toml` (gamepad/keyboard mappings) live in the
user data dir (`SDL_GetPrefPath`, e.g. `~/.local/share/mxmgorin/retsurf/` on Linux).
Templates with the defaults are written on first run, and most settings are editable
in-app from the settings overlay.

See **[Configuration & bindings](docs/CONFIGURATION.md)** for every option and the
full bindings reference.

## Support the project

Bug reports and ideas are welcome — open an issue for anything broken or missing.
If you find retsurf useful, a star on GitHub helps others discover it — and keeps
me motivated.

## References

- [Handheld notes](docs/HANDHELD_PORT.md) — how it works, architecture, porting status
- [Android notes](docs/ANDROID_PORT.md) — build/packaging, storage, touch, lifecycle, status
- [The Servo Book](https://book.servo.org/) — the embedded engine: architecture, concepts, build system
