# Building from source

retsurf builds with plain `cargo`; rustup installs the Rust version pinned in
[`rust-toolchain.toml`](../rust-toolchain.toml). Servo brings C/C++ dependencies of its
own, so the first build is long.

## Linux

```sh
sudo apt-get install -y build-essential clang cmake curl git gperf pkg-config python3 \
  libssl-dev libdbus-1-dev libfreetype6-dev libglib2.0-dev \
  libgl1-mesa-dev libegl1-mesa-dev libgles2-mesa-dev \
  libharfbuzz-dev liblzma-dev libudev-dev libunwind-dev libsdl2-dev
```

```sh
cargo run
```

## macOS

```sh
brew install cmake pkg-config
cargo build --release --features sdl2-bundled,sdl2-static-link
```

## Windows

SDL2 is built from source and linked statically, so no SDL2 DLL is needed.

```sh
cargo build --release --features sdl2-bundled,sdl2-static-link
```

If CMake 4.x rejects the bundled SDL2, set `CMAKE_POLICY_VERSION_MINIMUM=3.5`.

## Cargo features

| Feature | Default | What it does |
| --- | --- | --- |
| `webgl` | on | WebGL over SDL's EGL display |
| `software` | off | CPU rendering for devices without a GPU |
| `sdl2-bundled` | off | Build SDL2 from source |
| `sdl2-static-link` | off | Link SDL2 statically |

A software build has no GL, so it turns the defaults off:

```sh
cargo build --release --no-default-features --features software
```

CI release builds add fat LTO and `codegen-units = 1`; `Cargo.toml` leaves them out to
keep local builds quick.

## Android

Needs NDK r27c (`27.2.12479018`, as pinned in `.github/workflows/build-android.yml`), JDK 17,
Android SDK platform and build-tools 34, and [`cargo-ndk`](https://github.com/bbqsrc/cargo-ndk).
The APK targets API level 29.

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk --locked
sdkmanager --install "ndk;27.2.12479018" "platforms;android-34" "build-tools;34.0.0"
export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/27.2.12479018"

./android/scripts/build.sh           # debug APK: android/app/build/outputs/apk/debug/
./android/scripts/build.sh release   # release APK
adb install -r android/app/build/outputs/apk/release/app-release.apk
```

`build.sh` builds `libSDL2.so` on its first run, cross-compiles the Rust library and
assembles the APK. The first build compiles SpiderMonkey (30 to 60 minutes). Debug and
release share one signing key, so `-r` updates in place. Test on a device with a release
build: a debug build has been seen to stop before the first page load.

Android Studio can run and debug the `android/` project once `build.sh` has put the `.so`
files in `app/src/main/jniLibs/`; it does not build the Rust part.

What `build.sh` does, by hand:

```sh
cargo fetch && bash android/scripts/sync-sdl.sh    # SDL glue + libSDL2.so
tc="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64"   # or darwin-x86_64
export ANDROID_NDK="$ANDROID_NDK_HOME" ANDROID_NDK_VERSION="$(basename "$ANDROID_NDK_HOME")"
export ANDROID_VERSION=29 ANDROID_TOOLCHAIN_DIR="$tc"
export ANDROID_CLANG="$tc/bin/aarch64-linux-android29-clang"
# bindgen needs the NDK's libclang, which r27 keeps under musl/lib.
export LIBCLANG_PATH="$tc/musl/lib" BINDGEN_EXTRA_CLANG_ARGS="--sysroot=$tc/sysroot"
# rustc still links -lgcc, which the NDK dropped; libunwind stands in.
mkdir -p target/ndk-libgcc-stub && echo 'INPUT(-lunwind)' > target/ndk-libgcc-stub/libgcc.a
cargo ndk -t arm64-v8a -P 29 -o android/app/src/main/jniLibs build --release
cd android && ./gradlew assembleRelease
```

[`ANDROID_PORT.md`](ANDROID_PORT.md) covers how the port is put together.

## Tests

```sh
cargo test                           # unit tests + the engine-source guard
python3 tests/run_pages.py           # the pages, in a headless browser
```

The page runner needs a release binary, `Xvfb`, `xdotool` and `ffmpeg`. It loads each page
from `tests/serve.py` and checks the results the page reports. The `Check` workflow runs
both.

## Handhelds

CI builds these; the scripts build locally in the same containers.

```sh
tools/arm64/build.sh                 # aarch64 per-core binaries -> dist/arm64/
tools/arm64/package-portmaster.sh    # builds, then dist/portmaster{,.zip}
tools/armhf/build.sh                 # Miyoo Mini (armv7) binary
```

Details: [`tools/arm64/README.md`](../tools/arm64/README.md),
[`tools/armhf/README.md`](../tools/armhf/README.md),
[Handhelds](HANDHELD_PORT.md), [Rendering](RENDERING.md).
