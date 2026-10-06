# retsurf for spruceOS on the Miyoo Mini

A web browser on a Miyoo Mini Plus or Mini Flip (the original Mini has no wifi).
The engine is [Servo](https://servo.org).

## Install

Unzip `retsurf-spruceos-armhf.zip` into the root of the SD card, so the app
lands in `App/RetsurfMini/`. It shows up under Apps.

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
| MENU | Quit | |

The full table is in Settings → Controls → Button bindings, where every one of
them can be rebound.

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
