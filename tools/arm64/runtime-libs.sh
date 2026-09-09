#!/usr/bin/env bash
# The fontconfig chain, for a firmware that ships none. Only two ports in all of
# PortMaster carry these, so the port cannot assume the device has them either.
#
#   runtime-libs.sh <lib-dir>
#
# `Retsurf.sh` puts this directory on the search path only when the loader
# reports something missing, so where the firmware has its own, nothing changes.
#
# buster is the glibc-2.28 era, comfortably under `glibc-floor`, and Debian ships
# these stripped already.
set -euxo pipefail

out=${1:?usage: runtime-libs.sh <lib-dir>}
mkdir -p "$out"

pool=http://archive.debian.org/debian/pool/main
work=$(mktemp -d)
cd "$work"
curl --retry 5 -fsSLO "$pool/f/fontconfig/libfontconfig1_2.13.1-2_arm64.deb"
curl --retry 5 -fsSLO "$pool/e/expat/libexpat1_2.2.6-2+deb10u4_arm64.deb"
curl --retry 5 -fsSLO "$pool/u/util-linux/libuuid1_2.33.1-0.1_arm64.deb"
# `ar` and not `dpkg -x`: packaging also runs on hosts that are not Debian.
mkdir -p "$work/x"
for f in *.deb; do
  ar x "$f"
  tar -xf data.tar.* -C "$work/x"
  rm -f data.tar.* control.tar.* debian-binary
done

# By name, not by glob: the packages also carry a wide-character expat the binary
# never names. A copy under the soname, not a link — a FAT32 card has neither.
for name in libfontconfig.so.1 libexpat.so.1 libuuid.so.1; do
  real=$(readlink -f "$(find "$work/x" -name "$name" | head -1)")
  cp -a "$real" "$out/"
  cp -a "$real" "$out/$name"
done
cd /
rm -rf "$work"

ls -la "$out"
