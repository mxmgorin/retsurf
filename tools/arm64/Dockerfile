# Cross-build environment for the aarch64 handhelds (ArkOS, muOS, Knulli,
# ROCKNIX), on the base the Linux ARM workflow runs its own container on.
#
# CI has no reason to come through here — it gets free native ARM runners. This is
# for a fix that still lives in a local Servo checkout, which a runner cannot see.
#
# Ubuntu 20.04 for its glibc: 2.31 keeps the link on `glibc-floor` (2.30, what
# ArkOS ships), where jammy's 2.35 put 45 symbols above it.
#
# Cross rather than qemu: a Servo build under emulation is a working day. The
# arm64 side is Debian multiarch, so there is no sysroot to assemble.
FROM ubuntu:20.04

# arm64 packages come from ports.ubuntu.com, so the existing sources must be
# pinned to amd64 first or apt fails the update on arm64 indexes it cannot find.
RUN sed -i 's|^deb |deb [arch=amd64] |' /etc/apt/sources.list \
 && printf '%s\n' \
      'deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports focal main universe' \
      'deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports focal-updates main universe' \
    > /etc/apt/sources.list.d/arm64.list \
 && dpkg --add-architecture arm64

# GCC 10 on both sides, beside focal's 9.3: SpiderMonkey wants >= 10.1, and 12's
# libstdc++ names `__libc_single_threaded`, a glibc 2.32 symbol above the floor.
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential gcc-10 g++-10 cmake gperf pkg-config \
      ca-certificates curl git gnupg xz-utils binutils file zip \
      gcc-10-aarch64-linux-gnu g++-10-aarch64-linux-gnu \
 && rm -rf /var/lib/apt/lists/*

# focal stops at Python 3.9 and Servo's WebIDL codegen is written in `match`.
# 3.11 and not 3.12, which dropped `distutils` from the stdlib.
ARG PYTHON_VERSION=3.11.16+20260901
RUN arch=$(uname -m) \
 && curl --retry 5 -fsSL \
      "https://github.com/astral-sh/python-build-standalone/releases/download/${PYTHON_VERSION#*+}/cpython-${PYTHON_VERSION}-${arch}-unknown-linux-gnu-install_only.tar.gz" \
    | tar -xz -C /opt \
 && ln -s /opt/python/bin/python3 /usr/local/bin/python3 \
 && ln -s /opt/python/bin/python3 /usr/local/bin/python \
 && python3 -V

# libclang for bindgen, from LLVM's own apt: focal stops at 10 and mozjs_sys 153
# needs 19, the version SpiderMonkey's configure also names.
RUN curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key \
      | gpg --dearmor > /usr/share/keyrings/llvm.gpg \
 && echo "deb [arch=amd64 signed-by=/usr/share/keyrings/llvm.gpg] http://apt.llvm.org/focal/ llvm-toolchain-focal-19 main" \
      > /etc/apt/sources.list.d/llvm19.list \
 && apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      clang-19 libclang-19-dev llvm-19 \
 && rm -rf /var/lib/apt/lists/* \
 && ls /usr/lib/llvm-19/lib/libclang.so /usr/lib/llvm-19/bin/llvm-objdump

# The workflow's list in arm64 form; everything else in the graph builds from
# source. fontconfig is named outright (focal spells it libfontconfig1-dev)
# because `yeslogic-fontconfig-sys` probes for it.
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      libssl-dev:arm64 libdbus-1-dev:arm64 libfreetype6-dev:arm64 \
      libfontconfig1-dev:arm64 libexpat1-dev:arm64 \
      libglib2.0-dev:arm64 libgl1-mesa-dev:arm64 libegl1-mesa-dev:arm64 \
      libgles2-mesa-dev:arm64 libharfbuzz-dev:arm64 liblzma-dev:arm64 \
      libudev-dev:arm64 libunwind-dev:arm64 libsdl2-dev:arm64 zlib1g-dev:arm64 \
 && rm -rf /var/lib/apt/lists/*

# cc-rs puts its own `-lstdc++` on the link line, which `-static-libstdc++` does
# not cover; a directory holding only the archive answers that one statically.
RUN mkdir -p /opt/cxx-static \
 && ln -s /usr/lib/gcc-cross/aarch64-linux-gnu/10/libstdc++.a /opt/cxx-static/libstdc++.a \
 && ls -l /opt/cxx-static/libstdc++.a

# Every pkg-config question the build asks, asked here instead: an hour into a
# cargo run is an expensive place to learn that a `.pc` is missing.
RUN aarch64-linux-gnu-gcc-10 --version | head -1 \
 && dpkg-query -W -f 'libc6:arm64 ${Version}\n' libc6:arm64 \
 && PKG_CONFIG_ALLOW_CROSS=1 \
    PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig \
    pkg-config --libs --cflags \
      sdl2 freetype2 fontconfig openssl dbus-1 glib-2.0 harfbuzz libudev zlib \
 && ls /usr/lib/aarch64-linux-gnu/libSDL2.so /usr/lib/aarch64-linux-gnu/libEGL.so
