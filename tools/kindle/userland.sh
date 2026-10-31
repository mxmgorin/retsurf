#!/usr/bin/env bash
# The libraries the Kindle package brings, the firmware having no hard-float
# userland: glibc and its loader from the Miyoo sysroot (2.28, the binary's
# floor), an SDL2 with only the dummy and offscreen drivers, and the fontconfig
# chain. Run where the Miyoo toolchain is: inside the build image, or on a host
# that unpacked it (CI).
#
#   userland.sh <lib-dir> <runtime-libs.sh>
#
# $TOOLCHAIN (default /opt/miyoomini-toolchain); $HOST_UID/$HOST_GID, when set,
# hand <lib-dir> back to its owner.
set -euxo pipefail

out=${1:?usage: userland.sh <lib-dir> <runtime-libs.sh>}
runtime_libs=${2:?usage: userland.sh <lib-dir> <runtime-libs.sh>}
tc=${TOOLCHAIN:-/opt/miyoomini-toolchain}
sdl_version=2.30.9
mkdir -p "$out"
out=$(cd "$out" && pwd)

sys=$tc/arm-linux-gnueabihf/libc/lib
for n in ld-linux-armhf.so.3 libc.so.6 libm.so.6 libdl.so.2 librt.so.1 \
  libpthread.so.0 libresolv.so.2 libnss_dns.so.2 libnss_files.so.2; do
  cp -L "$sys/$n" "$out/$n"
done

work=$(mktemp -d)
cd "$work"
# The toolchain's GCC, so SDL links against the sysroot's glibc.
curl --retry 5 -fsSLO "https://github.com/libsdl-org/SDL/releases/download/release-$sdl_version/SDL2-$sdl_version.tar.gz"
tar xf "SDL2-$sdl_version.tar.gz"
cmake -S "SDL2-$sdl_version" -B sdl -DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=armv7 \
  -DCMAKE_C_COMPILER="$tc/bin/arm-linux-gnueabihf-gcc" -DCMAKE_BUILD_TYPE=Release \
  -DSDL_STATIC=OFF -DSDL_TEST=OFF -DSDL_X11=OFF -DSDL_WAYLAND=OFF -DSDL_KMSDRM=OFF \
  -DSDL_OPENGL=OFF -DSDL_OPENGLES=OFF -DSDL_VULKAN=OFF -DSDL_PULSEAUDIO=OFF \
  -DSDL_PIPEWIRE=OFF -DSDL_JACK=OFF -DSDL_SNDIO=OFF -DSDL_ALSA=OFF -DSDL_DBUS=OFF \
  -DSDL_IBUS=OFF -DSDL_LIBUDEV=OFF >/dev/null
make -C sdl -j"$(nproc)" >/dev/null
cp -L sdl/libSDL2-2.0.so.0 "$out/"

# fontconfig's freetype chain; the firmware has only soft-float builds.
pool=http://archive.debian.org/debian/pool/main
mkdir debs
for deb in \
  "f/freetype/libfreetype6_2.9.1-3+deb10u3_armhf.deb" \
  "libp/libpng1.6/libpng16-16_1.6.36-6_armhf.deb" \
  "z/zlib/zlib1g_1.2.11.dfsg-1+deb10u1_armhf.deb" \
; do
  curl --retry 5 -fsSL -o "debs/$(basename "$deb")" "$pool/$deb"
  dpkg -x "debs/$(basename "$deb")" x
done
for n in libfreetype.so.6 libpng16.so.16 libz.so.1; do
  cp -L "$(find x -name "$n" | head -1)" "$out/$n"
done
cd /
rm -rf "$work"
TOOLCHAIN=$tc bash "$runtime_libs" "$out"
[ -z "${HOST_UID:-}" ] || chown -R "$HOST_UID:$HOST_GID" "$out"
