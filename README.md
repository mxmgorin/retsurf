[![Build linux ARM](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml/badge.svg)](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml)
[![Dependencies](https://deps.rs/repo/github/mxmgorin/retsurf/status.svg)](https://deps.rs/repo/github/mxmgorin/retsurf)

# 🌊 retsurf

A lightweight, experimental web browser written in **Rust**, using [**Servo**](https://github.com/servo/servo) as the rendering engine, **SDL2** for windowing and input, and **egui** for the UI.

It is designed to run **without X11 or Wayland** — rendering through **OpenGL ES** on bare KMS/DRM — with **gamepad support**, targeting PortMaster-compatible Linux handhelds (**Knulli, muOS, ROCKNIX**) on Mali-class GPUs, as well as regular Linux desktops.

> 🛠️ **Work in progress.** Early development, but it already renders real pages and
> navigates entirely from a controller on actual hardware (verified on Knulli / Mali).

<!-- TODO: a short demo GIF/video -->

## Why?

On Knulli / muOS / ROCKNIX handhelds there's effectively no way to browse the modern web. Your options are text-era browsers that break on real sites, or full desktop browsers that need a compositor and a keyboard+mouse. retsurf is built for the gap in between: a modern engine, gamepad support, and no desktop required.

## Features

**Rendering**
- Real web rendering via the **Servo** engine (WebRender)
- Runs on **OpenGL ES 3.x**; no X11/Wayland required (works on bare KMS/DRM)
- Single GL context, zero CPU readback — Servo renders straight into the on-screen context

**Gamepad support** (no keyboard needed)
- Virtual **cursor** (left stick / D-pad) that can click page links *and* toolbar buttons
- **On-screen keyboard** with symbols, caps, and shift for typing URLs and searches
- Full-screen **menu** (Select) with **Tabs**, **Bookmarks**, and **History** sections — switch / open / close tabs, and open, delete, or clear saved entries
- Right-stick scroll · A = click/select · B = back / close · L1/R1 = back / forward · L2/R2 = switch tabs · Start = bookmark page · Y = reload

**Platform**
- Minimal **egui** toolbar: address bar, back / forward / reload
- Keyboard, mouse, and gamepad input
- Single self-contained binary

## How it works

SDL2 owns the window and the single GL/GLES context. Servo renders each page into an offscreen framebuffer (FBO) in that context; egui then composites the page texture together with the toolbar and presents the frame via SDL2. Keeping everything on one GLES context — with no compositor and no CPU readback — is what lets it run on bare handheld hardware.

## Building & running

### Desktop (Linux)

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

**Environment variables:**

| Variable | Default | Effect |
|----------|---------|--------|
| `RETSURF_GLES` | `1` | `0` uses desktop OpenGL instead of GLES (debugging) |
| `RETSURF_CONFIG` | — | Path to the config file (overrides the default in the data dir) |
| `RETSURF_LOG_LEVEL` | `info` | Log verbosity (`error`/`warn`/`info`/`debug`/`trace`) |
| `RETSURF_LOG_STYLE` | `always` | Log coloring (`always`/`auto`/`never`) |
| `RETSURF_LOG_FILE` | — | Mirror logs to this file (handheld launchers often discard stderr) |
| `RETSURF_PANIC_FILE` | `retsurf-panic.log` | File for a panic's message + backtrace |
| `SDL_VIDEODRIVER` | auto | SDL video backend (`wayland`/`x11`/`kmsdrm`); auto-set to `wayland` on a Wayland desktop |

retsurf also sets `SURFMAN_FORCE_GLES=1` automatically when GLES is in use (so SDL's
and Servo's GL stacks agree) — you don't normally set it yourself.

## Configuration (`config.toml`)

Settings live in `config.toml` in the user data dir (`SDL_GetPrefPath`, e.g.
`~/.local/share/mxmgorin/retsurf/config.toml` on Linux), or wherever `RETSURF_CONFIG`
points. A template with the defaults is written on first run; missing fields fall back
to their defaults, so a partial file (just one section, or one key) is valid.

```toml
[browser]
home_page = "https://duckduckgo.com"
search_page = "https://duckduckgo.com/?q=%s"   # %s is replaced with the query
experimental_prefs_enabled = true              # enable Servo's experimental web features

[interface]
width = 640
height = 480
use_gles = true            # request an OpenGL ES context (required on Mali handhelds)
cursor_linger_ms = 1500    # how long the gamepad cursor stays visible after moving

[history]
enabled = true             # set false to stop recording (existing entries stay viewable/clearable)
max_entries = 200          # cap on retained entries; oldest are dropped past this

[gamepad]
deadzone = 0.25            # stick deflection below this is treated as centered
cursor_speed = 750.0       # cursor speed at full deflection (logical px/s)
scroll_speed = 1600.0      # scroll speed at full deflection (device px/s)
trigger_threshold = 0.5    # pull above which L2/R2 count as pressed
osk_nav_threshold = 0.5    # stick deflection that counts as an on-screen-keyboard move
osk_nav_initial_delay_ms = 350   # delay before the first auto-repeat of held nav
osk_nav_repeat_ms = 140          # interval between auto-repeats
```

## Status

Experimental. Basic page rendering and navigation work, including full gamepad control on real hardware. WebGL is disabled on devices whose EGL stack is too old for surfman (e.g. EGL 1.4 Mali blobs); everything else renders normally. Many browser features are not yet implemented — contributions and device reports welcome.

## References

- [The Servo Book](https://book.servo.org/title-page.html)
