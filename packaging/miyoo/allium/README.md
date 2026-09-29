# retsurf for Allium

A web browser on a Miyoo Mini Plus or Flip. The engine is
[Servo](https://servo.org); there is no GPU on this device, so the page is
rasterized on the CPU by WebRender's own software backend and the chrome is
drawn by SDL's 2D renderer.

**Miyoo Mini Plus and Flip only.**

## Install

Unzip `retsurf-allium.zip` into the root of the SD card, so the app lands in
`Apps/Retsurf.pak/`. It shows up on the Apps tab.

Turn wifi on in Allium's settings first — it is off by default.

## Controls

| Button | Action | Held |
|---|---|---|
| D-pad | Move the cursor; pushed against an edge, scroll the page | |
| A | Click / confirm | |
| B | Back out of an overlay | |
| X | On-screen keyboard | Reader view |
| Y | Link hints | Bookmark this page |
| L1 / R1 | Back / forward | Home page / reload |
| L2 / R2 | Zoom out / in | |
| L2 + R2 | Reset the zoom | |
| Start | Quick Access — reader view, Game Mode, bookmark, page theme | |
| Start + L1 / R1 | Close the tab / open a new one | |
| Select | Quick Menu — home, tabs, bookmarks, history, downloads, settings, quit | Address bar |
| Select + L1 / R1 | Previous / next tab | |
| MENU | Quit | (held with a pad it stays Allium's) |

The full table is in Settings → Controls → Button bindings, where every one of
them can be rebound. MENU is the exception and deliberately not a binding.

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

## In the Games tab as well

Allium's **Ports Collection** console runs anything under `Roms/PORTS` whose
name ends `.port`, by entering the folder and running its `launch.sh`.
`ports/Retsurf.port/` in this package is that folder — copy it to
`Roms/PORTS/Retsurf.port/` and the browser appears among the games too. It holds
one line, which hands over to the install under `Apps/`, so there is one copy.
Box art, if you want it: `Roms/PORTS/Imgs/Retsurf.png`.

## Credits

- Developed and ported by [mxmgorin](https://github.com/mxmgorin/)
- Engine: [Servo](https://servo.org), under MPL-2.0
- Bundled SDL2 for the Miyoo Mini by
  [Steward Fu](https://github.com/steward-fu/sdl2) (zlib, with LGPL-2.1 drivers)
  — provenance and licences in `lib/README.md`
- Fonts: [DejaVu](https://dejavu-fonts.github.io/), under its own free licence
- [Allium](https://github.com/goweiwen/Allium) is goweiwen's; this package only
  follows its `Apps/*.pak` layout
- Source and issues: https://github.com/mxmgorin/retsurf
