# Cross-build environment for the aarch64 handhelds (muOS, Knulli, ROCKNIX),
# matching what the Linux ARM workflow builds natively on arm64 runners.
#
# CI has no reason to come through here — it gets free native ARM runners. This
# exists so a fix can be built and packaged before it reaches the fork's remote,
# which is exactly when CI cannot help: the engine is a git dependency pinned by
# revision, so anything still local to a working copy is invisible to a runner.
#
# Ubuntu 22.04 on purpose: the workflow runs on `ubuntu-22.04-arm`, so its glibc
# (2.35) is the floor every shipped aarch64 binary already assumes. A newer base
# would quietly raise that floor, which is what `glibc-floor` guards against.
#
# Cross rather than qemu: a Servo build under emulation is a working day. The
# arm64 side is Debian multiarch, so there is no separate sysroot to assemble —
# headers are shared and the libraries land in /usr/lib/aarch64-linux-gnu.
FROM ubuntu:22.04

# Host-side build tools. Servo brings real C/C++: SpiderMonkey drives its own
# configure/make (python3), and bindgen wants libclang.
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential clang libclang-dev llvm-dev cmake gperf pkg-config \
      python3 python3-pip python3-setuptools \
      ca-certificates curl git xz-utils binutils file zip \
      gcc-aarch64-linux-gnu g++-aarch64-linux-gnu \
 && rm -rf /var/lib/apt/lists/*

# arm64 packages come from ports.ubuntu.com, not the main archive, so the
# existing sources have to be pinned to amd64 first or apt tries to fetch arm64
# indexes it will never find and fails the whole update.
RUN sed -i 's|^deb |deb [arch=amd64] |' /etc/apt/sources.list \
 && printf '%s\n' \
      'deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports jammy main universe' \
      'deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports jammy-updates main universe' \
    > /etc/apt/sources.list.d/arm64.list \
 && dpkg --add-architecture arm64

# The same list the workflow apt-installs, in its arm64 form, plus fontconfig:
# `yeslogic-fontconfig-sys` probes it with pkg-config and the workflow never
# names it, because on a native runner it arrives as somebody's dependency.
# Everything else in the graph builds from source, which is why this list is no
# longer than it is.
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      libssl-dev:arm64 libdbus-1-dev:arm64 libfreetype6-dev:arm64 \
      libfontconfig-dev:arm64 libexpat1-dev:arm64 \
      libglib2.0-dev:arm64 libgl1-mesa-dev:arm64 libegl1-mesa-dev:arm64 \
      libgles2-mesa-dev:arm64 libharfbuzz-dev:arm64 liblzma-dev:arm64 \
      libudev-dev:arm64 libunwind-dev:arm64 libsdl2-dev:arm64 zlib1g-dev:arm64 \
 && rm -rf /var/lib/apt/lists/*

# Every pkg-config question the build asks, asked here instead — an hour into a
# cargo run is an expensive place to learn that a `.pc` is missing.
RUN aarch64-linux-gnu-gcc --version | head -1 \
 && PKG_CONFIG_ALLOW_CROSS=1 \
    PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig \
    pkg-config --libs --cflags \
      sdl2 freetype2 fontconfig openssl dbus-1 glib-2.0 harfbuzz libudev zlib \
 && ls /usr/lib/aarch64-linux-gnu/libSDL2.so /usr/lib/aarch64-linux-gnu/libEGL.so
