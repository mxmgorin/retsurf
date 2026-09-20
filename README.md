<p align="center">
  <img src="resources/images/retsurf-banner.png" alt="retsurf — a gamepad-first web browser for retro handhelds" width="830">
</p>

<div align="center">
  <a href="https://github.com/mxmgorin/retsurf/releases/latest"><img src="https://img.shields.io/github/v/release/mxmgorin/retsurf?style=flat-square&labelColor=16171a&color=3fb8a0&label=release&cacheSeconds=180" alt="Latest release"></a>
  <a href="https://github.com/mxmgorin/retsurf/releases"><img src="https://img.shields.io/github/downloads/mxmgorin/retsurf/total?style=flat-square&labelColor=16171a&color=3fb8a0&label=downloads&cacheSeconds=180" alt="Downloads"></a>
  <!-- The check workflow, not a platform build: it is the one a push and a pull request run, and it carries the tests and clippy. -->
  <a href="https://github.com/mxmgorin/retsurf/actions/workflows/check.yml"><img src="https://img.shields.io/github/actions/workflow/status/mxmgorin/retsurf/check.yml?branch=main&style=flat-square&labelColor=16171a&color=3fb8a0&logo=githubactions&logoColor=white&label=ci&cacheSeconds=180" alt="CI"></a>
</div>

