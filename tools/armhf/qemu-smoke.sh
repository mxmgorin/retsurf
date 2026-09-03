#!/bin/bash
# Does the armhf binary actually run? Answers it against a runtime the device
# would recognise: the toolchain's own glibc 2.28 and SigmaStar MI libraries,
# plus the Miyoo SDL2 build that retleaf already carries.
#
# It rules out a broken cross build, nothing more — qemu has no panel, no MI
# hardware and no memory pressure. Expect it to fall back past GL (the driver
# refuses the attributes), bring up swgl and WebRender, and then die in the
# panel driver it has no hardware for. Anything short of that is the build.
#
# Runs inside the build image, so it wants three mounts:
#
#   docker run --rm --network host \
#     -v "$PWD/tools/armhf":/s \
#     -v "$(dirname "$(tools/armhf/build.sh)")":/work \
#     -v ~/Repos/retleaf/onionos/App/Retleaf/lib:/leaf:ro \
#     retsurf-armhf-cross bash /s/qemu-smoke.sh
#
# /work holds the binary, /leaf is retleaf's OnionOS lib dir.
set -eux

apt-get update -qq
apt-get install -y -qq --no-install-recommends qemu-user >/dev/null

TC=/opt/miyoomini-toolchain
SYS=$TC/arm-linux-gnueabihf/libc
RT=/tmp/rt
rm -rf "$RT"; mkdir -p "$RT"

# The device userland: loader, glibc, the SigmaStar MI libs, freetype/png/bz2.
cp -a "$SYS/lib" "$RT/lib"
mkdir -p "$RT/usr"
cp -a "$SYS/usr/lib" "$RT/usr/lib"
# libstdc++ 6.0.25, the one this compiler pairs with.
cp -a "$TC/arm-linux-gnueabihf/lib/libstdc++.so.6"* "$RT/lib/"

# SDL2 as the device runs it, from retleaf's OnionOS package, plus the shims it
# ships for the two firmware libraries it does not carry.
cp -a /leaf/libSDL2-2.0.so.0 /leaf/libEGL.so /leaf/libjson-c.so.5 "$RT/lib/"
cp -a /leaf/fallback/libGLESv2.so /leaf/fallback/libshmvar.so "$RT/lib/"

# fontconfig is on neither side; buster is the glibc-2.28 era.
pool=http://archive.debian.org/debian/pool/main
cd /tmp && mkdir -p debs && cd debs
curl --retry 5 -fsSLO "$pool/f/fontconfig/libfontconfig1_2.13.1-2_armhf.deb"
curl --retry 5 -fsSLO "$pool/e/expat/libexpat1_2.2.6-2+deb10u4_armhf.deb"
curl --retry 5 -fsSLO "$pool/u/util-linux/libuuid1_2.33.1-0.1_armhf.deb"
for f in *.deb; do dpkg -x "$f" /tmp/fc; done
cp -a /tmp/fc/usr/lib/arm-linux-gnueabihf/*.so* "$RT/lib/" 2>/dev/null || true
cp -a /tmp/fc/lib/arm-linux-gnueabihf/*.so* "$RT/lib/" 2>/dev/null || true

echo "=== glibc floor of every library in the runtime ==="
for so in "$RT"/lib/*.so*; do
  [ -f "$so" ] || continue
  v=$(readelf -V "$so" 2>/dev/null | grep -o 'GLIBC_2\.[0-9]*' | sort -uV | tail -1)
  [ -n "$v" ] && echo "  $(basename "$so"): $v"
done | sort -t: -k2 -V | tail -8

mkdir -p /tmp/data
export LD_LIBRARY_PATH="$RT/lib:$RT/usr/lib"
export SDL_VIDEODRIVER=dummy
export SDL_AUDIODRIVER=dummy
export RETSURF_DATA_DIR=/tmp/data
# `RETSURF_LOG_LEVEL`, not `RUST_LOG`: the app builds its env_logger with that
# name (`src/lib.rs`), so `RUST_LOG` here was doing nothing at all. Overridable —
# `info` says which renderer came up, `debug` how far engine startup got, which
# is the question when it comes up and then stops.
: "${RETSURF_LOG_LEVEL:=info}"
export RETSURF_LOG_LEVEL
export HOME=/tmp

echo "=== run ==="
set +e
# -k, or the deadline is advisory: TERM goes to the emulated program, which
# installs handlers of its own and ignores it, and qemu outlives the timeout.
# Startup under emulation is minutes, so RETSURF_SMOKE_SECS raises the deadline.
timeout -k 10 "${RETSURF_SMOKE_SECS:-120}" qemu-arm -L "$RT" /work/retsurf > /tmp/run.log 2>&1
rc=$?
set -e
echo "exit=$rc"
echo "=== output ==="
head -40 /tmp/run.log
echo "..."
tail -15 /tmp/run.log
