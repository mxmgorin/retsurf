#!/usr/bin/env bash
# Assemble the Allium card layout around a freshly cross-built binary.
#
#   tools/armhf/package-allium.sh [-n]      # -n: skip the build, package what is there
#
# Produces dist/allium/ (the SD-card tree) and dist/retsurf-allium.zip.
#
# Three sets of files come from outside this repo, because none of them are ours
# to keep a second copy of:
#
#   RETSURF_SDL_LIB   the Miyoo SDL2 build and its shims, from a sibling port
#                     (retsend's `onionos/App/Retsend/lib`) — the panel needs the
#                     `Mini` video driver and no upstream SDL2 has it
#   the toolchain     libstdc++/libgcc_s, from the build image, so they match the
#                     compiler that built the binary
#   Debian buster     fontconfig and its two dependencies, that being the
#                     glibc-2.28 era; neither firmware ships any of it
#
# Fonts come from the host's DejaVu install, which every distro packages.
set -euo pipefail

build=yes
[ "${1:-}" = "-n" ] && build=no

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
image=retsurf-armhf-cross
sdl_lib=${RETSURF_SDL_LIB:-$HOME/Repos/retsend/onionos/App/Retsend/lib}
fonts_dir=${RETSURF_FONTS_DIR:-/usr/share/fonts/TTF}
dist=$repo/dist/allium
app=$dist/Apps/Retsurf.pak

if [ "$build" = yes ]; then
  # The build log belongs on the terminal; the path it prints last is the value.
  bin=$("$here/build.sh" | tee /dev/stderr | tail -1)
else
  bin=${RETSURF_ARM_BIN:-${RETSURF_ARM_CACHE:-$HOME/.cache/retsurf-armhf}/target/armv7-unknown-linux-gnueabihf/release/retsurf}
fi
[ -f "$bin" ] || { echo "no binary at $bin" >&2; exit 1; }

for f in libSDL2-2.0.so.0 libEGL.so libjson-c.so.5 fallback/libGLESv2.so fallback/libshmvar.so; do
  [ -f "$sdl_lib/$f" ] || { echo "missing $sdl_lib/$f (set RETSURF_SDL_LIB)" >&2; exit 1; }
done

# The trust store, from wherever this distribution keeps its extracted bundle.
# Servo's TLS verifier reads the platform store and panics when it finds none,
# and no Miyoo firmware has one.
ca=${RETSURF_CA_BUNDLE:-}
if [ -z "$ca" ]; then
  for p in /etc/ssl/certs/ca-certificates.crt \
           /etc/ca-certificates/extracted/tls-ca-bundle.pem \
           /etc/pki/tls/certs/ca-bundle.crt; do
    [ -s "$p" ] && { ca=$p; break; }
  done
fi
[ -s "$ca" ] || { echo "no CA bundle found (set RETSURF_CA_BUNDLE)" >&2; exit 1; }

rm -rf "$dist"
mkdir -p "$app/lib/fallback" "$app/fonts" "$app/etc/fonts" "$app/etc/ssl"

# The runtime the toolchain and Debian owe us, collected in the build image so
# the C++ runtime is the one this compiler pairs with. `RETSURF_NO_DOCKER=1` runs
# the same script directly, for a machine that already has the toolchain unpacked
# — which is what CI is.
if [ "${RETSURF_NO_DOCKER:-0}" = 1 ]; then
  "$here/runtime-libs.sh" "$app/lib"
else
  docker run --rm -i --network host -v "$app/lib":/out \
    -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
    "$image" bash -s /out < "$here/runtime-libs.sh"
fi

cp -a "$sdl_lib/libSDL2-2.0.so.0" "$sdl_lib/libEGL.so" "$sdl_lib/libjson-c.so.5" "$app/lib/"
cp -a "$sdl_lib/fallback/libGLESv2.so" "$sdl_lib/fallback/libshmvar.so" "$app/lib/fallback/"
[ -f "$sdl_lib/README.md" ] && cp -a "$sdl_lib/README.md" "$app/lib/"

# Sans in three styles, serif in three, one mono: enough for the web's generic
# families and the ones `fonts.conf.in` maps onto them, and small enough that the
# cache and the open faces stay cheap on a 128 MB device.
for f in DejaVuSans DejaVuSans-Bold DejaVuSans-Oblique \
         DejaVuSerif DejaVuSerif-Bold DejaVuSerif-Italic DejaVuSansMono; do
  cp "$fonts_dir/$f.ttf" "$app/fonts/"
done

cp "$repo/allium/Apps/Retsurf.pak/config.json" "$app/"
cp "$repo/allium/Apps/Retsurf.pak/launch.sh" "$app/"
cp "$repo/allium/Apps/Retsurf.pak/etc/fonts/fonts.conf.in" "$app/etc/fonts/"
cp "$ca" "$app/etc/ssl/cacert.pem"
cp -r "$repo/allium/Apps/Retsurf.pak/ports" "$app/"
cp "$repo/allium/README.md" "$app/"
cp "$repo/resources/icon.png" "$app/"
install -m 755 "$bin" "$app/retsurf"
chmod 755 "$app/launch.sh" "$app/ports/Retsurf.port/launch.sh"

# The tier `auto` would pick anyway on 128 MB, pinned so a device reporting an
# odd MemTotal cannot choose a heavier one. A template, not `data/config.toml`:
# the launcher installs it only when there is no config to keep, so updating the
# package never costs the user their settings.
cat > "$app/etc/config.toml" <<'EOF'
[browser]
# One tab: a second one is a second engine's worth of memory on a device that
# already browses out of swap, and every navigation replaces rather than stacks.
max_tabs = 1

[performance]
memory_profile = "micro"

# Decoded images are what this device runs out of memory on: 87 MB of WebRender
# image memory on one gallery page, 9 MB at this cap. Settings -> Data saving.
[data_saving]
max_images_per_page = 12

[display]
software_render = true
EOF

zip_out=$repo/dist/retsurf-allium.zip
rm -f "$zip_out"
if command -v zip >/dev/null 2>&1; then
  (cd "$dist" && zip -qr "$zip_out" Apps)
else
  # No zip on this host. Python's writes no permission bits, which costs nothing
  # here: the card is FAT32 and the mount decides the mode for every file on it.
  (cd "$dist" && python3 -m zipfile -c "$zip_out" Apps)
fi
du -sh "$app" "$zip_out"
find "$dist" -type f | sed "s|$dist/||" | sort
