# Building the Miyoo Mini (armhf) binary

The Miyoo Mini family is SSD202D, armv7, no GPU at all. Its renderer is the `software`
feature ([`docs/RENDERING.md`](../../docs/RENDERING.md)), and `packaging/miyoo/allium/` and
`packaging/miyoo/onionos/` are the device-side packages (each with its own `README.md`) —
one binary, two card layouts.

```sh
tools/armhf/build.sh              # prints the binary's path
RETSURF_ARM_LTO=thin tools/armhf/build.sh   # lighter link when RAM is short
tools/armhf/package-miyoo.sh -n   # both zips around the binary that is already built
```

It cross-compiles from x86_64 in a container; `.github/workflows/build-linux-armhf.yml` is
the same recipe in CI.

**The sysroot and the compiler come from different places.** The Miyoo community
toolchain supplies the sysroot (glibc 2.28, the device's floor); the compiler is Ubuntu's
cross GCC 10, since SpiderMonkey needs 10.1 and GCC 12's libstdc++ needs glibc 2.32.
libstdc++ is linked statically and bindgen uses libclang 19. `Dockerfile` records why.

Each of these cost a build to find, and each fails quietly:

- **Only `$SYSROOT/usr/lib` on the library path.** `--sysroot` alone links the host's libc;
  `$SYSROOT/lib` brings the toolchain's older libstdc++.
- **`-mtune=cortex-a7`, never `-mcpu`.** `-mcpu` makes GCC warn, and cc-rs then drops every
  `flag_if_supported` flag, including `mozjs_sys`'s `-fno-rtti` and
  `-fno-sized-deallocation` (a broken link, or heap corruption on the device).
- **NEON twice.** `-C target-feature=+neon` for Rust, and `-mfpu=neon-vfpv4` on the compiler
  driver (not `CFLAGS`, which SpiderMonkey's configure resets) for C/C++.
- **`HOST_CC`/`HOST_CXX` set to GCC**, or SpiderMonkey's configure picks the cross compiler
  for its build-machine tools.

SDL2 and fontconfig in the image are for linking only; the device brings its own SDL2.
