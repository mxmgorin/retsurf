<h1 align="center">
  <img src="resources/images/retsurf-logo.png" alt="retsurf" width="100">
</h1>

<p align="center">A gamepad-native web browser for unconventional devices.</p>

<div align="center">
  <a href="https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml"><img src="https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml/badge.svg" alt="Linux ARM"></a>
  <a href="https://github.com/mxmgorin/retsurf/actions/workflows/build-windows.yml"><img src="https://github.com/mxmgorin/retsurf/actions/workflows/build-windows.yml/badge.svg" alt="Windows"></a>
  <a href="https://github.com/mxmgorin/retsurf/actions/workflows/build-macos.yml"><img src="https://github.com/mxmgorin/retsurf/actions/workflows/build-macos.yml/badge.svg" alt="macOS"></a>
  <a href="https://github.com/mxmgorin/retsurf/actions/workflows/build-linux.yml"><img src="https://github.com/mxmgorin/retsurf/actions/workflows/build-linux.yml/badge.svg" alt="Linux"></a>
  <a href="https://github.com/mxmgorin/retsurf/actions/workflows/build-android.yml"><img src="https://github.com/mxmgorin/retsurf/actions/workflows/build-android.yml/badge.svg" alt="Android"></a>
  <a href="https://deps.rs/repo/github/mxmgorin/retsurf"><img src="https://deps.rs/repo/github/mxmgorin/retsurf/status.svg" alt="Dependencies"></a>
</div>

retsurf (**ret**ro + **surf**ing) is an experimental web browser written in Rust, built to bring a fully featured web experience to devices where traditional browsers aren't practical.

Web rendering comes from [Servo](https://github.com/servo/servo), with SDL2 handling windowing and input and egui providing the UI. retsurf runs **without X11 or Wayland**, rendering OpenGL ES directly through KMSDRM, and is designed around **gamepad-first navigation**.

It runs on [PortMaster-compatible](https://portmaster.games/supported-devices.html) Linux handhelds, including the Miyoo Mini family, as well as regular desktops and Android. On GPU-less devices such as the Miyoo Mini, retsurf uses CPU rasterization for both web pages and its UI.

> **Work in progress.** Early development — expect bugs.

## Gallery

<table>
  <tr>
    <td align="center"><img src="resources/images/retsurf-trimui-smart-pro.jpg" alt="retsurf on a TrimUI Smart Pro" width="260"></td>
    <td align="center"><img src="resources/images/retsurf-rgb30.jpg" alt="retsurf on a Powkiddy RGB30" width="260"></td>
    <td align="center"><img src="resources/images/retsurf-rg35xx-sp.jpg" alt="retsurf on an Anbernic RG35XX SP" width="260"></td>
  </tr>
</table>

| Start page | Browsing | Link hints | Keyboard |
|:---:|:---:|:---:|:---:|
| ![The built-in start page: a search field over a speed-dial grid of pinned sites](resources/images/retsurf-start-page.png) | ![Hacker News rendered by Servo in its mobile layout, the toolbar above it](resources/images/retsurf-page.png) | ![Vimium-style hints over a Wikipedia article, each link labeled with the gamepad buttons that open it](resources/images/retsurf-hints.png) | ![The on-screen keyboard raised under the start page's search field, which shows what has been typed](resources/images/retsurf-keyboard.png) |

| Tabs | Downloads | Reader mode | Settings |
|:---:|:---:|:---:|:---:|
| ![The menu's Tabs section: open tabs by title, each with a bookmark and close button](resources/images/retsurf-tabs.png) | ![The Downloads section: one file downloading with percentage and size, one finished](resources/images/retsurf-downloads.png) | ![A Wikipedia article stripped to its text by reader mode](resources/images/retsurf-reader.png) | ![The settings overlay on its Browser tab: home page, search URL, user agent, zoom, theme and the experimental web features](resources/images/retsurf-settings.png) |

## Why?

Handheld Linux devices have no good browser options. Lightweight browsers often struggle with modern, JavaScript-heavy sites, while desktop browsers depend on a windowing system, mouse and keyboard, and hardware that these devices do not have.

retsurf is an attempt to fill that gap: a modern web engine, gamepad-first controls, and direct rendering without a compositor.

## Features

- **Gamepad-native navigation**<br>
  Virtual cursor with stick/D-pad control, Vimium-style link hints, and an on-screen keyboard (QWERTY + ЙЦУКЕН).

- **Customizable controls**<br>
  Every gesture is rebindable in-app or in [`bindings.toml`](docs/CONFIGURATION.md#bindings-bindingstoml), with D-pad scrolling for stickless devices.

- **Tabs, bookmarks, history, and downloads**<br>
  Everything lives in one full-screen menu. Downloads run in the background with progress and cancellation, with an ⬇ toolbar chip for active downloads.

- **Real page zoom**<br>
  Reflows the layout rather than magnifying it, following Firefox's 50–300% zoom ladder. Zoom is per-tab, so the whole web remains usable on a small screen.

- **Reader mode**<br>
  Strips pages down to their articles using Mozilla's [Readability](https://github.com/mozilla/readability). Runs in place, so it also works with logged-in and dynamically rendered pages.

- **Dark web pages**<br>
  Uses sites' own dark themes through `prefers-color-scheme`, or forces a dark appearance by inverting pages that don't provide one.

- **Ad & tracker blocking**<br>
  Network-level blocking powered by Brave's [`adblock-rust`](https://github.com/brave/adblock-rust), using EasyList and EasyPrivacy. Filters are compiled and cached locally, making warm starts instant and allowing blocking to work offline.

- **Native start page**<br>
  A search/URL field over a speed-dial grid of pins (`retsurf:home`), fully controller-navigable like every other overlay.

- **In-app updates**<br>
  Checks GitHub for updates and shows release notes inline. On PortMaster handhelds and Linux desktops, updates can be installed in place; elsewhere, the release page is opened. Supports stable, beta, and dev channels.

- **Web Audio**<br>
  Custom Servo media backend with SDL2 output. Supports oscillators, gain, filters, panners, scripted buffers, and `decodeAudioData` for MP3, WAV, FLAC, Ogg/Vorbis, and AAC/M4A, with resampling to the context rate.

- **Modern rendering**<br>
  Servo's WebRender uses OpenGL ES 3.x with a single GL context and zero CPU readback, drawing directly into the on-screen framebuffer.

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

### Miyoo Mini (Allium)

There is no GPU on this device, so the page is rasterized by WebRender's software
backend and the chrome by SDL's 2D renderer — the `software` cargo feature. The armv7
binary and the card layout around it are cross-built in a container:

```sh
tools/armhf/build.sh                 # prints the binary's path
tools/armhf/package-allium.sh -n     # dist/retsurf-allium.zip around what was just built
```

Packaging borrows three sets of files it cannot ship itself — the Miyoo SDL2 build
above all; the script header says where each comes from. See **[the Allium
package](allium/README.md)** for the device side: install, controls, and what 128 MB
of RAM costs you.

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
- [Allium package](allium/README.md) — the Miyoo Mini build: install, controls, limits
- [The Servo Book](https://book.servo.org/) — the embedded engine: architecture, concepts, build system
