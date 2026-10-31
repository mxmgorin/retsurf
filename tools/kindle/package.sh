#!/usr/bin/env bash
# Assemble the Kindle package around a built binary, and its zip beside it.
# <out-dir> (default dist/kindle) mirrors the root of the Kindle's USB storage:
# the app, its KUAL extension and its KOReader plugin each land where the device
# looks for them, so the zip unpacks straight onto it.
#
#   tools/kindle/package.sh <binary> [out-dir]
#
# The libraries come from userland.sh, in the armhf build image;
# `RETSURF_NO_DOCKER=1` runs it directly where the toolchain is unpacked (CI).
set -euo pipefail

bin=$(readlink -f "${1:?usage: package.sh <binary> [out-dir]}")
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
out=${2:-$repo/dist/kindle}
zip_name=retsurf-kindle
# Fonts come from the host's DejaVu install.
fonts_dir=${RETSURF_FONTS_DIR:-/usr/share/fonts/TTF}

pkg=$repo/packaging/kindle
# Only what this script writes: <out-dir> may hold other things.
rm -rf "$out/retsurf" "$out/extensions/retsurf" "$out/koreader/plugins/retsurf.koplugin"
mkdir -p "$out/extensions" "$out/koreader/plugins"
cp -r "$pkg/kual" "$out/extensions/retsurf"
cp -r "$pkg/koreader/retsurf.koplugin" "$out/koreader/plugins/"
out=$(cd "$out" && pwd)/retsurf
mkdir -p "$out/lib" "$out/etc" "$out/fonts"
cp "$bin" "$out/retsurf"
cp "$pkg/retsurf.sh" "$pkg/from-koreader.sh" "$repo/LICENSE" "$repo/NOTICE" "$out/"
cp "$pkg/config.toml" "$out/etc/config.toml"
cp "$repo/packaging/miyoo/shared/fonts.conf.in" "$out/etc/"
for f in DejaVuSans DejaVuSans-Bold DejaVuSans-Oblique \
         DejaVuSerif DejaVuSerif-Bold DejaVuSerif-Italic DejaVuSansMono; do
  cp "$fonts_dir/$f.ttf" "$out/fonts/"
done
cp /etc/ssl/certs/ca-certificates.crt "$out/etc/cacert.pem"

if [ "${RETSURF_NO_DOCKER:-0}" = 1 ]; then
  "$here/userland.sh" "$out/lib" "$repo/tools/armhf/runtime-libs.sh"
else
  docker run --rm -i --network host -v "$out":/out -v "$repo":/repo:ro \
    -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
    retsurf-armhf-cross bash -s /out/lib /repo/tools/armhf/runtime-libs.sh \
    < "$here/userland.sh"
fi

root=$(dirname "$out")
zip_out=$(dirname "$root")/$zip_name.zip
rm -f "$zip_out"
if command -v zip >/dev/null 2>&1; then
  (cd "$root" && zip -qr "$zip_out" retsurf extensions koreader)
else
  (cd "$root" && python3 -m zipfile -c "$zip_out" retsurf extensions koreader)
fi
echo "$zip_out"
