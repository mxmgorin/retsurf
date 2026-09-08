#!/usr/bin/env bash
# Cross-build the aarch64 handheld binaries the way the Linux ARM workflow does,
# and print their paths.
#
#   tools/arm64/build.sh [cpu...]        # default: a35 a53 a55
#   tools/arm64/build.sh universal       # the generic non-PortMaster binary
#
# Three per-core binaries exist because Cortex-A55 is ARMv8.2 (LSE atomics,
# fp16, dotprod) and code built for it SIGILLs on the v8.0 cores; `Retsurf.sh`
# picks one at runtime from /proc/cpuinfo.
#
# Caches live outside the repo so container-root files never mix with host
# builds. RETSURF_ARM64_LTO=thin trades a slower binary for a link that fits in
# less RAM; the default matches CI.
set -euo pipefail

target=aarch64-unknown-linux-gnu
libdir=aarch64-linux-gnu

image=retsurf-arm64-cross
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
cache=${RETSURF_ARM64_CACHE:-$HOME/.cache/retsurf-arm64}
floor=$(cat "$here/glibc-floor")

cpus=("$@")
[ ${#cpus[@]} -eq 0 ] && cpus=(a35 a53 a55)

mkdir -p "$cache/target" "$cache/cargo"
# --network host for the same reason as the run below: the Debian indexes and
# the Rust toolchain are fetched from the network, and the bridge is not always
# a route out.
docker build -q -t "$image" --network host -f "$here/Dockerfile" "$here" >/dev/null

# A local Servo checkout, mounted at the path a `[patch]` in Cargo.toml names, so
# a fix can be built for a device before it reaches the fork. Unset normally.
servo_mount=()
if [ -n "${RETSURF_SERVO_SRC:-}" ]; then
  src=$(cd "$RETSURF_SERVO_SRC" && pwd)
  servo_mount=(-v "$src":"$src")
fi

# --network host: the Servo fork and inputbind are fetched from git.
docker run --rm -i --network host \
  -v "$repo":/repo \
  "${servo_mount[@]}" \
  -v "$cache/target":/target \
  -v "$cache/cargo":/cargo \
  -e CARGO_TARGET_DIR=/target -e CARGO_HOME=/cargo -e RUSTUP_HOME=/cargo \
  -e "TARGET=$target" -e "LIBDIR=$libdir" -e "FLOOR=$floor" \
  -e "LTO=${RETSURF_ARM64_LTO:-fat}" \
  -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
  -e "CPUS=${cpus[*]}" \
  "$image" bash -euxs <<'CONTAINER'
    export PATH="/cargo/bin:$PATH"
    command -v cargo >/dev/null || curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --default-toolchain none
    # build.rs stamps the About screen from git; without this the mounted repo
    # belongs to another user and git refuses it, leaving "unknown".
    git config --global --add safe.directory /repo
    cd /repo
    rustup target add "$TARGET"

    # GCC 10 and not focal's 9.3: SpiderMonkey wants >= 10.1, and 12's libstdc++
    # names __libc_single_threaded, a glibc 2.32 symbol above the floor.
    export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc-10
    export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++-10
    export AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc-10

    # bindgen runs the host libclang against target headers, so it needs the
    # triple and the multiarch include dir by hand -- it inherits nothing from
    # the cross gcc. The C++ headers are the cross compiler's: focal's own are
    # libstdc++ 9 and SpiderMonkey requires 10 or newer.
    export BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_linux_gnu="\
      --target=$TARGET -I/usr/include/$LIBDIR \
      -isystem /usr/$LIBDIR/include/c++/10 \
      -isystem /usr/$LIBDIR/include/c++/10/$LIBDIR \
      -isystem /usr/lib/gcc-cross/$LIBDIR/10/include"
    # Not the distro's clang: focal carries 10 and mozjs_sys 153 needs 19.
    export LIBCLANG_PATH=/usr/lib/llvm-19/lib
    export CLANG_PATH=/usr/bin/clang-19
    # SpiderMonkey's configure gives up if there is no llvm-objdump beside the
    # clang it found, and the compiler here is GCC.
    export LLVM_OBJDUMP=/usr/lib/llvm-19/bin/llvm-objdump

    # SpiderMonkey builds tools that run on the build machine, and its configure
    # falls back to the target compiler when these are unset. GCC 10 for the same
    # _GLIBCXX_RELEASE reason as the target side.
    export HOST_CC=gcc-10
    export HOST_CXX=g++-10

    # Unset, mozjs_sys downloads a prebuilt SpiderMonkey built on Ubuntu 22.04:
    # symbols above the floor, and a libstdc++ 11 GCC 10 cannot answer.
    export MOZJS_FROM_SOURCE=1

    # Multiarch means the sysroot is `/`: only the library path differs, so
    # PKG_CONFIG_SYSROOT_DIR must stay unset or every -I would be doubled.
    export PKG_CONFIG_ALLOW_CROSS=1
    export PKG_CONFIG_LIBDIR="/usr/lib/$LIBDIR/pkgconfig:/usr/share/pkgconfig"

    # CI performance build, kept out of Cargo.toml so a local `cargo build
    # --release` stays fast.
    export CARGO_PROFILE_RELEASE_LTO="$LTO"
    export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1

    mkdir -p /repo/dist/arm64
    for cpu in $CPUS; do
      case "$cpu" in
        # No -C target-cpu: runs on any ARMv8.0+. Default features, so webgl is
        # on and the surfman probe's catch_unwind needs unwind tables.
        universal) tune=""            ; feats=""                     ; panic=unwind ;;
        # ARMv8.0-A, in-order. RK3326; runs on A53 too (same ISA).
        a35)       tune=cortex-a35    ; feats="--no-default-features" ; panic=abort  ;;
        # ARMv8.0-A with crypto off (optional on A53). H700, Allwinner A133 Plus.
        a53)       tune=cortex-a53    ; feats="--no-default-features" ; panic=abort  ;;
        # ARMv8.2-A. SIGILLs on the v8.0 cores above, hence a separate binary.
        a55)       tune=cortex-a55    ; feats="--no-default-features" ; panic=abort  ;;
        *) echo "unknown cpu: $cpu" >&2; exit 2 ;;
      esac

      # webgl off drops the surfman probe, our only catch_unwind, and unwind
      # tables with it. The universal binary keeps both.
      export CARGO_PROFILE_RELEASE_PANIC="$panic"

      # The binary is ~100 MB of demand-paged text off the same card the device
      # swaps to: an OOM-killed session showed 212 MB of file-backed resident.
      # The armhf build took the same three levers and went 95 -> 76 MB with
      # major faults 191k -> 931 and no regression in the frame.
      #   --gc-sections drops what the C/C++ halves never reference;
      #   relocation-model=static skips a PIE's relocation work at every launch.
      # Static libstdc++: the devices at the floor carry a GCC-9 C++ runtime.
      # /opt/cxx-static covers cc-rs's own -lstdc++, which the flag does not.
      export RUSTFLAGS="${tune:+-C target-cpu=$tune} \
        -C link-arg=-L/opt/cxx-static -C link-arg=-static-libstdc++ \
        -C link-arg=-Wl,--gc-sections \
        -C relocation-model=static -C link-arg=-no-pie"
      # One section per function/datum, so the link above can drop the unreached.
      export CFLAGS_aarch64_unknown_linux_gnu="-ffunction-sections -fdata-sections"
      export CXXFLAGS_aarch64_unknown_linux_gnu="$CFLAGS_aarch64_unknown_linux_gnu"
      # Size over speed for the bulk of the Rust: the engine's code is cold and
      # there is a lot of it. The rasterizers keep -O3, pinned in Cargo.toml.
      # RETSURF_ARM64_OPT=3 puts the old level back, which is how the two compare.
      export CARGO_PROFILE_RELEASE_OPT_LEVEL="${RETSURF_ARM64_OPT:-s}"

      cargo build --release $feats --target "$TARGET"
      out="/target/$TARGET/release/retsurf"
      aarch64-linux-gnu-strip -o "/repo/dist/arm64/retsurf.$cpu" "$out"

      file "/repo/dist/arm64/retsurf.$cpu"
      aarch64-linux-gnu-readelf -d "/repo/dist/arm64/retsurf.$cpu" | grep NEEDED
      # Above the floor the loader on the device refuses it, so fail here instead.
      newer=$(aarch64-linux-gnu-readelf -V "/repo/dist/arm64/retsurf.$cpu" \
        | grep -o "GLIBC_2\.[0-9]*" | sort -uV \
        | awk -F. -v f="${FLOOR#2.}" "\$2 > f" | tr "\n" " ")
      [ -z "$newer" ] || { echo "retsurf.$cpu requires $newer; the floor is GLIBC_$FLOOR" >&2; exit 1; }
      # The C++ runtime carries no GLIBC_ version for the check above to catch.
      if aarch64-linux-gnu-readelf -d "/repo/dist/arm64/retsurf.$cpu" | grep -q "NEEDED.*libstdc++"; then
        echo "retsurf.$cpu links libstdc++ dynamically; it has to be static here" >&2
        exit 1
      fi

      # A per-core build differs from the last only in codegen flags, so cargo
      # would otherwise reuse the previous binary wholesale.
      cargo clean --release --target "$TARGET" -p retsurf
    done

    # The caches too: SpiderMonkey's build state is written as the container's
    # root, and the host cannot even delete it otherwise.
    chown -R "$HOST_UID:$HOST_GID" /repo/dist/arm64 /target /cargo
CONTAINER

ls -la "$repo/dist/arm64"
