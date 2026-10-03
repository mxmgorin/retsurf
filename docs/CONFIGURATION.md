# Configuration

retsurf reads its settings from `config.toml` in the user data dir. A template with the
defaults is written on first run, and most settings are also editable in-app from the
settings overlay. Gamepad and keyboard mappings and Game Mode input maps are in
[Controls](CONTROLS.md); environment variables in [Command line](CLI.md#environment-variables).

## `config.toml`

Settings live in `config.toml` in the user data dir (`SDL_GetPrefPath`, e.g.
`~/.local/share/mxmgorin/retsurf/config.toml` on Linux), or wherever `RETSURF_CONFIG`
points. A template with the defaults is written on first run; missing fields fall back
to their defaults, so a partial file (just one section, or one key) is valid.

The data dir keeps retsurf's own files (`config.toml`, `history.toml`, `bookmarks.toml`,
`session.toml`) at its root, with Servo's site data (cookies, localStorage, HSTS) under
`servo/` and regenerable caches (the adblock engine) under `cache/` — the latter is safe
to delete.

**Settings > System > Clear browsing data** wipes history, cookies, localStorage, the
HTTP cache, the saved tab session and the finished downloads, and closes the open tabs
back to the home page. Two presses: the first arms the row, the second clears. Bookmarks,
speed-dial pins, settings and bindings are left alone. **IndexedDB is not cleared**: the
engine has no category for it yet, so a site's databases under `servo/clientstorage/`
survive until that directory is deleted by hand.

**Settings > System > Restore all defaults** is the other half: every settings row, the
speed-dial pins and the control bindings go back to how they ship, and nothing you saved
(bookmarks, history, tabs) is touched. Two presses as well. The settings and bindings are
written when the overlay closes, the pins right away; rows marked `*` need a restart.

