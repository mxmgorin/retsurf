#!/usr/bin/env bash
# Build FBInk and our C shim over it into one static library for the `eink`
# feature, at target/fbink/libretsurf_eink.a (where build.rs links it from).
#
#   tools/kindle/fbink.sh
#
# In the armhf build image, so the objects match the binary's sysroot;
# `RETSURF_NO_DOCKER=1` skips it where the toolchain is unpacked (CI).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
src=${RETSURF_FBINK_SRC:-$HOME/.cache/retsurf-kindle/src/FBInk}
rev=$(cat "$here/fbink-rev")

if [ ! -d "$src/.git" ]; then
  git clone --recurse-submodules https://github.com/NiLuJe/FBInk.git "$src"
fi
git -C "$src" checkout -q "$rev"
git -C "$src" submodule update -q --init --recursive

out=$repo/target/fbink
mkdir -p "$out"
if [ "${RETSURF_NO_DOCKER:-0}" = 1 ]; then
  "$here/fbink-build.sh" "$src" "$repo/src/platform/eink/fbink_shim.c" "$out"
else
  docker run --rm -i -v "$src":/fbink -v "$repo":/repo \
    -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
    retsurf-armhf-cross bash -s /fbink /repo/src/platform/eink/fbink_shim.c /repo/target/fbink \
    < "$here/fbink-build.sh"
fi
echo "$out/libretsurf_eink.a"
