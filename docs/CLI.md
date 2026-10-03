# Command line

```
retsurf [--game-mode] [PATH]
```

| Argument | Effect |
| --- | --- |
| `PATH` | Open a local game folder (its `index.html`) or an HTML file. |
| `--game-mode` | Start in Game Mode, before the page loads. |
| `-h`, `--help` / `-V`, `--version` | Print the usage or the version. |

A folder is served as `http://<folder-name>.localhost/`, with its own saves (localStorage,
IndexedDB); names differing only in case or punctuation share them. `.br` and `.gz` files
are decompressed.

## Examples

```sh
retsurf ~/games/hexgl                 # a game folder: opens its index.html
retsurf --game-mode ~/games/hexgl     # the same, straight into Game Mode
retsurf ~/games/carts/cart.html       # an HTML file inside a folder
retsurf --game-mode                   # Game Mode over the usual first tabs
```

A PATH starting with `-` goes after `--`: `retsurf -- -name/`. A bad PATH or an unknown
option prints the usage and exits with status 64.

## Environment variables

| Variable | Default | Effect |
|----------|---------|--------|
| `RETSURF_CONFIG` | — | Config file path |
| `RETSURF_DATA_DIR` | — | User data dir |
| `RETSURF_DOWNLOAD_DIR` | — | Download folder; `[downloads] dir` still wins |
| `RETSURF_LOG_LEVEL` | `info` | `error`, `warn`, `info`, `debug` or `trace` |
| `RETSURF_LOG_STYLE` | `always` | Log colors: `always`, `auto` or `never` |
| `RETSURF_LOG_FILE` | — | Write the log to this file |
| `RETSURF_PANIC_FILE` | `retsurf-panic.log` | Where a panic's message and backtrace go |
| `RETSURF_GLES` | `1` | `0` uses desktop OpenGL (debugging) |
| `RETSURF_SOFTWARE` | `0` | `1` forces CPU rendering |
| `RETSURF_SCALE` | — | UI zoom in place of the fit to the screen; `[interface] scale` still applies |
| `RETSURF_MAX_FPS` | — | Overrides `[performance] max_fps` |
| `RETSURF_KEYMAP` | auto | `miyoo` reads the pad from the keys that firmware sends, `desktop` never does |
| `RETSURF_MENU_QUIT` | `0` | `1` lets MENU quit, for launchers that hand the key over |
| `RETSURF_MAIN_NICE` | — | Main-thread niceness; a negative value needs root or `CAP_SYS_NICE` |
| `RETSURF_SERVO_PREFS` | — | Extra engine prefs, `name=value,...` |
| `RETSURF_HEAP_TUNE` | — | `0`/`1` overrides the memory tier's allocator tuning |
| `RETSURF_MEMORY_DETAIL` | `0` | `1` logs the 20 largest memory-report paths |
| `RETSURF_PARTIAL_PRESENT` | `0` | `1` presents only the changed part of a software frame (misplaced on Miyoo) |
| `RETSURF_ROUNDING` | `0` in software | `1` restores rounded corners on the software renderer |
| `RETSURF_FEATHERING` | by renderer | `0`/`1` overrides egui's edge smoothing |
| `SDL_VIDEODRIVER` | auto | SDL video backend; set to `wayland` on a Wayland desktop |

retsurf sets `SURFMAN_FORCE_GLES=1` itself when GLES is on.
