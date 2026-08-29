#!/usr/bin/env bash
# The shared libraries the package owes the device but the firmware does not
# have: the C++ runtime this compiler pairs with, and the fontconfig chain.
#
#   runtime-libs.sh <lib-dir>
#
# Wants the Miyoo toolchain at $TOOLCHAIN and a network. Run it inside the build
# image (where both are given) or on a host that unpacked the toolchain itself —
# CI does the latter, and packaging locally does the former, which is why this is
# a script and not a heredoc in [`package-allium.sh`].
#
# $HOST_UID/$HOST_GID, when set, hand the result back to the user who owns the
# directory the container wrote into.
set -euxo pipefail

out=${1:?usage: runtime-libs.sh <lib-dir>}
toolchain=${TOOLCHAIN:-/opt/miyoomini-toolchain}
tc=$toolchain/arm-linux-gnueabihf
mkdir -p "$out"

# The C++ runtime this compiler pairs with, plus its libgcc. The glob ends on a
# digit so it cannot also take the `-gdb.py` sitting beside it.
stdcxx=$(ls "$tc/lib"/libstdc++.so.6.[0-9]*[0-9])
cp -a "$stdcxx" "$tc/libc/lib/libgcc_s.so.1" "$out/"
# A copy, not a link: the card is FAT32, which has neither symlinks nor
# hardlinks, and the soname is the name the loader actually opens.
cp -a "$stdcxx" "$out/libstdc++.so.6"

# buster is the glibc-2.28 era, which is what the device runs.
pool=http://archive.debian.org/debian/pool/main
work=$(mktemp -d)
cd "$work"
curl --retry 5 -fsSLO "$pool/f/fontconfig/libfontconfig1_2.13.1-2_armhf.deb"
curl --retry 5 -fsSLO "$pool/e/expat/libexpat1_2.2.6-2+deb10u4_armhf.deb"
curl --retry 5 -fsSLO "$pool/u/util-linux/libuuid1_2.33.1-0.1_armhf.deb"
for f in *.deb; do dpkg -x "$f" "$work/fc"; done
# By name, not by glob: the packages also carry a wide-character expat the
# binary never names.
for name in libfontconfig.so.1 libexpat.so.1 libuuid.so.1; do
  real=$(readlink -f "$(find "$work/fc" -name "$name" | head -1)")
  cp -a "$real" "$out/"
  cp -a "$real" "$out/$name"
done
rm -rf "$work"

# The toolchain ships its runtime with debug info — 19 MB of it, and none of it
# loaded at run time. Regular files only: strip would replace a symlink with a
# copy of its target.
find "$out" -maxdepth 1 -type f -name '*.so*' \
  -exec "$toolchain/bin/arm-linux-gnueabihf-strip" {} +

[ -z "${HOST_UID:-}" ] || chown -R "$HOST_UID:${HOST_GID:-$HOST_UID}" "$out"
