# Cross-build environment for the Miyoo Mini (SSD202D, armv7, OnionOS), matching
# the Linux ARMhf workflow.
#
# armhf only. The aarch64 handhelds are built natively on ARM runners (see
# build-linux-arm.yml) and have no reason to come through here.
#
# The device's userland comes from the Miyoo Mini community toolchain: its
# buildroot sysroot is glibc 2.28, the OnionOS floor, so the floor holds by
# construction rather than by asking for stubs. That is now all it is used for —
# the compiler is Ubuntu's GCC 10 cross, because mozjs_sys 153 wants GCC >= 10.1
# and _GLIBCXX_RELEASE >= 10 where the toolchain's own GCC is 8.3.
#
# GCC 10 and not 12, which is also packaged: libstdc++ 12 references
# `__libc_single_threaded`, a glibc 2.32 symbol that does not exist in 2.28 at
# all, so it cannot be linked for this device by any arrangement of flags. GCC 10
# is the newest whose C++ runtime still fits, and it is exactly SpiderMonkey's
# floor. `build.sh` keeps the two halves apart: the sysroot supplies libc, the
# compiler its own libstdc++, and it is linked statically so no C++ runtime has
# to be shipped or matched. Mozilla's configure refuses a *compiler* that links
# libstdc++ statically by default (which is what ruled zig out — it always links
# its own libc++ that way and offers no way out); passing the flag at our own
# final link is not that, and configure never sees it.
#
# Ubuntu carries the host side instead of the toolchain's own buster image:
# SpiderMonkey's configure wants python3 >= 3.8 and bindgen wants a libclang
# newer than buster ships.
FROM ubuntu:22.04

ARG TOOLCHAIN_VERSION=v0.0.3
# Debian bookworm. Older SDL2 predates SDL_RenderGeometry, the call the software
# renderer draws through — the only way anything reaches the Miyoo's panel.
ARG SDL_VERSION=2.26.5+dfsg-1
ARG FONTCONFIG_VERSION=2.14.1-4
ARG FREETYPE_VERSION=2.12.1+dfsg-5+deb12u4
ARG EXPAT_VERSION=2.5.0-1+deb12u2
ARG ZLIB_VERSION=1.2.13.dfsg-1
ARG LIBPNG_VERSION=1.6.39-2+deb12u5
ARG BROTLI_VERSION=1.0.9-2+b6

