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

It cross-compiles from x86_64 in a container, the same as the local aarch64 build
(`tools/arm64/README.md`). `.github/workflows/build-linux-armhf.yml` is the same recipe in CI,
kept as its own workflow so it shares nothing with the aarch64 one.

**The compiler and the userland come from different places, and that is the whole design.**
The Miyoo Mini community toolchain supplies only its sysroot — glibc 2.28, the floor the
device's loader sets — while the compiler is Ubuntu's cross GCC 10, because SpiderMonkey 153
requires GCC 10.1 and `_GLIBCXX_RELEASE >= 10` where that toolchain's own GCC is 8.3. GCC 12
is packaged too and cannot be used: its libstdc++ references `__libc_single_threaded`, a
glibc 2.32 symbol absent from 2.28. So libc is taken from the sysroot, libstdc++ from the
compiler and linked statically, and bindgen runs against libclang 19 — `tools/armhf/Dockerfile`
records each of those choices and why zig is still out.

Four things about this target cost a build each to find, and all four fail quietly rather
than loudly:

- **The sysroot must not reach the library search path whole.** `--sysroot` alone leaves the
  link resolving libc from the build host, taking the binary over the floor; adding
  `$SYSROOT/lib` instead answers `-static-libstdc++` with the toolchain's libstdc++ 8.3,
  older than the headers the code compiled against. Only `$SYSROOT/usr/lib` goes on it,
  which has libc's linker script and no libstdc++ at all.

- **`-mtune=cortex-a7`, never `-mcpu`.** cc-rs passes `-march=armv7-a` of its own, GCC warns
  that `-mcpu` conflicts with it, and cc-rs treats *any* stderr from a flag probe as "flag
  unsupported" — so one warning silently drops every `flag_if_supported` flag in the graph.
  `mozjs_sys` asks for `-fno-rtti` and `-fno-sized-deallocation` that way; losing the first
  breaks the link against SpiderMonkey, losing the second is an ABI mismatch on
  `operator delete` that would only show up as heap corruption on the device.
- **NEON is off unless asked for twice, and the FPU is off unless the driver carries it.**
  `-C target-feature=+neon` for the Rust half — the rustc target spec disables NEON outright
  and `-C target-cpu` does not undo that — and `-mfpu=neon-vfpv4` for the C/C++ half, which
  goes on the compiler driver rather than in `CFLAGS`: SpiderMonkey's configure probes with a
  bare `-march=armv7-a` that resets the FPU choice, and `-mfloat-abi=hard` then has nothing
  to use. That shim also settles cc-rs's `-mfpu=vfpv3-d16`, which has no NEON.
- **`HOST_CC`/`HOST_CXX` must be set.** SpiderMonkey builds tools that run on the build
  machine, and its configure otherwise falls back to the cross compiler and rejects it. They
  name GCC: 153 wants clang 19 or newer for a compiler, and the distro's is older.

The sysroot in the image carries SDL2 and fontconfig from Debian for the link step only; the
toolchain's own sysroot has neither. On the device both have to come from somewhere else —
its SDL2 is a Miyoo-specific build.
