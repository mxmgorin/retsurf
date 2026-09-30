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