retsurf is a web browser written in Rust and built using [Servo](https://servo.org/) and [SDL2](https://www.libsdl.org/). It aims to provide a full-featured web experience while staying lightweight and portable. It targets handheld devices while also working on PCs, with flexible, remappable controls for comfortable web browsing and gaming using only a gamepad or keyboard.

**[Install](#install)** on a PortMaster handheld, a Miyoo Mini, Android, Linux, Windows, or macOS.

> **Work in progress.** Early development — expect bugs.

## Demos

| Powkiddy RGB30 | Miyoo Mini Flip |
|:---:|:---:|
| <img src="resources/images/retsurf-demo-rgb30.webp" alt="retsurf held in both hands on a Powkiddy RGB30, scrolling an image-heavy news site" height="250"> | <img src="resources/images/retsurf-demo-miyoo-mini-flip.webp" alt="retsurf on a Miyoo Mini Flip, scrolling servo.org with the D-pad" height="250"> |
| <sub>OpenGL ES</sub> | <sub>`software` build, no GPU</sub> |

## Screenshots

| Start page | Browsing | Link hints | Keyboard |
|:---:|:---:|:---:|:---:|
| ![The built-in start page: a search field over a speed-dial grid of pinned sites](resources/images/retsurf-start-page.png) | ![Hacker News rendered by Servo in its mobile layout, the toolbar above it](resources/images/retsurf-page.png) | ![Vimium-style hints over a Wikipedia article, each link labeled with the gamepad buttons that open it](resources/images/retsurf-hints.png) | ![The on-screen keyboard raised under the start page's search field, which shows what has been typed](resources/images/retsurf-keyboard.png) |

| Tabs | Downloads | Reader mode | Settings |
|:---:|:---:|:---:|:---:|
| ![The menu's Tabs section: open tabs by title, each with a bookmark and close button](resources/images/retsurf-tabs.png) | ![The Downloads section: one file downloading with percentage and size, one finished](resources/images/retsurf-downloads.png) | ![A Wikipedia article stripped to its text by reader mode](resources/images/retsurf-reader.png) | ![The settings overlay on its Browser tab: home page, search URL, user agent, zoom, theme and the experimental web features](resources/images/retsurf-settings.png) |

## Features

- **Gamepad-first navigation**<br>
  The browser is fully navigable with a gamepad or keyboard, with a virtual cursor, Vimium-style link hints, and an on-screen keyboard (QWERTY + ЙЦУКЕН).

- **Customizable browser controls**<br>
  Every browser action can be rebound in-app or in [`bindings.toml`](docs/CONFIGURATION.md#bindings-bindingstoml), with support for taps, holds, and button chords.

- **Game mode**<br>
  Hides the browser chrome and hands keyboard and gamepad input to the page, with an in-app remapper that can rebind any button or stick and save multiple profiles. Built-in input maps cover arrows, WASD, mouse, and raw gamepad input.

- **Tabs, bookmarks, history, and downloads**<br>
  Everything lives in one full-screen menu. Downloads run in the background with progress and cancellation, with a toolbar chip for active downloads.

- **Real page zoom**<br>
  Reflows the layout rather than simply magnifying it, with 50–300% zoom steps. Zoom is per-tab.

- **Reader mode**<br>
  Strips pages down to their articles using Mozilla's [Readability](https://github.com/mozilla/readability). Runs in place, so it also works with logged-in and dynamically rendered pages.

- **Dark web pages**<br>
  Uses sites' own dark themes through `prefers-color-scheme`, or forces a dark appearance by inverting pages that don't provide one.

- **Ad & tracker blocking**<br>
  Network-level blocking powered by Brave's [`adblock-rust`](https://github.com/brave/adblock-rust), using EasyList and EasyPrivacy. Filters are cached locally, so blocking works offline.

- **In-app updates**<br>
  Checks GitHub for updates, displays release notes, and installs updates in place on PortMaster handhelds and Linux desktops. Supports stable, beta, and nightly channels.

- **Audio and video**<br>
  A custom Servo media backend provides audio and video playback for MP3, WAV, FLAC, Ogg/Vorbis, AAC/M4A, and H.264 video in MP4. Supports direct media files and embedded players, but not streaming sites that require MSE.

- **No display server required**<br>
  SDL2 draws through whatever video backend the firmware ships, including handhelds that run none at all. X11 and Wayland are optional, not required.

- **Hardware or software rendering**<br>
  A custom Servo rendering backend uses OpenGL ES for GPU-accelerated rendering on supported devices, with a CPU-based software renderer for devices without a GPU.

## Install

Download from [Releases](https://github.com/mxmgorin/retsurf/releases), then:

| Device                                                                                                                  | Package                                                 | Where it goes                      |
| ----------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- | ---------------------------------- |
| [PortMaster handhelds](https://portmaster.games/supported-devices.html) (ArkOS, dArkOS, EmuELEC, Knulli, muOS, ROCKNIX) | `retsurf-portmaster.zip`                                | ports folder, e.g. `/roms/ports/`  |
| Miyoo Mini Flip and Plus on [OnionOS](https://onionui.github.io/)                                                       | `retsurf-onionos.zip`                                   | `App/Retsurf/` on the SD card      |
| Miyoo Mini Flip and Plus on [Allium](https://github.com/goweiwen/Allium)                                                | `retsurf-allium.zip`                                    | `Apps/Retsurf.pak/` on the SD card |
| Android                                                                                                                 | `retsurf-android-arm64.apk`                             | sideload it                        |
| Linux                                                                                                                   | `retsurf-linux-x86_64.zip`, `retsurf-linux-aarch64.zip` | unpack and run                     |
| Windows                                                                                                                 | `retsurf-windows-x86_64.zip`                            | unpack and run                     |
| macOS                                                                                                                   | `retsurf-macos-aarch64.dmg`                             | open it and run `Retsurf.app`      |

On both Miyoo firmwares the app shows up in the Apps menu, and **MENU quits** it.

## Building

`cargo run`, once Servo's build dependencies are installed. See **[Building from
source](docs/BUILDING.md)** for the prerequisites on each OS, the Cargo features,
Android, and the handheld cross-builds.

## Configuration

`config.toml` (settings) and `bindings.toml` (gamepad/keyboard mappings) live in the
user data dir (`SDL_GetPrefPath`, e.g. `~/.local/share/mxmgorin/retsurf/` on Linux).
Templates with the defaults are written on first run. See **[Configuration & bindings](docs/CONFIGURATION.md)** for every option and the
full bindings reference.

## How to help

If you find the project useful, here is how you can help:

- **Tell other people about it.** Sharing the project helps it reach more users.
- **Report bugs and request features** in [Issues](https://github.com/mxmgorin/retsurf/issues). If something is broken or you have an idea, let me know.
- **Star the repo.** It helps the project get noticed and keeps me motivated.

## Credits

- Web rendering by [Servo](https://servo.org)
- [SDL2](https://libsdl.org) through [rust-sdl2](https://github.com/Rust-SDL2/rust-sdl2)
  for window, input and audio
- [egui](https://github.com/emilk/egui) draws every overlay, with icons from
  [Phosphor](https://phosphoricons.com/)
- Blocking by Brave's [adblock-rust](https://github.com/brave/adblock-rust), over
  [EasyList](https://easylist.to/) and EasyPrivacy
- Reader mode by Mozilla's [Readability](https://github.com/mozilla/readability)
- Media by [Symphonia](https://github.com/pdeljanov/Symphonia) and
  [openh264](https://github.com/ralfbiedert/openh264-rs) over Cisco's codec
- TLS by [rustls](https://github.com/rustls/rustls)
- SDL2 for the Miyoo Mini by [Steward Fu](https://github.com/steward-fu/sdl2)
