# retsurf for OnionOS

A web browser on a Miyoo Mini Plus or Flip. The engine is
[Servo](https://servo.org); there is no GPU on this device, so the page is
rasterized on the CPU by WebRender's own software backend and the chrome is
drawn by SDL's 2D renderer.

**Miyoo Mini Plus and Flip only.**

## Install

Unzip `retsurf-onionos.zip` into the root of the SD card, so the app lands in
`App/Retsurf/`. It shows up under Apps.

Turn wifi on in Onion's settings first — the Plus keeps it off by default.

## Controls

| Button | Action | Held |
|---|---|---|
| D-pad | Move the cursor, or scroll while scroll mode is on | |
| A | Click / confirm | |
| B | Back out of an overlay | Home page |
| X | On-screen keyboard | Reader view |
| Y | Link hints | Bookmark this page |
| L1 / R1 | Back / forward | Zoom out / in |
| L1 + R1 | Reset the zoom | |
| Start | Scroll mode | Reload |
| Select | Menu — tabs, bookmarks, history, downloads | Settings |
| Select + Start | Settings, and **quit** when settings is already open | |
| MENU | Quit | (held with a pad it stays Onion's) |

The full table is in Settings → Controls, where every one of them can be
rebound. MENU is the exception and deliberately not a binding: it is the one way
out that has to survive whatever the tables are edited to.

## Where things go

Everything writable stays inside the app folder, so removing it removes the lot:

| | |
|---|---|
| `data/config.toml` | settings (also editable in Settings) |
| `data/bindings.toml` | the control bindings |
| `data/` | cookies, history, bookmarks, the ad-block cache |
| `downloads/` | downloaded files |
| `log.txt` | the last run |
| `retsurf-panic.log` | written only if it crashes |

## Credits

- Developed and ported by [mxmgorin](https://github.com/mxmgorin/)
- Engine: [Servo](https://servo.org), under MPL-2.0
- Bundled SDL2 for the Miyoo Mini by
  [Steward Fu](https://github.com/steward-fu/sdl2) (zlib, with LGPL-2.1 drivers)
  — provenance and licences in `lib/README.md`
- Fonts: [DejaVu](https://dejavu-fonts.github.io/), under its own free licence
- [OnionOS](https://github.com/OnionUI/Onion) is the OnionUI team's; this package
  only follows its `App/*` layout
- Source and issues: https://github.com/mxmgorin/retsurf
