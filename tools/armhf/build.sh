#!/usr/bin/env bash
# Cross-build the Miyoo Mini binary the way CI does, and print its path.
#
#   tools/armhf/build.sh [cargo args...]
#
# Caches live outside the repo so container-root files never mix with host
# builds. RETSURF_ARM_LTO=thin trades a slower binary for a link that fits in
# less RAM; the default matches CI.
set -euo pipefail

target=armv7-unknown-linux-gnueabihf
libdir=arm-linux-gnueabihf

image=retsurf-armhf-cross
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
cache=${RETSURF_ARM_CACHE:-$HOME/.cache/retsurf-armhf}
# The glibc floor, from the oldest userland we target (OnionOS). The toolchain's
# own libc is this version, so the floor holds by construction; the check at the
# end guards against a stray link against something newer.
floor=$(cat "$here/glibc-floor")

mkdir -p "$cache/target" "$cache/cargo"
# Cached after the first run. --network host for the same reason as the run
# below: the toolchain tarball and the Debian debs are fetched from the network,
# and the bridge is not always a route out.
docker build -q -t "$image" --network host -f "$here/Dockerfile" "$here" >/dev/null

# A local Servo checkout, mounted at the path a `[patch]` in Cargo.toml names, so
# a fix can be built for the device before it reaches the fork. Unset normally.
servo_mount=()
if [ -n "${RETSURF_SERVO_SRC:-}" ]; then
  src=$(cd "$RETSURF_SERVO_SRC" && pwd)
  servo_mount=(-v "$src":"$src")
fi