```toml
[browser]
home_page = "retsurf:home"                     # built-in start page; or any URL
# %s is replaced with the query. The default is DuckDuckGo's no-JS endpoint: it
# renders the results server-side, so they show without running a script.
search_page = "https://lite.duckduckgo.com/lite/?q=%s"
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
# How many tabs may be open at once; 0 = unlimited. Every tab is a live Servo
# webview with no way to suspend it, so a few heavy pages can exhaust a 1 GB
# board — lower this there. Opening past the cap closes the oldest tab that
# isn't in view (1 therefore replaces); a tab the *page* opens (window.open,
# target="_blank") is refused at the cap instead of evicting one of yours.
max_tabs = 8
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
indexeddb = true              # IndexedDB (apps and games store here) — essential
storage_manager = true        # navigator.storage: quota, persistence       — essential
webgl2 = true                 # WebGL 2.0 (GLES 3.0-class 3D)        — balanced+
offscreen_canvas = true       # OffscreenCanvas (canvas off-thread)  — balanced+
webgpu = false                # WebGPU (next-gen GPU API)            — full only
notification = false          # Web Notifications                    — full only
async_clipboard = false       # Async Clipboard API                  — full only
permissions = false           # Permissions API                      — full only

[interface]
scale = 1.0                # UI zoom, as a factor over the fit to the panel (see below)
toolbar_position = "top"   # which edge the toolbar sits on: "top" or "bottom"
toolbar_autohide = false   # hide on scroll down, reveal on scroll up (floats over the page, either edge)
home_style = "banner"      # start page header: "banner", "wordmark", or "compact" (none, more room for the dial)
page_icons = true          # site icons on tabs, bookmarks, history and the speed dial (see below)
cursor_linger_ms = 1500    # how long the cursor stays visible after moving

[controls]
deadzone = 0.25            # stick deflection below this is treated as centered
cursor_speed = 600.0       # cursor speed at full deflection (logical px/s)
scroll_speed = 1600.0      # scroll speed at full deflection (device px/s)
trigger_threshold = 0.5    # pull above which L2/R2 count as pressed
osk_nav_threshold = 0.5    # stick deflection that counts as an on-screen-keyboard move
osk_nav_initial_delay_ms = 350   # delay before the first auto-repeat of held nav
osk_nav_repeat_ms = 140          # interval between auto-repeats
hold_ms = 400              # holding a button this long fires its "hold:" gesture
cursor_mode = "mouse"      # default D-pad/stick mode at startup: "mouse" or "scroll"
edge_scroll = true         # pushing the cursor against a window edge scrolls that way (never in Game Mode)
haptics = true             # let a page rumble the pad (the Gamepad vibration API)

[osk]
# Built-in on-screen-keyboard layouts to enable; the keyboard's Lang key cycles
# them in this order. Available: "en" (QWERTY), "ru" (ЙЦУКЕН). Unknown names are
# logged and skipped; an empty list falls back to ["en"].
# The Fn key is not a layout: it swaps any of them for Escape, F1-F12 and the
# navigation keys, which a character grid cannot carry.
layouts = ["en", "ru"]

[performance]
# Memory/performance tier for the Servo engine. Each profile bundles a coordinated
# set of engine prefs — JS heap/GC ceilings, back-forward-cache depth, HTTP cache,
# subpixel AA, thread counts, and which DOM subsystems even start — so lower tiers
# use less RAM at some cost to speed (important on unified-memory handhelds, where
# the GPU draws from the same pool). One of:
#   auto      pick a tier from the build target + detected RAM (the default)
#   micro     ~128 MB boards (Miyoo Mini): embedded with a quarter of the JS heap,
#             no slack before a collection and no page kept alive for back
#   embedded  ~512 MB / sub-1 GB boards: baseline JIT only, single-threaded, no caches
#   tight     ~1 GB boards (RK3326, H700): baseline JIT only, small caches
#   balanced  ~2 GB boards (RK3566, A527): modest parallelism, full JIT
#   generous  ~4 GB handhelds (A527): higher GC ceiling, deeper history, full JIT
#   android   Android phones/tablets (>3 GB): full JIT, more threads, eager mem return
#   desktop   Servo's own defaults, untouched — unlimited JS heap, auto-scaled threads
# `auto` resolves to: android build -> android; windows/macos -> desktop; Linux with
# >6 GB -> desktop; otherwise by RAM (from /proc/meminfo). Changing it needs a restart.
memory_profile = "auto"
# `RETSURF_MAIN_NICE=<n>` reprioritizes the main thread (compositing and input)
# against the engine's. Off by default: measured on a Miyoo Flip it earns nothing,
# because two cores at 21% utilisation are not contended and priority only decides
# who waits. Worth up to -38% on frame cost once the cores *are* saturated, which
# is why the knob exists. A negative value needs root or CAP_SYS_NICE; without
# either it is skipped with a log line.
# Hold the CPU's `performance` governor while a page loads, then put the old one
# back. The kernel's own governor ramps too late for a load burst: measured on a
# Miyoo Flip, `performance` is worth -16% page time and -11% CPU, and confining it
# to loads keeps the idle clock (and the battery) where it was. Needs a writable
# `scaling_governor`, so root — the handheld launchers have it, a desktop does
# not, and without it this goes inert after one attempt with a log line. Applies
# live; also in the settings overlay (System -> Performance).
cpu_boost_on_load = false
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
# (System tab, "HTTP disk cache (MB)"); 0 shows there as "Off".
http_disk_cache_mb = 0
max_fps = 30               # frame cap for the software renderer, which nothing else paces (0 = uncapped)

[display]
width = 640                # size the window opens at, and where it is left on exit
height = 480               # (desktop only: a handheld's window is its panel)
use_gles = true            # request an OpenGL ES context (required on Mali handhelds)
software_render = false    # draw everything on the CPU, with no GL at all (see below)
dark_last_row = false      # paint the screen's last row black, for panels that show it again as the first

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
# Which builds the in-app updater checks for (also selectable in Settings > System
# > Updates). One of:
#   release  tagged GitHub releases, stable only (the default)
#   beta     tagged releases including pre-releases (highest semver wins)
#   nightly  the rolling "nightly" pre-release, rebuilt from main once a day. Its
#            tag carries no version, so this channel compares the commit the build
#            records instead. Unsigned builds straight off main — expect bugs.
#            The old spelling "ci" still parses and means this.
channel = "release"
auto_check = true          # throttled background check at startup (once/day); false = About tab only

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

[game_mode]
# Game Mode hands the input to the page and hides the chrome, so a web game gets
# the keys and buttons the browser would otherwise take. The `quick_access` binding
# (Start on the pad, Ctrl+Alt+G) opens Quick Access, a strip at the right edge,
# and `menu` (Select, Ctrl+M) opens Quick Menu at the left, in or out of the
# mode. Inside it both pad gestures are mirrored as holds (a tap would take the
# button from the game; a held one hands its press over on release), so
# rebinding them moves the way out with them. Quick Access is the way in and out
# (Enter game mode, or Exit game mode last); inside the mode it also steps the
# View and the input map live and summons the on-screen keyboard over the game.
# Over a game Quick Menu holds only Settings and Quit; Settings opens on the
# Gaming tab, where the Input map row picks the map (written back here).
# Which input map drives the pad and the keyboard while the mode is on: "none"
# for no map at all (the pad and the keyboard reach the game as they are; an id
# no map answers to is none too), a built-in ("keys", "wasd" or "mouse"), or the
# stem of an input_maps/<id>.toml of your own.
# See CONTROLS.md for the input map format.
input_map = "keys"

[game_mode.view]
# How the page's game is shown while the mode is on: "off" leaves the page as it
# is; "fit" cuts the biggest canvas or iframe out of the page over a black
# backdrop, as large as its aspect ratio allows; "integer" does the same at the
# largest whole multiple of its pixels, drawn unsmoothed; "stretch" fills the
# screen, aspect ratio ignored. Undone on leaving the mode, kept across pages.
scaling = "off"
```

