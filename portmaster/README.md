# retsurf

A lightweight, gamepad-native web browser powered by the Servo rendering engine.
It renders modern websites over OpenGL ES with no X11 or Wayland compositor, and
is driven entirely from the controller: a virtual cursor, Vimium-style link
hints, and an on-screen keyboard. Tabs, bookmarks, history, downloads, real page
zoom, reader mode, and network-level ad and tracker blocking are all built in.

## Controls

D-pad / Left stick   Move the virtual cursor (or scroll the page in scroll mode)
L2 / R2              Cycle tabs / drive the on-screen keyboard

A                    Confirm (click / select)
B                    Cancel (back / close)          Hold: go to home page
X                    On-screen keyboard             Hold: reader mode
Y                    Link hints                     Hold: bookmark the page
L1                   Previous (menu / history back) Hold: zoom out
R1                   Next (menu / history forward)  Hold: zoom in
L1 + R1              Reset zoom
Left stick click     Link hints
Right stick click    Settings
Start                Toggle cursor / page-scroll    Hold: reload
Select               Menu                           Hold: settings
Select + Start       Settings (press again while open to quit)

Every gesture is rebindable in-app from the settings overlay, or by editing
`bindings.toml` in the data folder. Devices without a right analog stick use the
Start scroll toggle and Select to reach every function.

## Notes

- First launch fetches and compiles the ad/tracker block lists (EasyList +
  EasyPrivacy); they are cached locally, so later starts are instant and work
  offline.
- Settings live in `config.toml` and controls in `bindings.toml`, both written
  with defaults on first run inside the port's `data/` folder. Most settings are
  editable in-app.
- Downloads are saved to the port's `downloads/` folder.
- On problems, check `log.txt` (and `retsurf-panic.log`) in the port folder.

## Credits

- Port and app by mxmgorin (Troidem)
- Rendering by [Servo](https://github.com/servo/servo)
- Source and issues: https://github.com/mxmgorin/retsurf
