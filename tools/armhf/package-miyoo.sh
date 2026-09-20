#!/usr/bin/env bash
# Assemble a Miyoo Mini card layout around a freshly cross-built binary.
#
#   tools/armhf/package-miyoo.sh [-n] [allium|onionos ...]
#
# `-n` skips the build; naming no firmware does both. Produces dist/<firmware>/
# (the SD-card tree) and dist/retsurf-<firmware>.zip. Only the card layout
# differs, so the payload is collected once and copied into each.
#
# Three sets of files come from outside this repo, none of them ours to vendor:
#
#   RETSURF_SDL_LIB   the Miyoo SDL2 build and its shims, from a sibling port
#                     (retsend); no upstream SDL2 has the `Mini` video driver
#   the toolchain     libstdc++/libgcc_s from the build image, so they match the
#                     compiler that built the binary
#   Debian buster     fontconfig and its two dependencies, that being the
#                     glibc-2.28 era; neither firmware ships any of it
#
# Fonts come from the host's DejaVu install, which every distro packages.
set -euo pipefail

build=yes
firmwares=()
for arg in "$@"; do
  case $arg in
    -n) build=no ;;
    allium | onionos) firmwares+=("$arg") ;;
    *) echo "usage: $(basename "$0") [-n] [allium|onionos ...]" >&2; exit 2 ;;
  esac
done
[ ${#firmwares[@]} -gt 0 ] || firmwares=(allium onionos)

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
image=retsurf-armhf-cross
sdl_lib=${RETSURF_SDL_LIB:-$HOME/Repos/retsend/onionos/App/Retsend/lib}
fonts_dir=${RETSURF_FONTS_DIR:-/usr/share/fonts/TTF}
pkg=$repo/packaging/miyoo
shared=$pkg/shared
payload=$repo/dist/miyoo-payload

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

# Servo panics when the platform store holds no CA bundle, which is every Miyoo.
# Taken from wherever this distribution keeps its extracted one.
ca=${RETSURF_CA_BUNDLE:-}
if [ -z "$ca" ]; then
  for p in /etc/ssl/certs/ca-certificates.crt \
           /etc/ca-certificates/extracted/tls-ca-bundle.pem \
           /etc/pki/tls/certs/ca-bundle.crt; do
    [ -s "$p" ] && { ca=$p; break; }
  done
fi
[ -s "$ca" ] || { echo "no CA bundle found (set RETSURF_CA_BUNDLE)" >&2; exit 1; }

rm -rf "$payload"
mkdir -p "$payload/lib/fallback" "$payload/fonts"

# Collected in the build image so the C++ runtime matches the compiler that built
# the binary. `RETSURF_NO_DOCKER=1` skips it where the toolchain is unpacked (CI).
if [ "${RETSURF_NO_DOCKER:-0}" = 1 ]; then
  "$here/runtime-libs.sh" "$payload/lib" "$bin"
else
  docker run --rm -i --network host -v "$payload/lib":/out -v "$bin":/bin.arm:ro \
    -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
    "$image" bash -s /out /bin.arm < "$here/runtime-libs.sh"
fi

cp -a "$sdl_lib/libSDL2-2.0.so.0" "$sdl_lib/libEGL.so" "$sdl_lib/libjson-c.so.5" "$payload/lib/"
cp -a "$sdl_lib/fallback/libGLESv2.so" "$sdl_lib/fallback/libshmvar.so" "$payload/lib/fallback/"
[ -f "$sdl_lib/README.md" ] && cp -a "$sdl_lib/README.md" "$payload/lib/"

# The generic families `fonts.conf.in` maps, and no more: on 128 MB the cache and
# the open faces both cost.
for f in DejaVuSans DejaVuSans-Bold DejaVuSans-Oblique \
         DejaVuSerif DejaVuSerif-Bold DejaVuSerif-Italic DejaVuSansMono; do
  cp "$fonts_dir/$f.ttf" "$payload/fonts/"
done

for fw in "${firmwares[@]}"; do
  dist=$repo/dist/$fw
  case $fw in
    # Allium scales the icon it finds, so that one is the 256px source.
    allium)
      src=$pkg/allium/Apps/Retsurf.pak
      app=$dist/Apps/Retsurf.pak
      root=Apps
      icon=$repo/resources/icon.png
      ;;
    # Onion's MainUI draws it at native size, hence the package's own downscale.
    onionos)
      src=$pkg/onionos/App/Retsurf
      app=$dist/App/Retsurf
      root=App
      icon=$src/icon.png
      ;;
  esac

  rm -rf "$dist"
  mkdir -p "$app/etc/fonts" "$app/etc/ssl"
  cp -a "$payload/lib" "$payload/fonts" "$app/"
  cp "$shared/fonts.conf.in" "$app/etc/fonts/"
  cp "$shared/config.toml" "$app/etc/"
  cp "$ca" "$app/etc/ssl/cacert.pem"
  cp "$src/config.json" "$src/launch.sh" "$app/"
  cp "$pkg/$fw/README.md" "$icon" "$app/"
  install -m 755 "$bin" "$app/retsurf"
  chmod 755 "$app/launch.sh"
  # Allium's Games tab entry, which hands over to the install under `Apps/`.
  if [ -d "$src/ports" ]; then
    cp -r "$src/ports" "$app/"
    chmod 755 "$app/ports"/*/launch.sh
  fi

  zip_out=$repo/dist/retsurf-$fw.zip
  rm -f "$zip_out"
  if command -v zip >/dev/null 2>&1; then
    (cd "$dist" && zip -qr "$zip_out" "$root")
  else
    # Python's writes no permission bits, which costs nothing on FAT32.
    (cd "$dist" && python3 -m zipfile -c "$zip_out" "$root")
  fi
  du -sh "$app" "$zip_out"
  find "$dist" -type f | sed "s|$dist/||" | sort
done
