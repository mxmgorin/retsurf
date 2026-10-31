#!/usr/bin/env bash
# The FBInk half of fbink.sh, run where the Miyoo toolchain is: inside the build
# image, or on a host that unpacked it (CI).
#
#   fbink-build.sh <fbink-src> <shim.c> <out-dir>
#
# $TOOLCHAIN (default /opt/miyoomini-toolchain); $HOST_UID/$HOST_GID, when set,
# hand <out-dir> back to its owner.
set -euxo pipefail

src=${1:?usage: fbink-build.sh <fbink-src> <shim.c> <out-dir>}
shim=${2:?usage: fbink-build.sh <fbink-src> <shim.c> <out-dir>}
out=${3:?usage: fbink-build.sh <fbink-src> <shim.c> <out-dir>}
tc=${TOOLCHAIN:-/opt/miyoomini-toolchain}/bin/arm-linux-gnueabihf
mkdir -p "$out"

make -C "$src" clean >/dev/null
# `small`: MINIMAL plus image support, which fbink_print_raw_data needs.
make -C "$src" small KINDLE=1 CROSS_TC="$tc" -j"$(nproc)"
"$tc-gcc" -O2 -fPIC -mtune=cortex-a7 -mfpu=neon-vfpv4 -I"$src" \
  -c "$shim" -o "$out/fbink_shim.o"
cp "$src/Release/libfbink.a" "$out/libretsurf_eink.a"
"$tc-ar" rs "$out/libretsurf_eink.a" "$out/fbink_shim.o"
[ -z "${HOST_UID:-}" ] || chown -R "$HOST_UID:$HOST_GID" "$out"
