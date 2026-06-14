#!/usr/bin/env bash
# One-command Android build: cross-compiles the Rust cdylib and assembles the APK.
#
#   ./android/scripts/build.sh            # build debug APK (faster)
#   ./android/scripts/build.sh release    # build release APK (LTO, slower)
#
# Prereqs (one-time):
#   rustup target add aarch64-linux-android
#   cargo install cargo-ndk --locked
#   Android SDK + an NDK + CMake (Android Studio installs these).
#
# Auto-detects the SDK and the newest installed NDK; override with ANDROID_SDK_ROOT
# / ANDROID_NDK_HOME. First run compiles SpiderMonkey from source (~30-60 min);
# later runs are incremental.
set -euo pipefail

profile="${1:-debug}"
abi="arm64-v8a"
api="29"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"   # android/
repo="$(cd "$here/.." && pwd)"

# --- locate SDK + NDK ---------------------------------------------------------
sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
[ -d "$sdk" ] || { echo "Android SDK not found (set ANDROID_SDK_ROOT)"; exit 1; }
export ANDROID_SDK_ROOT="$sdk"

ndk="${ANDROID_NDK_HOME:-${ANDROID_NDK:-}}"
if [ -z "$ndk" ]; then
    ndk="$(ls -d "$sdk"/ndk/* 2>/dev/null | sort -V | tail -1 || true)"
fi
[ -n "$ndk" ] && [ -d "$ndk" ] || { echo "NDK not found (set ANDROID_NDK_HOME)"; exit 1; }
export ANDROID_NDK_HOME="$ndk"
ndk_ver="$(basename "$ndk")"
echo "SDK: $sdk"
echo "NDK: $ndk ($ndk_ver)"

# --- toolchain checks ---------------------------------------------------------
rustup target list --installed | grep -q aarch64-linux-android \
    || { echo "run: rustup target add aarch64-linux-android"; exit 1; }
command -v cargo-ndk >/dev/null \
    || { echo "run: cargo install cargo-ndk --locked"; exit 1; }

# --- SDL libs + glue (only if missing) ----------------------------------------
if [ ! -f "$here/app/src/main/jniLibs/$abi/libSDL2.so" ]; then
    echo "==> building libSDL2.so + syncing SDL glue"
    bash "$here/scripts/sync-sdl.sh"
fi

# --- mozjs cross-compile env (cargo-ndk does NOT set these) -------------------
tc="$ndk/toolchains/llvm/prebuilt/linux-x86_64"
[ "$(uname)" = "Darwin" ] && tc="$ndk/toolchains/llvm/prebuilt/darwin-x86_64"
export ANDROID_NDK="$ndk"
export ANDROID_NDK_VERSION="$ndk_ver"
export ANDROID_VERSION="$api"
export ANDROID_TOOLCHAIN_DIR="$tc"
export ANDROID_CLANG="$tc/bin/aarch64-linux-android${api}-clang"
# bindgen must use the NDK's libclang (host clang-15+ dropped builtins it needs).
# In NDK r27 it lives under musl/lib, not lib/ (which only has libclang_rt.*);
# locate it rather than hard-coding, since the path moves between NDK releases.
libclang="$(find "$tc" -maxdepth 3 -name libclang.so 2>/dev/null | grep -v clang_rt | head -1)"
[ -n "$libclang" ] || { echo "libclang.so not found under $tc"; exit 1; }
export LIBCLANG_PATH="$(dirname "$libclang")"
export BINDGEN_EXTRA_CLANG_ARGS="--sysroot=$tc/sysroot"

# --- link workaround ----------------------------------------------------------
# NDK r23+ dropped libgcc, but rustc's aarch64-linux-android target spec still
# emits `-lgcc`. Write a stub that redirects it to libunwind. The `-L` search
# paths for this stub and the bundled libSDL2.so (in jniLibs) live in
# .cargo/config.toml under [target.aarch64-linux-android] — kept there, not in
# RUSTFLAGS, so changing them doesn't force a full rebuild.
mkdir -p "$repo/target/ndk-libgcc-stub"
echo "INPUT(-lunwind)" > "$repo/target/ndk-libgcc-stub/libgcc.a"

# --- build cdylib -------------------------------------------------------------
echo "==> cross-compiling libretsurf.so ($profile)"
cd "$repo"
ndk_flags=(-t "$abi" -P "$api" -o android/app/src/main/jniLibs build)
[ "$profile" = "release" ] && ndk_flags+=(--release)
# WebGL stays ON (do NOT pass --no-default-features): surfman works on Android EGL.
cargo ndk "${ndk_flags[@]}"

# --- assemble APK -------------------------------------------------------------
echo "==> assembling APK ($profile)"
cd "$here"
if [ "$profile" = "release" ]; then
    [ -f app/debug.keystore ] || keytool -genkeypair -v -keystore app/debug.keystore \
        -storepass android -keypass android -alias androiddebugkey \
        -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=retsurf,O=retsurf,C=US"
    ./gradlew --no-daemon assembleRelease
    echo "APK: android/app/build/outputs/apk/release/app-release.apk"
else
    ./gradlew --no-daemon assembleDebug
    echo "APK: android/app/build/outputs/apk/debug/app-debug.apk"
fi
