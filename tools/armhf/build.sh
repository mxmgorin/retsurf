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
    # /opt/cc-shims first: it holds the two driver wrappers that put the A7's FPU
    # back (see the Dockerfile). Shimming by name rather than by `CC_*` keeps the
    # compiler every build script records unchanged.
    # The Miyoo toolchain's own bin dir stays off PATH: only its sysroot is used
    # now, and its GCC-8-era ar/readelf would answer for the cross names ahead of
    # the ones matching the compiler.
    export PATH="/cargo/bin:/opt/cc-shims:$PATH"
    command -v cargo >/dev/null || curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --default-toolchain none
    # build.rs stamps the About screen from git; without this the mounted repo
    # belongs to another user and git refuses it, leaving "unknown".
    git config --global --add safe.directory /repo
    cd /repo
    rustup target add "$TARGET"

    tc=/opt/miyoomini-toolchain
    sys=$tc/arm-linux-gnueabihf/libc
    export CC_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-gcc-10
    export CXX_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-g++-10
    export AR_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-ar
    export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc-10

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
    # -B$sys/usr/lib is load-bearing, not an optimisation: it makes gcc take
    # crt1.o and libc_nonshared.a from the glibc-2.28 sysroot. Ubuntu's GCC 10
    # defaults them to its own glibc 2.35, where __libc_csu_init is gone and
    # .init_array is the loader's job (needs glibc >= 2.34). On the device's
    # 2.28 neither path then runs .init_array, so every C++ global constructor
    # is skipped -- SpiderMonkey spins forever on an unconstructed static.
    # --sysroot does not redirect the crt gcc auto-adds; -B does.
    # The link takes libc from the device's sysroot and libstdc++ from the
    # compiler, which is the whole reason this arrangement works: only
    # $sys/usr/lib goes on the search path, never $sys/lib -- that one also holds
    # the toolchain's libstdc++ 8.3, and finding it first would answer
    # -static-libstdc++ with a runtime older than the headers.
    # Static libstdc++/libgcc: nothing then has to ship a C++ runtime that
    # matches both GCC 10 and glibc 2.28, and no such runtime is packaged.
    # /opt/cxx-static holds only the archive, which is what makes cc-rs's own
    # `-lstdc++` static too (see the Dockerfile). `libgcc_s.so.1` is left
    # dynamic: its symbols are GCC_3.0 to GCC_4.3.0, frozen for two decades.
    # -lpthread by hand: glibc folded it into libc in 2.34, so the compiler no
    # longer passes it, and libstdc++'s <thread> needs it on 2.28.
    export RUSTFLAGS="-L /opt/sysroot/usr/lib/$LIBDIR \
      -C link-arg=--sysroot=$sys \
      -C link-arg=-L/opt/cxx-static \
      -C link-arg=-L$sys/usr/lib \
      -C link-arg=-static-libstdc++ -C link-arg=-static-libgcc \
      -C link-arg=-lpthread \
      -C link-arg=-Wl,--allow-shlib-undefined \
      -C link-arg=-Wl,--gc-sections \
      -C relocation-model=static -C link-arg=-no-pie \
      -C link-arg=-B$sys/usr/lib \
      -C target-cpu=cortex-a7 -C target-feature=+neon"
    # -mtune, never -mcpu: cc-rs passes -march=armv7-a of its own, and GCC warns
    # that -mcpu conflicts with it. That warning is fatal in a way that looks
    # like nothing -- cc-rs reads any stderr from a flag probe as "unsupported"
    # (lib.rs: `status.success() && stderr.is_empty()`), so one warning makes it
    # drop every flag_if_supported flag in the graph. mozjs_sys asks for
    # -fno-rtti and -fno-sized-deallocation that way: the first breaks the link
    # against the no-rtti SpiderMonkey, the second is a silent ABI mismatch on
    # operator delete.
    # No -mfpu here: the driver shim appends it after everything, which is what
    # also settles the vfpv3-d16 cc-rs passes (no NEON, and swgl without NEON
    # falls back to scalar).
    # -I: the Debian sysroot headers (zlib for libpng, see the Dockerfile). The
    # toolchain finds its own headers without help.
    # One section per function/datum, so the link above can drop the unreached.
    # --sysroot: the compiler is Ubuntu's, the userland the device's. Without it
    # the C/C++ halves would compile against jammy's glibc 2.35 headers and the
    # link would resolve symbols there, taking the binary over the floor.
    export CFLAGS_armv7_unknown_linux_gnueabihf="--sysroot=$sys \
      -mtune=cortex-a7 \
      -ffunction-sections -fdata-sections \
      -I/opt/sysroot/usr/include"
    export CXXFLAGS_armv7_unknown_linux_gnueabihf="$CFLAGS_armv7_unknown_linux_gnueabihf"

    # bindgen runs the host libclang against target headers, so it needs pointing
    # at both sysroots by hand -- it inherits nothing from the cross gcc.
    # Not the distro's clang: see the Dockerfile for why bindgen needs 19.
    export LIBCLANG_PATH=/usr/lib/llvm-19/lib
    export CLANG_PATH=/usr/bin/clang-19
    # SpiderMonkey's configure hunts for llvm-objdump beside whichever clang it
    # found and gives up if there is none -- it is what the static-libstdc++
    # detection reads. Named outright, since the compiler here is GCC.
    export LLVM_OBJDUMP=/usr/lib/llvm-19/bin/llvm-objdump
    # The C++ headers are the cross compiler's, not the sysroot's: the sysroot
    # carries libstdc++ 8.3 headers and SpiderMonkey requires 10 or newer.
    export BINDGEN_EXTRA_CLANG_ARGS_armv7_unknown_linux_gnueabihf="\
      --target=$TARGET --sysroot=$sys \
      -isystem /usr/arm-linux-gnueabihf/include/c++/10 \
      -isystem /usr/arm-linux-gnueabihf/include/c++/10/arm-linux-gnueabihf \
      -isystem /usr/lib/gcc-cross/arm-linux-gnueabihf/10/include \
      -isystem $tc/arm-linux-gnueabihf/include \
      -isystem /opt/sysroot/usr/include"

    # SpiderMonkey builds tools that run on the build machine, and its configure
    # falls back to the target compiler when these are unset. GCC and not the
    # clang beside it: 153 wants clang 19 or newer and jammy carries 14, while
    # its GCC floor is 10.1 and the host GCC is 11.
    export HOST_CC=gcc
    export HOST_CXX=g++
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