## Interface scale

The chrome is drawn against a 640x480 design and zoomed to fit the panel it is
on, so a toolbar keeps its size in thumbs rather than in pixels. A fit within a
quarter of a whole number is rounded down to it — fractional zoom lands glyphs
between pixels, and the spare pixels widen the page instead. `[interface].scale`
is a factor over that fit (0.6 to 1.6), so one setting means the same thing on a
handheld and on a desktop window. The page follows the same zoom as its device
pixel ratio, which keeps a CSS pixel and a chrome point the same size.

On 640x480 and 752x560 panels the fit is 1.0, so nothing changes there; a
desktop window is where the zoom is visibly above one. `RETSURF_SCALE` replaces
the fit where the launcher knows better — Android sets it to the display
density, which a resolution alone cannot tell.

## Software rendering

`software_render` swaps both renderers for CPU ones: the page is rasterized by
swgl (WebRender's own software backend) and the chrome is drawn by SDL's 2D
renderer, so nothing needs a GL driver. It exists for devices that have no GPU —
the Miyoo Mini family — and needs a build with the `software` cargo feature; a
build without it logs a warning and stays on GL. Builds that do have it fall back
to software on their own when no GL context can be created, so the switch is only
for forcing it on a machine that has both. [`RENDERING.md`](RENDERING.md) has how
it works.

## Page icons

`page_icons` puts each site's icon beside its tab, bookmark and history row and
on its speed-dial tile. The icon is the one a page names with `<link rel="icon">`;
`/favicon.ico` is not tried, so a site without the tag gets a globe (a letter on
the dial). Nothing extra is fetched: the engine downloads the icon either way.

Icons are shrunk to 32 px and kept as one PNG per site in the data dir's
`page_icons/` folder, so the lists show them without the page open. A dark icon
is drawn on a light plate in the lists and on a light tile on the speed dial,
where other tiles take a muted shade of their icon's main colour. The folder is
trimmed at startup to the sites the bookmarks, pins, history and saved tabs still
name. With `[history] enabled = false`, only bookmarked and pinned sites are
stored. **Clear browsing data** and clearing the history drop the icons that
only the history referenced. Turning the setting off frees the icons in memory
and leaves the folder alone.

SVG icons currently show only from the second visit in a session: the engine
does not report them until their raster is cached.
