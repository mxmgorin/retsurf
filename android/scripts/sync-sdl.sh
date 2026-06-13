#!/usr/bin/env bash
# Pull the SDL pieces that must match the linked sdl2-sys version (2.26.4) out of
# the Cargo registry into this Gradle project, and build the matching libSDL2.so:
#
#   1. Java glue   -> app/src/main/java/org/libsdl/app/*.java
#   2. Gradle wrapper jar + gradlew (so ./gradlew works without a system Gradle)
#   3. libSDL2.so (built from SDL source for arm64-v8a via the NDK + CMake)
#   4. libc++_shared.so (from the NDK sysroot)
#
# Run `cargo fetch` first so the sdl2-sys source is present. Requires
# ANDROID_NDK_HOME (or ANDROID_NDK) and cmake on PATH.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"   # android/
repo="$(cd "$here/.." && pwd)"
abi="arm64-v8a"
api="29"

ndk="${ANDROID_NDK_HOME:-${ANDROID_NDK:-}}"
[ -n "$ndk" ] || { echo "set ANDROID_NDK_HOME (or ANDROID_NDK)"; exit 1; }

# SDL 2.26.4 declares cmake_minimum_required 3.0.0, which CMake 4.x rejects.
# Prefer an Android-SDK-bundled CMake (3.x) over a too-new system one. Override
# with $CMAKE.
cmake_bin="${CMAKE:-}"
if [ -z "$cmake_bin" ]; then
    sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
    bundled="$(ls -d "$sdk"/cmake/*/bin/cmake 2>/dev/null | sort -V | tail -1 || true)"
    if [ -n "$bundled" ]; then
        cmake_bin="$bundled"
    else
        cmake_bin="cmake"
    fi
fi
echo "using cmake: $cmake_bin ($("$cmake_bin" --version | head -1))"

cargo_home="${CARGO_HOME:-$HOME/.cargo}"
sdl_crate="$(find "$cargo_home/registry/src" -maxdepth 2 -type d -name 'sdl2-sys-*' | sort -V | tail -1)"
[ -n "$sdl_crate" ] || { echo "sdl2-sys not found; run 'cargo fetch' first"; exit 1; }
sdl_src="$sdl_crate/SDL"
sdl_proj="$sdl_src/android-project"
echo "using SDL from: $sdl_crate"

# 1. Java glue
glue_dst="$here/app/src/main/java/org/libsdl/app"
mkdir -p "$glue_dst"
cp "$sdl_proj"/app/src/main/java/org/libsdl/app/*.java "$glue_dst/"

# 1b. Placeholder launcher icons so @mipmap/ic_launcher resolves (replace with
#     retsurf branding later; not version-coupled to SDL, just convenient here).
for d in "$sdl_proj"/app/src/main/res/mipmap-*; do
    [ -d "$d" ] || continue
    dst="$here/app/src/main/res/$(basename "$d")"
    mkdir -p "$dst"
    cp "$d"/ic_launcher.png "$dst/" 2>/dev/null || true
done

# 2. Gradle wrapper (jar is forward-compatible; our gradle-wrapper.properties pins 8.7)
mkdir -p "$here/gradle/wrapper"
cp "$sdl_proj/gradle/wrapper/gradle-wrapper.jar" "$here/gradle/wrapper/"
cp "$sdl_proj/gradlew" "$sdl_proj/gradlew.bat" "$here/"
chmod +x "$here/gradlew"

# 3. libSDL2.so for arm64-v8a
jnilibs="$here/app/src/main/jniLibs/$abi"
mkdir -p "$jnilibs"
build="$repo/target/sdl-android-$abi"
"$cmake_bin" -S "$sdl_src" -B "$build" \
    -DCMAKE_TOOLCHAIN_FILE="$ndk/build/cmake/android.toolchain.cmake" \
    -DANDROID_ABI="$abi" -DANDROID_PLATFORM="android-$api" \
    -DSDL_STATIC=OFF -DSDL_SHARED=ON \
    -DSDL_SENSOR=OFF >/dev/null   # SDL 2.26.4's Android sensor uses ALooper_pollAll,
                                  # removed in NDK 27 headers; unused by a browser.
"$cmake_bin" --build "$build" --target SDL2 -j"$(nproc 2>/dev/null || echo 4)" >/dev/null
cp "$build"/libSDL2.so "$jnilibs/"

# 4. libc++_shared.so from the NDK sysroot
host="linux-x86_64"; [ "$(uname)" = "Darwin" ] && host="darwin-x86_64"
cp "$ndk/toolchains/llvm/prebuilt/$host/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so" "$jnilibs/"

echo "synced SDL glue + libs into $here"
