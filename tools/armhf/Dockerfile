# Cross-build environment for the Miyoo Mini (SSD202D, armv7, OnionOS), matching
# the Linux ARMhf workflow.
#
# armhf only. The aarch64 handhelds are built natively on ARM runners (see
# build-linux-arm.yml) and have no reason to come through here.
#
# The compiler is the Miyoo Mini community toolchain — ARM's A-profile GCC 8.3
# over a buildroot sysroot built for this device. Three facts made it the choice
# over zig, which is what the sibling handheld repos cross-build with:
#
#   * its glibc is 2.28, the OnionOS floor, so the floor holds by construction
#     rather than by asking for stubs;
#   * GCC 8.3 is exactly SpiderMonkey 140's minimum (moz.configure names 8.1.0
#     and checks _GLIBCXX_RELEASE >= 8);
#   * it links libstdc++ dynamically. zig always links its own libc++ statically
#     and offers no way out (-stdlib=libstdc++ and -nostdlib++ are both ignored),
#     and Mozilla's configure refuses a static C++ runtime outright: "Firefox
#     does not support linking statically with libstdc++".
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
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential clang libclang-dev llvm-dev cmake gperf pkg-config \
      python3 python3-pip python3-setuptools \
      ca-certificates curl git xz-utils binutils file \
 && rm -rf /var/lib/apt/lists/*

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
