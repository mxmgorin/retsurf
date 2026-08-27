# retsurf for Allium

A web browser on a Miyoo Mini Plus or Flip. The engine is
[Servo](https://servo.org); there is no GPU on this device, so the page is
rasterized on the CPU by WebRender's own software backend and the chrome is
drawn by SDL's 2D renderer.

**Miyoo Mini Plus and Flip only.** The original Mini has no wifi, and a browser
without a network is nothing.

**This is a first device build.** It has been checked on a desktop, under qemu,
and nowhere else. Expect it to be slow, and expect to find out here whether 128
MB is enough.

## Install

Unzip `retsurf-allium.zip` into the root of the SD card, so the app lands in
`Apps/Retsurf.pak/`. It shows up on the Apps tab. The `.pak` suffix is what makes
a folder an app rather than one to walk into, so keep it if you rename anything.

Turn wifi on in Allium's settings first — it is off by default.

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
| MENU | Quit | (held with a pad it stays Allium's) |

The full table is in Settings → Controls, where every one of them can be
rebound. MENU is the exception and deliberately not a binding: Allium keeps no
kill helper of the kind OnionOS has, so it is the one way out that has to
survive whatever the tables are edited to. Powering off and closing the lid
arrive as a signal, and the browser writes its cookies and open tabs on it.

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

## What the launcher does for this device

- **Software rendering** (`RETSURF_SOFTWARE=1`): the SSD202 has no GPU. The page
  is rasterized by swgl, the chrome by SDL's software renderer into an offscreen
  surface, and the panel gets one texture copy of the composed frame — which is
  the only thing this display driver shows.
- **Its own SDL2** in `lib/`, preloaded: the panel hangs off the SigmaStar
  display pipeline and no upstream SDL2 can reach it. `SDL_VIDEODRIVER=Mini`,
  `SDL_RENDER_DRIVER="Miyoo Mini"` — SDL lists its own software driver ahead of
  the panel's, and what that one draws never arrives. Provenance and licences in
  `lib/README.md`.
- **The clock.** A boot can come up a year behind, and then no https page loads
  at all because every certificate is "not valid yet". The Flip has an RTC that
  keeps what it is given, but nothing in this firmware ever gives it anything;
  the Mini Plus has no RTC, so there this happens on every boot. The launcher
  acts only when the clock is behind something already known to have happened,
  so a right clock costs nothing: it jumps to that floor immediately — enough
  for a certificate's `notBefore` — and asks the network for the real time in
  the background, so a wifi that has not associated yet never delays the start.
  `log.txt` records what it got. `RETSURF_CLOCK_FIX=0` turns it off.
- **The pad, as keys.** This SDL2 offers no controller mapping and sends key
  presses instead, so `RETSURF_KEYMAP=miyoo` tells the browser to read them as
  the pad — which is what makes the table above work. Without it A, B, X, Y and
  Select do nothing at all.
- **Its own CA bundle.** Servo checks TLS against the platform certificate
  store and *panics* when it finds none — and no Miyoo firmware carries one, so
  without `etc/ssl/cacert.pem` and the `SSL_CERT_FILE` pointing at it the
  browser aborts before its first frame.
- **Its own fontconfig and fonts.** Neither firmware ships either. The package
  carries DejaVu Sans, Serif and Sans Mono, and `etc/fonts/fonts.conf.in` maps
  the families the web asks for onto them; the launcher bakes the install path
  into it at startup.
- **Swap.** The working set is measured in hundreds of megabytes and the device
  reports 101 of them, so without somewhere to page anonymous memory the kernel
  kills the browser instead of swapping it. The launcher takes zram where a
  kernel has it and a 256 MB file on the card otherwise — which works even
  though the card is FAT32, because vfat can map a file for the swap code. This
  kernel has neither zram nor a loop device, so the file is what it uses. The
  file is created once (about 20 seconds) and released on exit; `log.txt`
  records which it got, or `no` if neither worked, and that line is the first
  thing to read if the browser dies early.

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
