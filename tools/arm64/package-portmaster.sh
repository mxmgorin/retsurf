#!/usr/bin/env bash
# Assemble the PortMaster port around freshly cross-built aarch64 binaries.
#
#   tools/arm64/package-portmaster.sh [-n]   # -n: skip the build, package what is there
#
# Produces dist/portmaster/ (the port tree) and dist/retsurf-portmaster.zip, laid
# out as the Linux ARM workflow's `package` job does: only the launcher at the
# port root, everything else in the retsurf/ gamedir.
set -euo pipefail

build=yes
[ "${1:-}" = "-n" ] && build=no

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
bins=$repo/dist/arm64
pkg=$repo/packaging/portmaster
pm=$repo/dist/portmaster

[ "$build" = yes ] && "$here/build.sh" a35 a53 a55

for cpu in a35 a53 a55; do
  [ -f "$bins/retsurf.$cpu" ] || { echo "missing $bins/retsurf.$cpu (run build.sh)" >&2; exit 1; }
done

rm -rf "$pm"
mkdir -p "$pm/retsurf"
cp "$pkg/Retsurf.sh" "$pm/"
cp "$pkg/port.json" "$pm/retsurf/"
cp "$pkg/gameinfo.xml" "$pm/retsurf/"
cp "$pkg/README.md" "$pm/retsurf/"
cp "$pkg/screenshot.png" "$pm/retsurf/"
# Bundled gamedir assets (licenses/), minus the placeholder.
cp -r "$pkg/retsurf/." "$pm/retsurf/"
rm -f "$pm/retsurf/.gitkeep"
cp "$bins"/retsurf.a35 "$bins"/retsurf.a53 "$bins"/retsurf.a55 "$pm/retsurf/"
chmod +x "$pm/Retsurf.sh" "$pm/retsurf/retsurf.a35" \
  "$pm/retsurf/retsurf.a53" "$pm/retsurf/retsurf.a55"

# Bundled libraries and fontconfig, reached only where the firmware carries none
# of its own (see tools/arm64/runtime-libs.sh).
"$here/runtime-libs.sh" "$pm/retsurf/libs" >/dev/null
mkdir -p "$pm/retsurf/etc/fonts"
cp "$pkg/fonts.conf.in" "$pm/retsurf/etc/fonts/"

# A device without fontconfig may register no fonts either, so the port carries
# the three families its config names. RETSURF_FONTS_DIR overrides the search.
fonts=${RETSURF_FONTS_DIR:-}
if [ -z "$fonts" ]; then
  for d in /usr/share/fonts/truetype/dejavu /usr/share/fonts/TTF /usr/share/fonts/dejavu; do
    [ -f "$d/DejaVuSans.ttf" ] && { fonts=$d; break; }
  done
fi
[ -n "$fonts" ] || { echo "no DejaVu fonts found; set RETSURF_FONTS_DIR" >&2; exit 1; }
mkdir -p "$pm/retsurf/fonts"
for f in DejaVuSans DejaVuSans-Bold DejaVuSerif DejaVuSerif-Bold \
  DejaVuSansMono DejaVuSansMono-Bold; do
  cp "$fonts/$f.ttf" "$pm/retsurf/fonts/"
done

( cd "$pm" && rm -f "$repo/dist/retsurf-portmaster.zip" && zip -qr "$repo/dist/retsurf-portmaster.zip" . )
sha256sum "$repo/dist/retsurf-portmaster.zip" > "$repo/dist/retsurf-portmaster.zip.sha256"

du -sh "$pm"
ls -la "$repo/dist/retsurf-portmaster.zip"
