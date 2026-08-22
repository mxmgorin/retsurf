# Configuration & bindings

retsurf reads two TOML files from the user data dir: `config.toml` (settings) and
`bindings.toml` (gamepad/keyboard mappings). Templates with the defaults are written
on first run. Most settings are also editable in-app from the ⚙ settings overlay.
A handful of [environment variables](#environment-variables) override paths and
control logging at launch.

## Configuration (`config.toml`)

Settings live in `config.toml` in the user data dir (`SDL_GetPrefPath`, e.g.
`~/.local/share/mxmgorin/retsurf/config.toml` on Linux), or wherever `RETSURF_CONFIG`
points. A template with the defaults is written on first run; missing fields fall back
to their defaults, so a partial file (just one section, or one key) is valid.

The data dir keeps retsurf's own files (`config.toml`, `history.toml`, `bookmarks.toml`,
`session.toml`) at its root, with Servo's site data (cookies, localStorage, HSTS) under
`servo/` and regenerable caches (the adblock engine) under `cache/` — the latter is safe
to delete.

```toml
[browser]
home_page = "retsurf:home"                     # built-in start page; or any URL
search_page = "https://duckduckgo.com/?q=%s"   # %s is replaced with the query
# The User-Agent sites see. Empty = Servo's platform default. The keywords
# "desktop", "mobile" (or "android"), and "ios" pick a stock UA — "mobile"
# makes sites serve their phone layouts, which fit a small screen far better;
user_agent = ""
# Keep site data (cookies, localStorage, HSTS) across restarts so logins
# survive. Stored in the data dir's servo/ subfolder; false = in-memory only, gone on exit.
persist_site_data = true
# Reopen the tabs that were open when you last quit, instead of starting on
# home_page. The list is kept in session.toml in the data dir, rewritten as the
# tabs change and at exit; turning this off deletes it. Tabs restore as their
# URLs — page scroll position and form contents are not part of the session.
restore_tabs = true
# Default page zoom for every tab (1.0 = 100%). Real zoom — it reflows the
# layout — so 1.25 makes the whole web bigger on a small screen. zoom_in /
# zoom_out step a Firefox-style ladder from here, zoom_reset returns.
page_zoom = 1.0
# How page content is themed; the app's own chrome is dark either way. Changing
# it reloads the open tabs, which is what makes the change take effect.
#   "light"       what every browser reports by default
#   "dark"        report prefers-color-scheme: dark — only sites that ship a
#                 dark theme react, the rest stay light
#   "forced-dark" invert every page, so sites without a dark theme get one too.
#                 Costs a full-page filter pass per frame (measure before using
#                 it on a handheld); photos are inverted back, CSS background
#                 images are not.
page_theme = "light"

[experimental]
# Servo experimental web-platform features. These are standard but not yet stable
# in Servo (it ships them off); most of the modern web needs them, so retsurf turns
# them on. In the settings overlay (Browser tab) the "Web features" row is a preset
# that sets the whole group at once — off / minimal / balanced / full — and shows
# "custom" once you hand-toggle any one below. The default is "balanced": the layout
# and compatibility essentials plus the graphics the handheld's GPU supports (WebGL2,
# OffscreenCanvas). "minimal" is the essentials only; "full" adds WebGPU (immature,
# heavy) and the niche APIs below. Each key can be set individually; changes apply on
# the next page load. This preset is the sole authority for WebGL2/WebGPU — the memory
# profile no longer gates them, so on <=1 GB boards prefer "minimal" to avoid WebGL2/
# OffscreenCanvas drawing from the shared GPU/RAM pool.
grid = true                   # CSS Grid (display: grid)             — essential
columns = true                # CSS multi-column                     — essential
container_queries = true      # CSS container queries (@container)   — essential
fontface = true               # web fonts (@font-face / FontFace)    — essential
intersection_observer = true  # IntersectionObserver (lazy-load)     — essential
resize_observer = true        # ResizeObserver                       — essential
webgl2 = true                 # WebGL 2.0 (GLES 3.0-class 3D)        — balanced+
offscreen_canvas = true       # OffscreenCanvas (canvas off-thread)  — balanced+
webgpu = false                # WebGPU (next-gen GPU API)            — full only
notification = false          # Web Notifications                    — full only
async_clipboard = false       # Async Clipboard API                  — full only
permissions = false           # Permissions API                      — full only

[display]
width = 640
height = 480
use_gles = true            # request an OpenGL ES context (required on Mali handhelds)
cursor_linger_ms = 1500    # how long the cursor stays visible after moving
toolbar_position = "top"   # which edge the toolbar sits on: "top" or "bottom"
toolbar_autohide = false   # hide on scroll down, reveal on scroll up (top reflows, bottom overlays)

[osk]
# Built-in on-screen-keyboard layouts to enable; the keyboard's Lang key cycles
# them in this order. Available: "en" (QWERTY), "ru" (ЙЦУКЕН). Unknown names are
# logged and skipped; an empty list falls back to ["en"].
layouts = ["en", "ru"]

[performance]
# Memory/performance tier for the Servo engine. Each profile bundles a coordinated
# set of engine prefs — JS heap/GC ceilings, back-forward-cache depth, HTTP cache,
# subpixel AA, thread counts, and which DOM subsystems even start — so lower tiers
# use less RAM at some cost to speed (important on unified-memory handhelds, where
# the GPU draws from the same pool). One of:
#   auto      pick a tier from the build target + detected RAM (the default)
#   embedded  ~512 MB / sub-1 GB boards: baseline JIT only, single-threaded, no caches
#   tight     ~1 GB boards (RK3326, H700): baseline JIT only, small caches
#   balanced  ~2 GB boards (RK3566, A527): modest parallelism, full JIT
#   generous  ~4 GB handhelds (A527): higher GC ceiling, deeper history, full JIT
#   android   Android phones/tablets (>3 GB): full JIT, more threads, eager mem return
#   desktop   Servo's own defaults, untouched — unlimited JS heap, auto-scaled threads
# `auto` resolves to: android build -> android; windows/macos -> desktop; Linux with
# >6 GB -> desktop; otherwise by RAM (from /proc/meminfo). Changing it needs a restart.
memory_profile = "auto"
# Servo thread counts. 0 = keep the memory profile's choice; a non-zero value
# overrides it (handy to fine-tune a tier without switching profiles).
layout_threads = 0         # Stylo/layout threads
worker_pool_max = 0        # cap applied to every worker pool (image cache, async
                           # runtime, storage, WebRender)
# Servo's on-disk HTTP cache, in MB; 0 (the default) is off. It is a spill store for
# the in-memory cache, not a second level: an entry the memory cache evicts is written
# to `cache/http-cache.sqlite3`, and a hit moves it back into memory and off disk. So
# it widens the cache and keeps whatever spilled across a restart, but it is not a
# durable archive of visited pages. Every spill is a write to the SD card, which is why
# it is opt-in; on `embedded`/`tight` (which switch the memory cache off, leaving
# nothing to spill) turning this on also revives a 16-entry memory cache. Needs a
# restart. Safe to delete the file at any time. Also in the settings overlay
# (Advanced tab, "HTTP disk cache (MB)"); 0 shows there as "Off".
http_disk_cache_mb = 0

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
# listed extension load in the browser as usual. Files a page builds in
# JavaScript (a "Generate & Download" button: fetch, then a blob URL) don't go
# through a navigation at all and are captured separately — no configuration,
# see src/browser/blob_download.rs.
extensions = ["zip", "7z", "rar", "iso", "chd", "pdf", "gba", "sfc", "nes"]

[update]
# Which builds the in-app updater checks for (also selectable in Settings > Advanced
# > Updates). One of:
#   release  tagged GitHub releases, stable only (the default)
#   beta     tagged releases including pre-releases (highest semver wins)
#   ci       per-commit main CI artifacts (dev/nightly) — needs a GitHub token
channel = "release"
auto_check = true          # throttled background check at startup (once/day); false = About tab only
# GitHub token (fine-grained PAT with actions:read) for the "ci" channel only —
# GitHub requires auth to download Actions artifacts. Prefer the RETSURF_GITHUB_TOKEN
# env var over writing a secret to disk; this field is a fallback. Unused by release/beta.
token = ""

[adblock]
enabled = true             # master switch for ad & tracker blocking
lists = [                  # filter lists (EasyList syntax) compiled into the engine
    "https://easylist.to/easylist/easylist.txt",
    "https://easylist.to/easylist/easyprivacy.txt",
]
update_days = 7            # re-download lists when the cached engine is older; 0 = never

[data_saving]
# Lightweight mode: skip whole subresource categories at the network level (like
# the ad blocker) to save bandwidth and memory. All apply live on the next load.
block_images = false       # skip <img>, CSS backgrounds, favicons
block_media = false        # skip audio/video/track loads
block_fonts = false        # skip web-font downloads (fall back to system fonts)
# Cap on distinct images per page (0 = unlimited). Servo loads/decodes every image
# eagerly (no lazy-loading), so a client-rendered grid of hundreds of thumbnails
# can freeze a handheld. Past the cap, image loads are soft-blocked; the count
# resets each navigation. Off by default — an ordinary page carries 40-90 images,
# so a cap that helps a thumbnail grid mostly just guts normal sites. Set it on
# weak boards, or when a specific page stalls.
max_images_per_page = 0

[audio]
# Audio output. retsurf renders the Web Audio graph itself and plays it through
# SDL2, so oscillators, gain, filters, panners, analysers and JS-filled AudioBuffers
# all make sound, decodeAudioData() decodes mp3/wav/flac/ogg-vorbis/aac, and <audio>
# elements play the same formats (progressive files only - no streaming/MSE, no Opus).
# <video> stays silent; a video file in an <audio>-style load plays its audio track.
# Read once at startup (restart to apply). Off means no audio device is ever opened;
# <audio> reports "can't play" so pages take their no-audio fallback, and
# decodeAudioData() still works.
enabled = true
# Longest clip decodeAudioData() will decode, in seconds (0 = unlimited). A decoded
# clip costs seconds * rate * channels * 4 bytes in memory, so a 5 min stereo track is
# ~106 MB - briefly twice that if the file needs resampling to the context rate - and
# an hour-long file would exhaust a 1 GB board. Past the cap the promise is rejected.
# Lower it on weak boards; raise it if a page needs long tracks.
max_decode_seconds = 300

[video]
# <video> playback: H.264-in-MP4 files, decoded in software (OpenH264) and synced
# to the audio track. Still no MSE, so streaming sites (YouTube etc.) stay dead;
# this covers direct .mp4 files and embeds. Decoding is CPU-bound - a weak board
# that cannot keep up drops video to a slideshow while audio stays smooth, and
# turning this off makes video files play audio-only (the pre-0.6 behavior).
# Read once at startup (restart to apply).
enabled = true

[input]
deadzone = 0.25            # stick deflection below this is treated as centered
cursor_speed = 600.0       # cursor speed at full deflection (logical px/s)
scroll_speed = 1600.0      # scroll speed at full deflection (device px/s)
trigger_threshold = 0.5    # pull above which L2/R2 count as pressed
osk_nav_threshold = 0.5    # stick deflection that counts as an on-screen-keyboard move
osk_nav_initial_delay_ms = 350   # delay before the first auto-repeat of held nav
osk_nav_repeat_ms = 140          # interval between auto-repeats
hold_ms = 400              # holding a button this long fires its "hold:" gesture
cursor_mode = "mouse"      # default D-pad/stick mode at startup: "mouse" or "scroll"
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
`settings` (settings overlay; pressed again while it's open, quits) · `home`
(go to the home page) · `quit` (quit the app) · `tab_next` / `tab_prev` ·
`zoom_in` / `zoom_out` / `zoom_reset` (page zoom along a Firefox-style 50–300%
ladder / back to the config default) ·
`nav_up` / `nav_down` / `nav_left` / `nav_right` (one step in whatever overlay
is open — menu, on-screen keyboard, or link hints; with none open the key goes
to the page) · `scroll` (gamepad-only: toggle the D-pad / left stick between
cursor and page scroll — the scroll fallback for devices without a right
analog stick) · `none`.

Invalid buttons, keys, actions, or gestures are logged and skipped at startup —
check the log if a binding doesn't respond.

## Environment variables

Set at launch; they override paths and control logging without touching the config
files.

| Variable | Default | Effect |
|----------|---------|--------|
| `RETSURF_GLES` | `1` | `0` uses desktop OpenGL instead of GLES (debugging) |
| `RETSURF_CONFIG` | — | Path to the config file (overrides the default in the data dir) |
| `RETSURF_DATA_DIR` | — | Override the user data dir (config, history, bookmarks, plus `servo/` for cookies and `cache/` for the adblock engine) |
| `RETSURF_DOWNLOAD_DIR` | — | Override where downloads are saved (created on demand). Takes precedence over the system download folder; the `[downloads].dir` config setting still wins over it. Falls back to `downloads/` in the data dir |
| `RETSURF_LOG_LEVEL` | `info` | Log verbosity (`error`/`warn`/`info`/`debug`/`trace`) |
| `RETSURF_LOG_STYLE` | `always` | Log coloring (`always`/`auto`/`never`) |
| `RETSURF_LOG_FILE` | — | Write logs to this file |
| `RETSURF_PANIC_FILE` | `retsurf-panic.log` | File for a panic's message + backtrace |
| `SDL_VIDEODRIVER` | auto | SDL video backend (`wayland`/`x11`/`kmsdrm`); auto-set to `wayland` on a Wayland desktop |

retsurf also sets `SURFMAN_FORCE_GLES=1` automatically when GLES is in use (so SDL's
and Servo's GL stacks agree) — you don't normally set it yourself.
