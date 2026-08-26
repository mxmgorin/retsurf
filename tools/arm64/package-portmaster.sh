#!/usr/bin/env bash
# Assemble the PortMaster port around freshly cross-built aarch64 binaries.
#
#   tools/arm64/package-portmaster.sh [-n]   # -n: skip the build, package what is there
#
# Produces dist/portmaster/ (the port tree) and dist/retsurf-portmaster.zip,
# laid out the way the Linux ARM workflow's `package` job does: only the
# launcher at the port root (it installs to /roms/ports/Retsurf.sh), everything
# else in the retsurf/ gamedir.
set -euo pipefail

build=yes
[ "${1:-}" = "-n" ] && build=no

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
bins=$repo/dist/arm64
pm=$repo/dist/portmaster

[ "$build" = yes ] && "$here/build.sh" a35 a53 a55

for cpu in a35 a53 a55; do
  [ -f "$bins/retsurf.$cpu" ] || { echo "missing $bins/retsurf.$cpu (run build.sh)" >&2; exit 1; }
done

rm -rf "$pm"
mkdir -p "$pm/retsurf"
cp "$repo/portmaster/Retsurf.sh" "$pm/"
cp "$repo/portmaster/port.json" "$pm/retsurf/"
cp "$repo/portmaster/gameinfo.xml" "$pm/retsurf/"
cp "$repo/portmaster/README.md" "$pm/retsurf/"
cp "$repo/portmaster/screenshot.png" "$pm/retsurf/"
# Bundled gamedir assets (licenses/), minus the placeholder.
cp -r "$repo/portmaster/retsurf/." "$pm/retsurf/"
rm -f "$pm/retsurf/.gitkeep"
cp "$bins"/retsurf.a35 "$bins"/retsurf.a53 "$bins"/retsurf.a55 "$pm/retsurf/"
chmod +x "$pm/Retsurf.sh" "$pm/retsurf/retsurf.a35" \
  "$pm/retsurf/retsurf.a53" "$pm/retsurf/retsurf.a55"

( cd "$pm" && rm -f "$repo/dist/retsurf-portmaster.zip" && zip -qr "$repo/dist/retsurf-portmaster.zip" . )
sha256sum "$repo/dist/retsurf-portmaster.zip" > "$repo/dist/retsurf-portmaster.zip.sha256"

du -sh "$pm"
ls -la "$repo/dist/retsurf-portmaster.zip"