# Host-side build tools. Servo brings real C/C++: SpiderMonkey drives its own
# configure/make (python3), and bindgen wants libclang.
#
# g++-10-arm-linux-gnueabihf is the target compiler (see above); it brings the
# cross binutils `build.sh` reads the finished ELF with.
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential cmake gperf pkg-config \
      gcc-10-arm-linux-gnueabihf g++-10-arm-linux-gnueabihf \
      python3 python3-pip python3-setuptools \
      ca-certificates curl git gnupg xz-utils binutils file \
 && rm -rf /var/lib/apt/lists/* \
 && arm-linux-gnueabihf-g++-10 --version | head -1

# libclang for bindgen, from LLVM's own apt: jammy stops at 15 and mozjs_sys 153
# needs 19, the version SpiderMonkey's configure also names. Its headers put a
# `[[maybe_unused]]` after a GNU attribute (mfbt/Attributes.h gives GCC and clang
# the same `__attribute__((format(printf, ...)))`), which every clang up to 15
# rejects outright: "an attribute list cannot appear here".
RUN curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key \
      | gpg --dearmor > /usr/share/keyrings/llvm.gpg \
 && echo "deb [signed-by=/usr/share/keyrings/llvm.gpg] http://apt.llvm.org/jammy/ llvm-toolchain-jammy-19 main" \
      > /etc/apt/sources.list.d/llvm19.list \
 && apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      clang-19 libclang-19-dev llvm-19 \
 && rm -rf /var/lib/apt/lists/* \
 && ls /usr/lib/llvm-19/lib/libclang.so /usr/lib/llvm-19/bin/llvm-objdump

# Ubuntu's cross GCC carries no default FPU, and SpiderMonkey's configure probes
# the compiler with a bare `-march=armv7-a` of its own: that resets the FPU
# choice, and armhf's default `-mfloat-abi=hard` then has nothing to use ("cc1:
# error: selected processor lacks an FPU"). CFLAGS cannot reach that probe, so
# the A7's FPU goes on the driver itself. Last wins, so this also settles the
# -mfpu that cc-rs passes. `build.sh` puts these first on PATH.
# `-static-libstdc++` only replaces the driver's own `-lstdc++`, and cc-rs puts
# another one on the link line (`cargo:rustc-link-lib=stdc++` for every C++ crate
# in the graph), which finds a shared library and pulls it back in. A directory
# holding only the archive answers that `-lstdc++` too: within one directory the
# `.so` wins, but a directory without one falls through to the archive here.
RUN mkdir -p /opt/cxx-static \
 && ln -s /usr/lib/gcc-cross/arm-linux-gnueabihf/10/libstdc++.a /opt/cxx-static/libstdc++.a \
 && ls -l /opt/cxx-static/libstdc++.a

RUN mkdir -p /opt/cc-shims \
 && for tool in gcc-10 g++-10; do \
      printf '%s\n' \
        '#!/bin/sh' \
        '# The A7 has NEON-VFPv4 and this GCC assumes none; see the Dockerfile.' \
        "exec /usr/bin/arm-linux-gnueabihf-$tool \"\$@\" -mfpu=neon-vfpv4" \
      > "/opt/cc-shims/arm-linux-gnueabihf-$tool" \
      && chmod 755 "/opt/cc-shims/arm-linux-gnueabihf-$tool"; \
    done

RUN curl --retry 5 -fsSL \
      "https://github.com/shauninman/miyoomini-toolchain-buildroot/releases/download/${TOOLCHAIN_VERSION}/miyoomini-toolchain.tar.xz" \
    | tar -xJ -C /opt \
 && /opt/miyoomini-toolchain/bin/arm-linux-gnueabihf-gcc --version | head -1 \
 && /opt/miyoomini-toolchain/bin/arm-linux-gnueabihf-gcc -print-sysroot

# SDL2 and fontconfig for the link step; the toolchain's sysroot carries the
# device's own libraries but neither of these. Only their symbols matter — the
# device loads its own SDL2, and fontconfig ships in the app.
#
# zlib is here for its headers too: freetype-sys builds libpng, which includes
# <zlib.h>, and it never looks at libz-sys's vendored copy (build.rs hardcodes a
# relative path that does not exist in the published crate). On a host build the
# system header covers that up; cross-compiling there is no system to fall back on.
#
# libpng and brotli are neither linked nor included — they are here because
# pkg-config refuses to answer for fontconfig until every .pc in the chain
# resolves, and Debian's freetype2.pc names them under Requires.private. The
# probe at the end is that same question, asked while the image is built rather
# than twenty minutes into a cargo run.
RUN pool=http://deb.debian.org/debian/pool/main \
 && cd /tmp \
 && mkdir -p /opt/sysroot \
 && for deb in \
      "libs/libsdl2/libsdl2-2.0-0_${SDL_VERSION}_armhf.deb" \
      "libs/libsdl2/libsdl2-dev_${SDL_VERSION}_armhf.deb" \
      "f/fontconfig/libfontconfig1_${FONTCONFIG_VERSION}_armhf.deb" \
      "f/fontconfig/libfontconfig-dev_${FONTCONFIG_VERSION}_armhf.deb" \
      "f/freetype/libfreetype6_${FREETYPE_VERSION}_armhf.deb" \
      "f/freetype/libfreetype-dev_${FREETYPE_VERSION}_armhf.deb" \
      "e/expat/libexpat1_${EXPAT_VERSION}_armhf.deb" \
      "e/expat/libexpat1-dev_${EXPAT_VERSION}_armhf.deb" \
      "z/zlib/zlib1g_${ZLIB_VERSION}_armhf.deb" \
      "z/zlib/zlib1g-dev_${ZLIB_VERSION}_armhf.deb" \
      "libp/libpng1.6/libpng16-16_${LIBPNG_VERSION}_armhf.deb" \
      "libp/libpng1.6/libpng-dev_${LIBPNG_VERSION}_armhf.deb" \
      "b/brotli/libbrotli1_${BROTLI_VERSION}_armhf.deb" \
      "b/brotli/libbrotli-dev_${BROTLI_VERSION}_armhf.deb" \
    ; do \
      curl --retry 5 -fsSLO "$pool/$deb" \
      && dpkg -x "$(basename "$deb")" /opt/sysroot; \
    done \
 && rm -f /tmp/*.deb \
 && ls /opt/sysroot/usr/lib/arm-linux-gnueabihf/libSDL2*.so \
       /opt/sysroot/usr/lib/arm-linux-gnueabihf/libfontconfig.so \
       /opt/sysroot/usr/lib/arm-linux-gnueabihf/libz.so \
       /opt/sysroot/usr/include/zlib.h \
 && PKG_CONFIG_LIBDIR=/opt/sysroot/usr/lib/arm-linux-gnueabihf/pkgconfig \
    PKG_CONFIG_SYSROOT_DIR=/opt/sysroot \
    pkg-config --libs --cflags fontconfig sdl2
