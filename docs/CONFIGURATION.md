# Configuration

Settings live in `config.toml` in the user data dir (e.g. `~/.local/share/mxmgorin/retsurf/`
on Linux), or wherever `RETSURF_CONFIG` points. A template with the defaults is written on
first run; missing keys take their defaults, so a partial file is valid. Most settings are
also in the in-app settings overlay.

Controls and Game Mode input maps are in [Controls](CONTROLS.md), environment variables in
[Command line](CLI.md#environment-variables).

The data dir also holds history, bookmarks (`bookmarks/`), the saved session, site data
(`servo/`) and caches (`cache/`, safe to delete).

- **Settings > System > Clear browsing data** wipes history, cookies, site storage, the cache,
  the saved session and finished downloads, and closes the tabs. Bookmarks, settings and
  bindings stay. IndexedDB is not cleared yet; delete `servo/clientstorage/` by hand.
- **Settings > System > Restore all defaults** resets settings and bindings, and leaves
  bookmarks, history and tabs alone.

Both take two presses. Rows marked `*` need a restart.

## `config.toml`

```toml
[browser]
search_page = "https://lite.duckduckgo.com/lite/?q=%s"   # %s = the query
# Empty = the default; "desktop", "mobile" (or "android") or "ios" pretend to be that
# browser. "mobile" gets phone layouts, which suit a small screen.
user_agent = ""
persist_site_data = true       # stay logged in across restarts
restore_tabs = true            # reopen last session's tabs
# Open tabs at most, 0 = unlimited. Past it the oldest hidden tab closes. Lower it on
# 1 GB devices.
max_tabs = 8
page_zoom = 1.0                # default page zoom
# "light", "dark" (sites with a dark theme use it) or "forced-dark" (every page inverted,
# photos kept; slower). Reloads the open tabs.
page_theme = "light"

[experimental]
# Web features Servo ships off. The settings overlay's "Web features" row sets them as a
# preset: off, minimal, balanced (the default) or full. On 1 GB devices prefer minimal.
# Applies on the next page load.
columns = true                # CSS multi-column                 minimal
container_queries = true      # CSS @container                   minimal
fontface = true               # web fonts                        minimal
intersection_observer = true  # IntersectionObserver             minimal
resize_observer = true        # ResizeObserver                   minimal
indexeddb = true              # IndexedDB                        minimal
storage_manager = true        # navigator.storage                minimal
webgl2 = true                 # WebGL 2                          balanced
offscreen_canvas = true       # OffscreenCanvas                  balanced
webgpu = false                # WebGPU                           full
notification = false          # Web Notifications                full
async_clipboard = false       # Async Clipboard API              full
permissions = false           # Permissions API                  full

[interface]
scale = 1.0                # UI size, 0.6 to 1.6
toolbar_position = "top"   # "top" or "bottom"
toolbar_autohide = false   # hide on scroll down, show on scroll up
page_icons = true          # site icons in tabs, bookmarks, history and the start page
button_hints = true        # name what the buttons do under the menus, settings and start page
cursor_linger_ms = 1500    # how long the cursor stays visible after moving

[home]
page = "retsurf:home"      # what Home and a new tab open: the start page or any URL
toolbar = false            # the toolbar on the start page
brand = "banner"           # start page header: "banner", "wordmark" or "none"
search = true              # the search field

[home.bookmarks]
filter = "all"             # "all", "sites", "apps", "games" or a tag
sort = "manual"            # "manual", "recent" (last opened first) or "name"

[controls]
deadzone = 0.25            # stick deflection treated as centered
cursor_speed = 600.0       # cursor speed at full deflection
cursor_curve = 2.0         # 1 to 6; 1 = linear, higher = slower small pushes, same top speed
scroll_speed = 1600.0      # scroll speed at full deflection
trigger_threshold = 0.5    # pull at which L2/R2 count as pressed
osk_nav_threshold = 0.5    # stick deflection that moves the on-screen keyboard
osk_nav_initial_delay_ms = 350   # delay before held navigation repeats
osk_nav_repeat_ms = 140          # repeat interval
hold_ms = 400              # press length for a "hold:" gesture
cursor_mode = "mouse"      # stick at startup: "mouse" (cursor) or "scroll"
hint_badges = true         # link hints show button combos; off = move between them
edge_scroll = true         # cursor at a window edge scrolls (not in Game Mode)
pad_layout = "nintendo"    # button labels shown: "nintendo", "xbox" or "playstation"
swap_face_buttons = false  # swap A/B and X/Y, for pads that report them swapped
system_keyboard = true     # Android: show the system keyboard for text fields
haptics = true             # let pages rumble the pad

[osk]
style = "grid"             # "grid" (D-pad over keys) or "wheel" (stick picks, buttons type)
layouts = ["en", "ru"]     # cycled by the Lang key

[performance]
# Memory budget, restart to apply. Lower tiers use less RAM and run slower; auto picks by
# device. micro (~128 MB, Miyoo Mini), embedded (~512 MB), tight (~1 GB), balanced
# (~2 GB), generous (~4 GB), android, desktop.
memory_profile = "auto"
cpu_boost_on_load = false  # full CPU speed while a page loads (needs root)
swap_tuning = false        # zram + swap sysctls on boards up to 1.5 GB (Linux, root); restart
memory_guard = true        # close the heaviest tab when the system runs out of memory (Linux)
memory_guard_floor_mb = 0  # free RAM + swap that trips it, 0 = a fifth of RAM (max 512); restart
memory_guard_pressure = 10 # PSI % that trips it while swapping, 0 = off; restart
layout_threads = 0         # 0 = the memory profile's choice
worker_pool_max = 0        # 0 = the memory profile's choice
http_disk_cache_mb = 0     # keep cached pages on disk across restarts, 0 = off; restart
max_fps = 30               # frame cap for software rendering, 0 = none
webgl_texture_units = 0    # texture units WebGL pages see, 0 = 8 on a software GL; restart
webgl = true               # WebGL for pages; off loads no GL for it; restart

[display]
width = 640                # desktop window size, remembered on exit
height = 480
use_gles = true            # OpenGL ES (required on handheld GPUs)
software_render = false    # render without the GPU (see below)
dark_last_row = false      # black last row, for panels that repeat it at the top

[history]
enabled = true             # false stops recording; existing entries stay
max_entries = 25

[downloads]
dir = ""                   # empty = the system download folder
extensions = ["zip", "7z", "rar", "iso", "chd", "pdf", "gba", "sfc", "nes"]   # downloaded, not opened

[update]
channel = "release"        # "release", "beta" or "nightly" (daily builds from main)
auto_check = true          # check once a day at startup

[adblock]
enabled = true
lists = [                  # EasyList-syntax filter lists
    "https://easylist.to/easylist/easylist.txt",
    "https://easylist.to/easylist/easyprivacy.txt",
]
update_days = 7            # list refresh interval, 0 = never

[data_saving]
block_images = false       # applies on the next load, like the two below
block_media = false
block_fonts = false
max_images_per_page = 0    # stop loading images past this many, 0 = no limit

[audio]
enabled = true             # off silences pages; restart
max_decode_seconds = 300   # longest audio clip a page may decode, 0 = no limit

[video]
enabled = true             # H.264 MP4 video, no streaming sites; off = audio only; restart

[game_mode]
# The map Game Mode uses: "keys", "wasd", "mouse", "gamepad", "none" (input reaches the game as is)
# or the id of an input_maps/<id>.toml (see CONTROLS.md).
input_map = "keys"
patches = true             # built-in fixes for known games (saves of Humble's asm.js ports)

[game_mode.view]
# "none", "fit" (the game's canvas, aspect kept), "integer" (whole multiples, sharp) or
# "stretch".
scaling = "none"
# "none", a built-in or the id of a shaders/<id>.glsl (see SHADERS.md).
shader = "none"
# Rows the shader sees the game in: "auto" (the scale each shader is made for: 3 screen px a
# row for scanlines and grid, 4 for CRT, 5 for SameBoy LCD, the game's own for upscalers),
# "native", "480", "360" or "240".
shader_resolution = "auto"
# Largest texture side a game keeps: "full", "1024", "512" or "256". Larger ones are halved
# until they fit, so a game too big for the GPU memory still loads; reload the game after
# changing it.
texture_limit = "full"
# How they are shrunk: "smooth" (averaged, drawn with linear filtering) or "sharp" (every other
# pixel, drawn as the game asks, for pixel art).
texture_filter = "smooth"
cursor = true              # draw the pad cursor over the game; off for one that draws its own

[debug]
memory_overlay = false     # show the engine's memory use on screen
memory_log = false         # log it every ten seconds
frame_timing = false       # log the time per frame
thread_cpu = false         # log CPU time per thread
```

## Interface scale

The UI is sized to fit the screen, so it looks the same on a handheld and in a desktop
window. `[interface] scale` makes it larger or smaller from there, and pages follow it.

## Software rendering

`software_render` draws everything without the GPU, for devices that have none (the Miyoo
Mini). It needs a build with the `software` feature, which also switches to it on its own
when the GPU cannot be used.

## Bookmarks

Bookmarks are the start page's tiles and the menu's Bookmarks list. Each is one file,
`bookmarks/<id>.toml` in the data dir:

```toml
name = "Wikipedia"                       # shown on the tile; without it, the site
url = "https://en.wikipedia.org"
order = 1                                # position; lower first, unset after the rest
tags = ["reference"]                     # groups for the start page's filter
launch = "browser"                       # "browser", "app" (no toolbar) or "game" (Game Mode too)
reader = false                           # open in reader view; not for a game
last_opened = 2026-10-21T09:12:00Z       # written by retsurf

[game_mode.view]                         # a game's own [game_mode] settings, any key of it
scaling = "fit"
```

An app or a game opens without the toolbar and keeps it hidden through its own links;
Quick Access ends in **Exit**, back to the start page. While a game runs, the `[game_mode]`
settings changed in Quick Access or Settings are written to its bookmark, not to
`config.toml`. A bookmark made in Game Mode is a game.

On the start page L1 / R1 step the filter and Y steps the sort; both are
saved to `[home.bookmarks]`.

The id is the file name and never changes. A file that does not parse is skipped. Removing
a bookmark that has tags or other settings takes a second press.

## Page icons

`page_icons` shows the icon a site declares (not `/favicon.ico`) beside its tab, bookmark and
history row and on its start-page tile; sites without one get a globe. Icons are kept in
`page_icons/` in the data dir for bookmarked, visited and open sites. SVG icons
appear from the second visit in a session.
