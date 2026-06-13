[![Linux ARM](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml/badge.svg)](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml)
[![Windows](https://github.com/mxmgorin/retsurf/actions/workflows/build-windows.yml/badge.svg)](https://github.com/mxmgorin/retsurf/actions/workflows/build-windows.yml)
[![macOS](https://github.com/mxmgorin/retsurf/actions/workflows/build-macos.yml/badge.svg)](https://github.com/mxmgorin/retsurf/actions/workflows/build-macos.yml)
[![Linux](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux.yml/badge.svg)](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux.yml)
[![Dependencies](https://deps.rs/repo/github/mxmgorin/retsurf/status.svg)](https://deps.rs/repo/github/mxmgorin/retsurf)

# 🌊 retsurf

A lightweight, experimental web browser written in **Rust**, using [**Servo**](https://github.com/servo/servo) as the rendering engine, **SDL2** for windowing and input, and **egui** for the UI.

It is designed to run **without X11 or Wayland** — rendering through **OpenGL ES** on bare KMS/DRM — with **gamepad support**, targeting PortMaster-compatible Linux handhelds (**Knulli, muOS, ROCKNIX**) on Mali-class GPUs, as well as regular Linux desktops.

> 🛠️ **Work in progress.** Early development — experimental and bugs are expected.

<!-- TODO: a short demo GIF/video -->

## Why?

On Knulli / muOS / ROCKNIX handhelds there's effectively no way to browse the modern web. Your options are text-era browsers that break on real sites, or full desktop browsers that need a compositor and a keyboard+mouse. retsurf is built for the gap in between: a modern engine, gamepad support, and no desktop required.

## Features

**Gamepad support** (no keyboard needed)
- Virtual **cursor** (left stick / D-pad) that can click page links *and* toolbar buttons
- **Link hints** (Y or L3) — Vimium adapted for a gamepad: clickable elements get highlighted, the stick hops between them spatially, A clicks (hold A / Enter on a link to open it in a background tab)
- **On-screen keyboard** with symbols, caps, shift, and switchable layouts (QWERTY + ЙЦУКЕН built in, picked via config) for typing URLs and searches
- Full-screen **menu** (Select) with **Tabs**, **Bookmarks**, **History**, and **Downloads** sections — switch / open / close tabs, and open, delete, or clear saved entries
- **Rebindable controls** (`bindings.toml`): gamepad gestures (tap, hold, two-button chords) and keyboard shortcuts over the same actions, plus a D-pad cursor to scroll toggle for devices without analog sticks
- Defaults: right-stick scroll · A = click/select · B = back / close · X = keyboard (hold: reader mode) · Y = link hints (hold: D-pad scroll toggle) · L1/R1 = back / forward (hold: zoom out / in) · L2/R2 = switch tabs · L3 = link hints · R3 = reader mode · Start = reload (hold: bookmark) · Select = menu

**Page zoom**
- Real zoom (reflows the layout, not a magnifier), stepping Firefox's 50–300% ladder, per tab
- `[browser] page_zoom` in the config scales every tab by default — set `1.25` once and the whole web fits a small screen better
- Hold R1/L1, `ctrl+=`/`ctrl+-`/`ctrl+0`, or the bindable `zoom_in`/`zoom_out`/`zoom_reset` actions

**Reader mode**
- Strip a page down to its article (Mozilla's [Readability](https://github.com/mozilla/readability), the Firefox Reader View engine) with a dark, narrow-column layout sized for small screens
- Runs in place — no refetch, so it works on logged-in and dynamic pages; toggling off reloads
- Toggle via the 🖹 toolbar button, R3 (or hold X on stickless devices), `ctrl+e`, or the bindable `reader` action

**Downloads**
- Navigating to a file link downloads it in the background instead of rendering it
- Progress, cancel, and history of finished downloads in the menu's **Downloads** section; a ⬇ toolbar chip shows what's in flight
- Saves into the system download folder (`XDG_DOWNLOAD_DIR` / `~/Downloads`) or any configured directory

**Ad blocking**
- Network-level ad & tracker blocking with [Brave's adblock-rust](https://github.com/brave/adblock-rust) engine (EasyList + EasyPrivacy by default)
- Filter lists are fetched in the background, compiled, and cached locally — warm starts are instant and work offline
- Fully configurable: toggle it off, change the lists, or change the refresh interval

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

**Environment variables:**

| Variable | Default | Effect |
|----------|---------|--------|
| `RETSURF_GLES` | `1` | `0` uses desktop OpenGL instead of GLES (debugging) |
| `RETSURF_CONFIG` | — | Path to the config file (overrides the default in the data dir) |
| `RETSURF_DATA_DIR` | — | Override the user data dir (config, history, bookmarks, plus `servo/` for cookies and `cache/` for the adblock engine) — created on demand; useful for portable installs or separate profiles |
| `RETSURF_LOG_LEVEL` | `info` | Log verbosity (`error`/`warn`/`info`/`debug`/`trace`) |
| `RETSURF_LOG_STYLE` | `always` | Log coloring (`always`/`auto`/`never`) |
| `RETSURF_LOG_FILE` | — | Write logs to this file |
| `RETSURF_PANIC_FILE` | `retsurf-panic.log` | File for a panic's message + backtrace |
| `SDL_VIDEODRIVER` | auto | SDL video backend (`wayland`/`x11`/`kmsdrm`); auto-set to `wayland` on a Wayland desktop |

retsurf also sets `SURFMAN_FORCE_GLES=1` automatically when GLES is in use (so SDL's
and Servo's GL stacks agree) — you don't normally set it yourself.

## Configuration (`config.toml`)

Settings live in `config.toml` in the user data dir (`SDL_GetPrefPath`, e.g.
`~/.local/share/mxmgorin/retsurf/config.toml` on Linux), or wherever `RETSURF_CONFIG`
points. A template with the defaults is written on first run; missing fields fall back
to their defaults, so a partial file (just one section, or one key) is valid.

The data dir keeps retsurf's own files (`config.toml`, `history.toml`, `bookmarks.toml`)
at its root, with Servo's site data (cookies, localStorage, HSTS) under `servo/` and
regenerable caches (the adblock engine) under `cache/` — the latter is safe to delete.

```toml
[browser]
home_page = "https://duckduckgo.com"
search_page = "https://duckduckgo.com/?q=%s"   # %s is replaced with the query
experimental_prefs_enabled = true              # enable Servo's experimental web features
# The User-Agent sites see. Empty = Servo's platform default. The keywords
# "desktop", "mobile" (or "android"), and "ios" pick a stock UA — "mobile"
# makes sites serve their phone layouts, which fit a small screen far better;
user_agent = ""
# Keep site data (cookies, localStorage, HSTS) across restarts so logins
# survive. Stored in the data dir's servo/ subfolder; false = in-memory only, gone on exit.
persist_site_data = true
# Default page zoom for every tab (1.0 = 100%). Real zoom — it reflows the
# layout — so 1.25 makes the whole web bigger on a small screen. zoom_in /
# zoom_out step a Firefox-style ladder from here, zoom_reset returns.
page_zoom = 1.0

[interface]
width = 640
height = 480
use_gles = true            # request an OpenGL ES context (required on Mali handhelds)
cursor_linger_ms = 1500    # how long the gamepad cursor stays visible after moving

[osk]
# Built-in on-screen-keyboard layouts to enable; the keyboard's Lang key cycles
# them in this order. Available: "en" (QWERTY), "ru" (ЙЦУКЕН). Unknown names are
# logged and skipped; an empty list falls back to ["en"].
layouts = ["en", "ru"]

[performance]
# Servo thread counts. The defaults (0 = auto) size everything from the CPU core
# count, which matters on 4-core handhelds where Servo's desktop defaults
# oversubscribe the cores. Set explicit values to override.
layout_threads = 0         # Stylo/layout threads; auto = cores - 2, clamped to 1..4
worker_pool_max = 0        # cap per worker pool (image cache, async runtime, storage,
                           # WebRender); auto = half the cores, at least 2

[history]
enabled = true             # set false to stop recording (existing entries stay viewable/clearable)
max_entries = 25           # cap on retained entries; oldest are dropped past this

[downloads]
# Where files are saved. Empty picks the system download folder (XDG_DOWNLOAD_DIR /
# ~/Downloads) when it exists, otherwise downloads/ in the user data dir. Point it
# at the SD card on a handheld, e.g. "/userdata/roms".
dir = ""
# URL path extensions treated as downloads when navigated to (navigation is
# cancelled and the file is fetched in the background instead). URLs without a
# listed extension load in the browser as usual.
extensions = ["zip", "7z", "rar", "iso", "chd", "pdf", "gba", "sfc", "nes"]

[adblock]
enabled = true             # master switch for ad & tracker blocking
lists = [                  # filter lists (EasyList syntax) compiled into the engine
    "https://easylist.to/easylist/easylist.txt",
    "https://easylist.to/easylist/easyprivacy.txt",
]
update_days = 7            # re-download lists when the cached engine is older; 0 = never

[gamepad]
deadzone = 0.25            # stick deflection below this is treated as centered
cursor_speed = 750.0       # cursor speed at full deflection (logical px/s)
scroll_speed = 1600.0      # scroll speed at full deflection (device px/s)
trigger_threshold = 0.5    # pull above which L2/R2 count as pressed
osk_nav_threshold = 0.5    # stick deflection that counts as an on-screen-keyboard move
osk_nav_initial_delay_ms = 350   # delay before the first auto-repeat of held nav
osk_nav_repeat_ms = 140          # interval between auto-repeats
hold_ms = 400              # holding a button this long fires its "hold:" gesture
```

## Bindings (`bindings.toml`)

Gamepad and keyboard layouts live in their own file, `bindings.toml`, next to
`config.toml` (a template with the defaults is written on first run). Each
entry maps a *gesture* to an *action*:

```toml
[gamepad]
a = "confirm"              # tap: fires on press
"hold:start" = "bookmark"  # hold the button for hold_ms
"l1+r1" = "reload"         # chord: press one while holding the other
y = "none"                 # explicitly unbind

[keyboard]
"ctrl+r" = "reload"        # modifier shortcuts always fire
f = "hints"                # plain keys fire only while no text input has focus
k = "nav_up"               # overlay navigation can move to vim-style keys
```

**Gamepad gestures**: a tap (`a`), a hold (`"hold:a"`), or a button chord
(`"a+b"`). Buttons: `a b x y l1 r1 l3 r3 start select` (the D-pad aims the
cursor and L2/R2 cycle tabs / drive the keyboard — they're not bindable). A
button with a hold or chord gesture fires its tap on release instead of press
(the gesture is ambiguous until then); `confirm` needs the press edge for
clicks and drags, so hold/chord gestures on its button are rejected.

**Keyboard shortcuts**: any key with optional `ctrl`/`alt`/`shift` modifiers,
matched strictly. Plain keys (no Ctrl/Alt) are muted whenever a text input —
on the page or the address bar — holds focus, so they can't hijack typing.
Defaults: `ctrl+r` reload · `ctrl+b` bookmark · `ctrl+e` reader mode ·
`ctrl+m` menu · `ctrl+left`/`ctrl+right` back/forward · `ctrl+f` link hints ·
`ctrl+t`/`ctrl+shift+t` next/previous tab · `ctrl+=`/`ctrl+-`/`ctrl+0`
zoom in/out/reset · arrows = overlay navigation.

**Actions**: `confirm` (click/select) · `cancel` (close/back) · `osk`
(on-screen keyboard) · `reload` · `prev` / `next` (menu section or history) ·
`hints` (link hints) · `bookmark` · `reader` (reader mode) · `menu` ·
`tab_next` / `tab_prev` ·
`zoom_in` / `zoom_out` / `zoom_reset` (page zoom along a Firefox-style 50–300%
ladder / back to the config default) ·
`nav_up` / `nav_down` / `nav_left` / `nav_right` (one step in whatever overlay
is open — menu, on-screen keyboard, or link hints; with none open the key goes
to the page) · `scroll` (gamepad-only: toggle the D-pad / left stick between
cursor and page scroll — the scroll fallback for devices without a right
analog stick) · `none`.

Invalid buttons, keys, actions, or gestures are logged and skipped at startup —
check the log if a binding doesn't respond.

## References

- [Handheld notes](docs/HANDHELD_PORT.md) — how it works, architecture, porting status
- [The Servo Book](https://book.servo.org/title-page.html)
