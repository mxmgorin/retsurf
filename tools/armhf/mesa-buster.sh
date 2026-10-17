#!/usr/bin/env bash
# Stage Mesa's software GL for the Miyoo into a directory: libEGL (glvnd), the
# Mesa EGL vendor, the swrast DRI driver (llvmpipe and softpipe) and every
# library they load, as Debian buster built them for armhf.
#
#   tools/armhf/mesa-buster.sh OUT_DIR
#
# buster because it is the glibc-2.28 era, the device's floor. The launchers put
# OUT_DIR last on the library path, so libraries the firmware already has win;
# libstdc++ is left out because the package ships a newer one.
set -euo pipefail

out=${1:?usage: $(basename "$0") OUT_DIR}
pool=http://archive.debian.org/debian/pool/main
arch=arm-linux-gnueabihf
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

pkgs=(
  libb/libbsd/libbsd0_0.9.1-2+deb10u1_armhf.deb
  libd/libdrm/libdrm-amdgpu1_2.4.97-1_armhf.deb
  libd/libdrm/libdrm-etnaviv1_2.4.97-1_armhf.deb
  libd/libdrm/libdrm-nouveau2_2.4.97-1_armhf.deb
  libd/libdrm/libdrm-radeon1_2.4.97-1_armhf.deb
  libd/libdrm/libdrm2_2.4.97-1_armhf.deb
  libe/libedit/libedit2_3.1-20181209-1_armhf.deb
  m/mesa/libegl-mesa0_18.3.6-2+deb10u1_armhf.deb
  libg/libglvnd/libegl1_1.1.0-1_armhf.deb
  e/elfutils/libelf1_0.176-1.1_armhf.deb
  e/expat/libexpat1_2.2.6-2+deb10u4_armhf.deb
  libf/libffi/libffi6_3.2.1-9_armhf.deb
  m/mesa/libgbm1_18.3.6-2+deb10u1_armhf.deb
  m/mesa/libgl1-mesa-dri_18.3.6-2+deb10u1_armhf.deb
  m/mesa/libglapi-mesa_18.3.6-2+deb10u1_armhf.deb
  libg/libglvnd/libglvnd0_1.1.0-1_armhf.deb
  l/llvm-toolchain-7/libllvm7_7.0.1-8+deb10u2_armhf.deb
  libg/libglvnd/libopengl0_1.1.0-1_armhf.deb
  l/lm-sensors/libsensors5_3.5.0-3_armhf.deb
  n/ncurses/libtinfo6_6.1+20181013-2+deb10u2_armhf.deb
  w/wayland/libwayland-client0_1.16.0-1_armhf.deb
  w/wayland/libwayland-server0_1.16.0-1_armhf.deb
  libx/libx11/libx11-xcb1_1.6.7-1+deb10u2_armhf.deb
  libx/libxau/libxau6_1.0.8-1+b2_armhf.deb
  libx/libxcb/libxcb-dri2-0_1.13.1-2_armhf.deb
  libx/libxcb/libxcb-dri3-0_1.13.1-2_armhf.deb
  libx/libxcb/libxcb-present0_1.13.1-2_armhf.deb
  libx/libxcb/libxcb-sync1_1.13.1-2_armhf.deb
  libx/libxcb/libxcb-xfixes0_1.13.1-2_armhf.deb
  libx/libxcb/libxcb1_1.13.1-2_armhf.deb
  libx/libxdmcp/libxdmcp6_1.1.2-3_armhf.deb
  libx/libxshmfence/libxshmfence1_1.3-1_armhf.deb
  z/zlib/zlib1g_1.2.11.dfsg-1+deb10u1_armhf.deb
)

mkdir -p "$work/debs" "$work/root"
for p in "${pkgs[@]}"; do
  curl --retry 5 -fsSL -o "$work/debs/$(basename "$p")" "$pool/$p"
done
for deb in "$work"/debs/*.deb; do
  if command -v dpkg-deb >/dev/null; then
    dpkg-deb -x "$deb" "$work/root"
  else
    bsdtar -xOf "$deb" 'data.tar.*' | bsdtar -xf - -C "$work/root"
  fi
done

lib=$work/root/usr/lib/$arch
find_lib() {
  for d in "$lib" "$work/root/lib/$arch" "$lib/dri"; do
    [ -e "$d/$1" ] && { echo "$d/$1"; return; }
  done
}

# The device provides glibc; the package provides the C++ runtime.
skip=" libc.so.6 libm.so.6 libdl.so.2 libpthread.so.0 librt.so.1 ld-linux-armhf.so.3 libgcc_s.so.1 libstdc++.so.6 "
todo=(libEGL.so.1 libEGL_mesa.so.0 libGLdispatch.so.0 swrast_dri.so)
done_list=" "
mkdir -p "$out/dri"
while [ ${#todo[@]} -gt 0 ]; do
  name=${todo[0]}; todo=("${todo[@]:1}")
  case "$skip$done_list" in *" $name "*) continue ;; esac
  done_list="$done_list$name "
  path=$(find_lib "$name")
  [ -n "$path" ] || { echo "not in the package set: $name" >&2; exit 1; }
  if [ "$name" = swrast_dri.so ]; then
    cp -L "$path" "$out/dri/"
  else
    cp -L "$path" "$out/"
  fi
  while read -r dep; do todo+=("$dep"); done < <(readelf -d "$path" | sed -n 's/.*NEEDED.*\[\(.*\)\]/\1/p')
done

# glvnd finds the vendor through this file; `library_path` is a bare soname, so
# the library path the launcher sets is what resolves it.
cp "$work/root/usr/share/glvnd/egl_vendor.d/50_mesa.json" "$out/"
du -sh "$out"
