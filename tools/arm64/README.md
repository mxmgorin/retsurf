# Building the aarch64 handheld binaries locally

CI builds these on arm64 runners (`.github/workflows/build-linux-arm.yml`). This cross-build
from x86_64 is for when the engine fix is still in a local Servo checkout, which a runner
cannot see: that is what `RETSURF_SERVO_SRC` is for.

```
tools/arm64/build.sh                      # a35 a53 a55 -> dist/arm64/
tools/arm64/build.sh a55                  # one core
tools/arm64/build.sh universal            # the generic non-PortMaster binary
tools/arm64/package-portmaster.sh         # builds, then dist/portmaster{,.zip}
tools/arm64/package-portmaster.sh -n      # package what is already built

RETSURF_SERVO_SRC=~/Repos/servo tools/arm64/build.sh a53   # against a local fork
```

Caches live in `~/.cache/retsurf-arm64` (`RETSURF_ARM64_CACHE`). `RETSURF_ARM64_LTO=thin`
links in less RAM for a slower binary. The `target/` cache belongs to one base image: after
changing the image, delete it, or every cached build script dies on `GLIBC_2.34 not found`.

## Ubuntu 20.04, for the glibc floor

A binary needs a glibc at least as new as the one it was built against. focal's 2.31 keeps
the link at `glibc-floor` (2.30, what ArkOS ships); jammy's 2.35 would not. `build.sh` fails
if the binary drifts above the floor. CI uses the same base (`.github/actions/arm-build-env`).

What follows from the old base:

- **GCC 10, libstdc++ linked statically.** SpiderMonkey needs GCC 10.1; GCC 12's libstdc++
  needs glibc 2.32; the devices carry a GCC 9 runtime.
- **libclang 19 and Python 3.11 from outside the archive**, for `mozjs_sys` and Servo's
  WebIDL codegen.
- **`MOZJS_FROM_SOURCE=1`.** The prebuilt SpiderMonkey is built on 22.04, above the floor
  and against libstdc++ 11.
- **Debian multiarch, not a sysroot**: set `PKG_CONFIG_LIBDIR` and `PKG_CONFIG_ALLOW_CROSS`,
  and leave `PKG_CONFIG_SYSROOT_DIR` unset. apt's existing sources are pinned to
  `[arch=amd64]` before arm64 is added.

## Three binaries

Cortex-A55 code (ARMv8.2) crashes on v8.0 cores, so `Retsurf.sh` picks one by
`/proc/cpuinfo`:

| | cores | panic |
|---|---|---|
| `a35` | RK3326, A53 | abort |
| `a53` | H700, A133 Plus | abort |
| `a55` | RK3566, A523/T527 | abort |
| `universal` | any ARMv8.0, non-PortMaster | unwind |

Each core's `RUSTFLAGS` rebuild the whole Rust graph; SpiderMonkey (~20 minutes) is reused.

## Not failures

- **`patch ... was not used in the crate graph`** under `RETSURF_SERVO_SRC`: the path patch
  replaced the git source, as intended. To confirm, `strings` the binary for text only the
  local checkout has.
- **A missing `.pc` late in the build**: a `-dev` package the native runner got for free. The
  image now asks pkg-config for every package the graph probes, so this fails early.
