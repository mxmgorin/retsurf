# retsurf

A lightweight, gamepad-first web browser powered by the Servo rendering engine.
It renders modern websites over OpenGL ES with no X11 or Wayland compositor, and
is driven entirely from the gamepad: a virtual cursor, Vimium-style link
hints, and an on-screen keyboard. Tabs, bookmarks, history, downloads, real page
zoom, reader mode, and network-level ad and tracker blocking are all built in.

## Controls

D-pad / Left stick   Move the virtual cursor; pushed against an edge, scroll
Right stick          Scroll the page
A                    Confirm (click / select)
B                    Cancel (back / close)
X                    On-screen keyboard             Hold: reader mode
Y                    Link hints                     Hold: bookmark the page
L1 / R1              Back / forward                 Hold: home page / reload
L2 / R2              Zoom out / in
L2 + R2              Reset zoom
Left stick click     Link hints
Start                Quick Access (reader view, Game Mode, bookmark, page theme)
Start + L1 / R1      Close the tab / open a new one
Select               Quick Menu (home, tabs,        Hold: address bar
                     bookmarks, history, downloads,
                     settings, quit)
Select + L1 / R1     Previous / next tab
Select + Start       Quit (PortMaster's own)

Every gesture is rebindable in-app (Settings → Controls → Button bindings), or
by editing `bindings.toml` in the data folder.

## Notes

- First launch fetches and compiles the ad/tracker block lists (EasyList +
  EasyPrivacy).
- Downloads are saved to the port's `downloads/` folder.
- On problems, check `log.txt` (and `retsurf-panic.log`) in the port folder.

## Credits

- Developed and ported [mxmgorin](https://github.com/mxmgorin)
- Rendering by [Servo](https://github.com/servo/servo)
- Source: https://github.com/mxmgorin/retsurf