# --network host: the Servo fork and inputbind are fetched from git.
# The container script arrives on stdin under a quoted heredoc rather than as a
# quoted argument, so an apostrophe in a comment cannot end it early.
docker run --rm -i --network host \
  -v "$repo":/repo \
  "${servo_mount[@]}" \
  -v "$cache/target":/target \
  -v "$cache/cargo":/cargo \
  -e CARGO_TARGET_DIR=/target -e CARGO_HOME=/cargo -e RUSTUP_HOME=/cargo \
  -e "TARGET=$target" -e "LIBDIR=$libdir" -e "FLOOR=$floor" \
  -e "LTO=${RETSURF_ARM_LTO:-fat}" \
  -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
  -e "CARGO_ARGS=$*" \
  "$image" bash -euxs <<'CONTAINER'
    # /opt/cc-shims first: it holds one g++ wrapper that respells `-std=c++20`
    # for GCC 8.3 (see the Dockerfile). Shimming by name rather than by `CXX_*`
    # keeps the compiler every build script records unchanged.
    export PATH="/cargo/bin:/opt/cc-shims:/opt/miyoomini-toolchain/bin:$PATH"
    command -v cargo >/dev/null || curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --default-toolchain none
    # build.rs stamps the About screen from git; without this the mounted repo
    # belongs to another user and git refuses it, leaving "unknown".
    git config --global --add safe.directory /repo
    cd /repo
    rustup target add "$TARGET"

    tc=/opt/miyoomini-toolchain
    sys=$tc/arm-linux-gnueabihf/libc
    export CC_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-gcc
    export CXX_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-g++
    export AR_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-ar
    export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc

    # The SSD202D is always a Cortex-A7. On the Rust side +neon is not redundant
    # with target-cpu: the target spec disables NEON outright and target-cpu does
    # not undo that (verified in the emitted asm), so dropping it silently costs
    # vectorisation. It warns about being unstable, once per crate.
    # --allow-shlib-undefined: this SDL2 pulls in X11/wayland/alsa/gbm, which
    # neither sysroot has and we never call.
    # The binary's own text is 64 MB demand-paged from the SD card, and the
    # device took 191k major faults in one session reading it back.
    #   -Wl,--gc-sections drops what the C/C++ halves never reference;
    #   relocation-model=static skips a PIE's relocation work at every launch.
    export RUSTFLAGS="-L /opt/sysroot/usr/lib/$LIBDIR \
      -C link-arg=-Wl,--allow-shlib-undefined \
      -C link-arg=-Wl,--gc-sections \
      -C relocation-model=static -C link-arg=-no-pie \
      -C target-cpu=cortex-a7 -C target-feature=+neon"
    # -mtune, never -mcpu: cc-rs passes -march=armv7-a of its own, and GCC warns
    # that -mcpu conflicts with it. That warning is fatal in a way that looks
    # like nothing -- cc-rs reads any stderr from a flag probe as "unsupported"
    # (lib.rs: `status.success() && stderr.is_empty()`), so one warning makes it
    # drop every flag_if_supported flag in the graph. mozjs_sys asks for
    # -fno-rtti and -fno-sized-deallocation that way: the first breaks the link
    # against the no-rtti SpiderMonkey, the second is a silent ABI mismatch on
    # operator delete.
    # -mfpu comes last and overrides the vfpv3-d16 that cc-rs passes, which has
    # no NEON; swgl without NEON falls back to scalar.
    # -I: the Debian sysroot headers (zlib for libpng, see the Dockerfile). The
    # toolchain finds its own headers without help.
    # One section per function/datum, so the link above can drop the unreached.
    export CFLAGS_armv7_unknown_linux_gnueabihf="-mtune=cortex-a7 -mfpu=neon-vfpv4 \
      -ffunction-sections -fdata-sections \
      -I/opt/sysroot/usr/include"
    export CXXFLAGS_armv7_unknown_linux_gnueabihf="$CFLAGS_armv7_unknown_linux_gnueabihf"

    # bindgen runs the host libclang against target headers, so it needs pointing
    # at both sysroots by hand -- it inherits nothing from the cross gcc.
    export BINDGEN_EXTRA_CLANG_ARGS_armv7_unknown_linux_gnueabihf="\
      --target=$TARGET --sysroot=$sys \
      -isystem $tc/arm-linux-gnueabihf/include/c++/8.3.0 \
      -isystem $tc/arm-linux-gnueabihf/include/c++/8.3.0/arm-linux-gnueabihf \
      -isystem $tc/lib/gcc/arm-linux-gnueabihf/8.3.0/include \
      -isystem $tc/arm-linux-gnueabihf/include \
      -isystem /opt/sysroot/usr/include"

    # SpiderMonkey builds tools that run on the build machine, and its configure
    # falls back to the target compiler when these are unset.
    export HOST_CC=clang
    export HOST_CXX=clang++
    # Only fontconfig is probed; everything else in the graph builds from source
    # once the probe fails, which is why the sysroot carries so little.
    export PKG_CONFIG_ALLOW_CROSS=1
    export PKG_CONFIG_SYSROOT_DIR=/opt/sysroot
    export PKG_CONFIG_LIBDIR="/opt/sysroot/usr/lib/$LIBDIR/pkgconfig"
    export CARGO_PROFILE_RELEASE_LTO="$LTO"
    export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
    # webgl off, so the surfman probe -- our only catch_unwind -- is gone and
    # unwind tables with it. Same trade as the aarch64 handheld build.
    export CARGO_PROFILE_RELEASE_PANIC=abort
    # Size over speed for the bulk of the Rust: the engine's code is cold and
    # there is a lot of it. The rasterizers keep -O3, see Cargo.toml.
    export CARGO_PROFILE_RELEASE_OPT_LEVEL="${RETSURF_ARM_OPT:-s}"

    cargo build --release --no-default-features --features software --target "$TARGET" $CARGO_ARGS
    out="/target/$TARGET/release/retsurf"
    file "$out"
    arm-linux-gnueabihf-readelf -d "$out" | grep NEEDED
    arm-linux-gnueabihf-readelf -V "$out" | grep -o "GLIBC_2\.[0-9]*" | sort -uV | tr "\n" " "
    echo
    # Above the floor the loader on the device refuses it, so fail here instead.
    newer=$(arm-linux-gnueabihf-readelf -V "$out" | grep -o "GLIBC_2\.[0-9]*" | sort -uV \
      | awk -F. -v f="${FLOOR#2.}" "\$2 > f" | tr "\n" " ")
    [ -z "$newer" ] || { echo "binary requires $newer; the floor is GLIBC_$FLOOR" >&2; exit 1; }
    chown -R "$HOST_UID:$HOST_GID" /target /cargo
CONTAINER

echo "$cache/target/$target/release/retsurf"
