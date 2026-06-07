[![Build linux ARM](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml/badge.svg)](https://github.com/mxmgorin/retsurf/actions/workflows/build-linux-arm.yml)
[![Dependencies](https://deps.rs/repo/github/mxmgorin/retsurf/status.svg)](https://deps.rs/repo/github/mxmgorin/retsurf)

# 🌊 retsurf

A lightweight, experimental web browser written in **Rust**, using [**Servo**](https://github.com/servo/servo) as the rendering engine, **SDL2** for windowing and input, and **egui** for the UI.

It is designed to run **without X11 or Wayland** — rendering through **OpenGL ES** on bare KMS/DRM — with **gamepad support**, targeting PortMaster-compatible Linux handhelds (**Knulli, muOS, ROCKNIX**) on Mali-class GPUs, as well as regular Linux desktops.

> 🛠️ **Work in progress.** Early development stage with limited features.

## Features

- Web rendering via the **Servo** engine (WebRender)
- Runs on **OpenGL ES 3.x**; no X11/Wayland required (works on bare KMS/DRM)
- Minimal **egui** toolbar: address bar, back / forward / reload
- Keyboard, mouse, and **gamepad** input
- Single self-contained binary

## How it works

SDL2 owns the window and the single GL/GLES context. Servo renders each page into an offscreen framebuffer (FBO) in that context; egui then composites the page texture together with the toolbar and presents the frame via SDL2. Keeping everything on one GLES context — with no compositor and no CPU readback — is what lets it run on bare handheld hardware.

The full specs (GLES vs desktop GL, the EGL/surfman constraints on older Mali blobs) lives in
[`docs/HANDHELD_PORT.md`](docs/HANDHELD_PORT.md).

## Building & running

### Desktop (Linux)

You need Servo's build dependencies. On Debian/Ubuntu:

```sh
sudo apt-get install -y build-essential clang cmake curl git gperf pkg-config python3 \
  libssl-dev libdbus-1-dev libfreetype6-dev libglib2.0-dev \
  libgl1-mesa-dev libegl1-mesa-dev libgles2-mesa-dev \
  libharfbuzz-dev liblzma-dev libudev-dev libunwind-dev libsdl2-dev
```

Then:

```sh
cargo run
```

On a Wayland desktop, retsurf auto-selects SDL's Wayland driver and a GLES context.

**Environment variables:**

| Variable | Default | Effect |
|----------|---------|--------|
| `RETSURF_GLES` | `1` | `0` uses desktop OpenGL instead of GLES (debugging) |
| `RETSURF_LOG_LEVEL` | `info` | Log verbosity (`error`/`warn`/`info`/`debug`/`trace`) |
| `SDL_VIDEODRIVER` | auto | Override SDL's video backend (`wayland`, `x11`, `kmsdrm`) |

## Status

Experimental. Basic page rendering and navigation work. WebGL is disabled on devices whose EGL stack is too old for surfman (e.g. EGL 1.4 Mali blobs); everything else renders normally. Many browser features are not yet implemented.

## References

- [The Servo Book](https://book.servo.org/title-page.html)
