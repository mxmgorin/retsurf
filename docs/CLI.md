# Command line

```
retsurf [--game-mode] [PATH]
```

| Argument | Effect |
| --- | --- |
| `PATH` | Open a local game folder (its `index.html`) or an HTML file. |
| `--game-mode` | Start in Game Mode, before the page loads. |
| `-h`, `--help` / `-V`, `--version` | Print the usage or the version. |

The folder is served as `http://<folder-name>.localhost/`, with its own saves
(localStorage, IndexedDB). Folders whose names differ only in case or punctuation
share them. `.br` and `.gz` files are decompressed.

## Examples

```sh
# a game folder: opens its index.html
retsurf ~/games/hexgl

# the same, straight into Game Mode
retsurf --game-mode ~/games/hexgl

# an HTML file inside a folder
retsurf ~/games/carts/cart.html

# Game Mode over the usual first tabs
retsurf --game-mode
```

A PATH starting with `-` goes after `--`: `retsurf -- -name/`. A bad PATH or an
unknown option prints the usage and exits with status 64.

## Environment variables

Set at launch; they override paths and control logging without touching the config
files.

| Variable | Default | Effect |
|----------|---------|--------|
| `RETSURF_GLES` | `1` | `0` uses desktop OpenGL instead of GLES (debugging) |
| `RETSURF_SCALE` | — | Pin the UI zoom the panel would otherwise be fitted to; `[interface].scale` still multiplies it. Set by the Android launcher to the display density |
| `RETSURF_SOFTWARE` | `0` | `1` forces CPU rendering (`[display].software_render`) |
| `RETSURF_MAX_FPS` | — | Overrides `[performance].max_fps`, the cap the software renderer is paced by (`0` uncapped) |
| `RETSURF_KEYMAP` | auto | `miyoo` reads the pad from the keys that firmware's SDL2 sends instead of a controller, `desktop` never does; detected from the video driver otherwise |
| `RETSURF_MENU_QUIT` | `0` | `1` lets MENU quit the app, for a launcher that hands the key over rather than spending it on a kill helper (both Miyoo packages set it) |
| `RETSURF_SERVO_PREFS` | — | Engine prefs the config does not expose, `name=value` comma-separated (e.g. `expose_servointernals_globally=true`) |
| `RETSURF_HEAP_TUNE` | — | `0`/`1` overrides whether the allocator is tuned for a small process; the memory tier decides otherwise |
| `RETSURF_MEMORY_DETAIL` | `0` | `1` logs the 20 largest whole memory-report paths beside the rolled-up groups |
| `RETSURF_PARTIAL_PRESENT` | `0` | `1` sends the panel only the part of the software frame that changed; the Miyoo driver misplaces a partial copy, which is why it is off |
| `RETSURF_ROUNDING` | `0` on the software renderer | `1` puts the chrome's rounded corners back there, to compare what they cost |
| `RETSURF_FEATHERING` | follows the renderer | `0`/`1` overrides egui's edge smoothing, which is off on the software renderer |
| `RETSURF_CONFIG` | — | Path to the config file (overrides the default in the data dir) |
| `RETSURF_DATA_DIR` | — | Override the user data dir (config, history, bookmarks, plus `servo/` for cookies and `cache/` for the adblock engine) |
| `RETSURF_DOWNLOAD_DIR` | — | Override where downloads are saved (created on demand). Takes precedence over the system download folder; the `[downloads].dir` config setting still wins over it. Falls back to `downloads/` in the data dir |
| `RETSURF_LOG_LEVEL` | `info` | Log verbosity (`error`/`warn`/`info`/`debug`/`trace`) |
| `RETSURF_LOG_STYLE` | `always` | Log coloring (`always`/`auto`/`never`) |
| `RETSURF_LOG_FILE` | — | Write logs to this file |
| `RETSURF_PANIC_FILE` | `retsurf-panic.log` | File for a panic's message + backtrace |
| `SDL_VIDEODRIVER` | auto | SDL video backend (`wayland`/`x11`, or whatever the firmware's SDL ships); auto-set to `wayland` on a Wayland desktop |

retsurf also sets `SURFMAN_FORCE_GLES=1` automatically when GLES is in use (so SDL's
and Servo's GL stacks agree) — you don't normally set it yourself.
