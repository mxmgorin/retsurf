# Handhelds

Which handhelds retsurf targets, and how it is packaged for them.

## PortMaster targets

- Knulli (Batocera-based), muOS, ROCKNIX, and ArkOS — the last one sets the glibc floor
  the binaries are built to (2.30), and is the only one of the four not yet run on a device
- aarch64, and no two of them reach the screen the same way: Knulli and muOS run no
  display server at all, ROCKNIX runs Sway, and not one has kmsdrm in its SDL
- Mali-G31 / G52 GPUs (RK3326 / RK3566), which expose OpenGL ES 3.2, and PowerVR on the
  TrimUI Smart Pro

## Packages

- **PortMaster** (the firmwares above, and the others PortMaster supports):
  `packaging/portmaster/`. Three per-core aarch64 binaries, picked at launch by
  `Retsurf.sh`; how they are built is in [`tools/arm64/README.md`](../tools/arm64/README.md).
  `libGLESv2` and `libEGL` (the GPU blob) resolve at runtime on the device, so they are not
  bundled.
- **Miyoo Mini** (OnionOS and Allium): `packaging/miyoo/onionos/` and
  `packaging/miyoo/allium/`, one armhf binary for both, rendered in software. Built as in
  [`tools/armhf/README.md`](../tools/armhf/README.md).

How the picture reaches the screen on these devices, including the EGL 1.4 quirks of their
blobs, is in [`RENDERING.md`](RENDERING.md).
