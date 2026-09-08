# Building the aarch64 handheld binaries locally

CI builds these natively on free arm64 runners (`.github/workflows/build-linux-arm.yml`),
which is the right way when the whole tree is pushed. This exists for when it is
not: the engine is a git dependency pinned by revision, so a fix that still lives
in a local Servo checkout is invisible to a runner. `RETSURF_SERVO_SRC` is the
whole point of the tool.

```
tools/arm64/build.sh                      # a35 a53 a55 -> dist/arm64/
tools/arm64/build.sh a55                  # one core
tools/arm64/build.sh universal            # the generic non-PortMaster binary
tools/arm64/package-portmaster.sh         # builds, then dist/portmaster{,.zip}
tools/arm64/package-portmaster.sh -n      # package what is already built

RETSURF_SERVO_SRC=~/Repos/servo tools/arm64/build.sh a53   # against a local fork
```

Caches live in `~/.cache/retsurf-arm64` (`RETSURF_ARM64_CACHE`), outside the repo
so container-root files never mix with host builds. `RETSURF_ARM64_LTO=thin`
trades a slower binary for a link that fits in less RAM.

The `target/` half of that cache belongs to one base image: it holds *host*
build-script binaries, cargo fingerprints nothing about the glibc they were
linked against, and a run under an older base then dies on `GLIBC_2.34 not
found` from every cached script at once. Changing the image means starting that
directory over.

## Why cross, and why Ubuntu 20.04

Cross rather than qemu: a Servo build under emulation is a working day.

The base is focal because glibc is a one-way ratchet: a binary links whatever
symbol versions the build host offers, and the loader on the device refuses
anything newer than its own. focal's 2.31 puts the link within reach of
`glibc-floor` (2.30, what ArkOS ships) where jammy's 2.35 sat two years of
symbols above it — 33 from the libpthread/libdl merge alone. Nothing is given up
to sit there: the graph's own highest reference is `gettid@GLIBC_2.30`, so 2.30
is where it lands anyway. The check at the end of `build.sh` fails the build if
that stops being true. CI runs the same base as a container on its arm64 runner
(`.github/actions/arm-build-env`).

Two consequences of the old base, both worth knowing before touching the image:

- **GCC 10 on both sides.** SpiderMonkey wants GCC >= 10.1 and
  `_GLIBCXX_RELEASE >= 10`, where focal's default is 9.3; GCC 12 would clear that
  but its libstdc++ references `__libc_single_threaded`, a glibc 2.32 symbol.
  libstdc++ is then linked statically, because the devices at the floor carry a
  GCC-9 runtime that has none of GCC 10's symbols.
- **libclang 19 and Python 3.11 come from outside the archive**, since focal
  stops at clang 10 and Python 3.9. `mozjs_sys` 153 names clang 19 itself;
  Python has two floors, SpiderMonkey's tree at 3.9 and Servo's WebIDL codegen
  at 3.11 (it is written in `match`), so a portable CPython is unpacked into
  `/opt` rather than trusting a third-party archive for a distro this old.

`MOZJS_FROM_SOURCE=1` is what makes any of the above matter. Left unset,
`mozjs_sys` downloads a prebuilt SpiderMonkey instead of compiling one, and that
archive is built on Ubuntu 22.04: it was both the bulk of the symbols above the
floor and a libstdc++ 11 the GCC 10 runtime cannot answer
(`std::__throw_bad_array_new_length` at link time). The compilers configured
here went unused until it was set.

The arm64 side is Debian multiarch, not a hand-assembled sysroot: headers are
shared with the host and the libraries land in `/usr/lib/aarch64-linux-gnu`, so
`PKG_CONFIG_SYSROOT_DIR` must stay **unset** (setting it would double every
`-I`). Only `PKG_CONFIG_LIBDIR` and `PKG_CONFIG_ALLOW_CROSS` are needed. This is
far less work than the armhf image next door, which unpacks a dozen Debian `.deb`
files by hand — there the toolchain is the device's own and there is no archive
to ask.

`apt` needs the existing sources pinned to `[arch=amd64]` before arm64 is added,
or it tries to fetch arm64 indexes from the main archive (which does not carry
them) and fails the whole update.

## Three binaries, not one

Cortex-A55 is ARMv8.2 — LSE atomics, fp16, dotprod — and code built for it
SIGILLs on the v8.0 cores. `Retsurf.sh` reads `/proc/cpuinfo` and execs the
matching one:

| | cores | flags |
|---|---|---|
| `a35` | RK3326; runs on A53 too, same ISA | `--no-default-features`, `panic=abort` |
| `a53` | H700, Allwinner A133 Plus (crypto off) | same |
| `a55` | RK3566, Allwinner A523/T527 | same |
| `universal` | any ARMv8.0+, non-PortMaster installs | default features (webgl on), `panic=unwind` |

`--no-default-features` drops webgl, which drops the surfman probe — our only
`catch_unwind` — and unwind tables with it. The universal binary keeps both,
which is why it cannot share the others' profile.

`RUSTFLAGS` differs per core, and that invalidates the whole dependency graph,
not just our crate: expect a near-full Rust rebuild per binary. SpiderMonkey is
a build script's output and survives that, which is what keeps this to tens of
minutes rather than hours — it is the one thing here that costs ~20 minutes on
twelve cores by itself.

## Two things that will look like failures and are not

**`patch ... was not used in the crate graph`**, once per patched Servo crate,
naming the git revision and pointing at the local path. This is cargo saying the
*git* source is no longer in the graph — because the `[patch]` replaced it with
the path. The patch worked. To confirm rather than trust it, look for a string
that only exists in the local checkout:

```
strings -a dist/arm64/retsurf.a53 | grep 'could not define the SpiderMonkey testing functions'
```

**A missing `.pc` an hour into the build.** The workflow's apt list is what a
*native* runner needed, and its dependency closure differs from what
`--no-install-recommends` gives here: fontconfig's `-dev` is pulled in for free
there and has to be named here (as `libfontconfig1-dev` — the unsuffixed alias
postdates focal), which cost one full build to discover. The image now asks
pkg-config for all nine packages the graph probes as its last build step, so the
next missing one fails in a minute instead.
