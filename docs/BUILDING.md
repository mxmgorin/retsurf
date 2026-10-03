# Building from source

retsurf builds with plain `cargo`. The Rust version is pinned in
[`rust-toolchain.toml`](../rust-toolchain.toml), so rustup installs the right
toolchain on the first build. The heavy part of the graph is Servo, which needs
its own C/C++ build dependencies — the first build is long on any platform.

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

Same two features: SDL2 is built from source and linked statically, so the build
needs no system SDL2 and the binary ships without a DLL.

```sh
cargo build --release --features sdl2-bundled,sdl2-static-link
```

If CMake 4.x refuses the bundled SDL2 (`cmake_minimum_required` below 3.5), set
`CMAKE_POLICY_VERSION_MINIMUM=3.5` for the build.

## Cargo features

| Feature | Default | What it does |
| --- | --- | --- |
| `webgl` | on | WebGL through surfman over SDL's EGL display. Off only where there is no EGL to compose over, which in practice means a `software` build. WebGPU is a Servo feature of its own and is not enabled. |
| `software` | off | CPU rendering end to end for GPU-less devices — swgl rasterizes the page, SDL's renderer paints the chrome. |
| `sdl2-bundled` | off | Build SDL2 from source instead of using the system one. |
| `sdl2-static-link` | off | Link SDL2 statically. |

A software build turns the default off, since swgl replaces the GL path entirely:

```sh
cargo build --release --no-default-features --features software
```

Release builds here stay quick to compile: fat LTO and `codegen-units = 1` are
set by the workflows, not by `Cargo.toml`.

## Android

The toolchain:

- NDK r27c (`27.2.12479018`), pinned in `.github/workflows/build-android.yml`.
- API level 29 (Android 10), for reliable JIT executable mappings and GLES 3.x.
- Rust target `aarch64-linux-android` (rustup honors the pinned channel in
  `rust-toolchain.toml`).
- [`cargo-ndk`](https://github.com/bbqsrc/cargo-ndk), which cross-compiles the cdylib per
  ABI and drops the `.so` files into `jniLibs/<abi>/`. Not cargo-apk, which can't drive our
  custom `SDLActivity` Gradle project.
- JDK 17 plus Android SDK platform 34 and build-tools 34 for Gradle (AGP 8.5.2, Gradle 8.7).

The SDK pieces come from Android Studio or `sdkmanager`:

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk --locked
sdkmanager --install "ndk;27.2.12479018" "platforms;android-34" "build-tools;34.0.0"
export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/27.2.12479018"
```

`android/scripts/build.sh` then does everything: it builds `libSDL2.so` (first run only),
cross-compiles the Rust cdylib, and assembles the APK.

```sh
./android/scripts/build.sh           # debug APK  -> app/build/outputs/apk/debug/app-debug.apk
./android/scripts/build.sh release   # release APK (LTO, slower)
```

It auto-detects the SDK and the newest installed NDK (override with `ANDROID_SDK_ROOT` or
`ANDROID_NDK_HOME`). The first build compiles SpiderMonkey from C++ source (roughly 30 to
60 minutes); later builds are incremental.

Install to a connected device with
`adb install -r android/app/build/outputs/apk/debug/app-debug.apk`. Debug and release sign
with the same `app/debug.keystore`, so `-r` updates in place and no uninstall is needed.
Test on a device with a release build: a debug build has been seen to never start the
first page load, which leaves a white page.

### In Android Studio

`build.sh` is still needed once to produce `libretsurf.so` and `libSDL2.so` in
`app/src/main/jniLibs/`, since Android Studio doesn't build Rust. After that, open the
`android/` folder in Android Studio and use Run to deploy or debug on a device or emulator;
Gradle just packages the prebuilt `.so` files. Re-run `build.sh` (or just the `cargo ndk`
step) whenever the Rust code changes.

### Manual (what build.sh automates)

```sh
cargo fetch && bash android/scripts/sync-sdl.sh    # SDL glue + libSDL2.so
tc="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64"   # or darwin-x86_64
export ANDROID_NDK="$ANDROID_NDK_HOME" ANDROID_NDK_VERSION="$(basename "$ANDROID_NDK_HOME")"
export ANDROID_VERSION=29 ANDROID_TOOLCHAIN_DIR="$tc"
export ANDROID_CLANG="$tc/bin/aarch64-linux-android29-clang"
# bindgen (mozjs_sys/mozangle/sdl2-sys) must use the NDK libclang — host clang-15+
# dropped builtins these need (same trap as the desktop LIBCLANG llvm-14 pin).
# NDK r27 keeps libclang.so under musl/lib (not lib/, which has only libclang_rt.*).
export LIBCLANG_PATH="$tc/musl/lib" BINDGEN_EXTRA_CLANG_ARGS="--sysroot=$tc/sysroot"
# NDK r23+ dropped libgcc but rustc still emits -lgcc; stub it to libunwind. The
# -L paths (stub + jniLibs for libSDL2.so) are in .cargo/config.toml under
# [target.aarch64-linux-android], so no RUSTFLAGS needed.
mkdir -p target/ndk-libgcc-stub && echo 'INPUT(-lunwind)' > target/ndk-libgcc-stub/libgcc.a
# WebGL stays ON (do NOT pass --no-default-features).
cargo ndk -t arm64-v8a -P 29 -o android/app/src/main/jniLibs build --release
cd android && ./gradlew assembleRelease
```

[`docs/ANDROID_PORT.md`](ANDROID_PORT.md) covers how the port is put together.

## Tests

```sh
cargo test                           # unit tests + the engine-source guard
python3 tests/run_pages.py           # the pages, in a headless browser
```

The page runner needs a release binary, `Xvfb`, `xdotool` (the pages that need a
gesture get one) and `ffmpeg` (the server builds its test clip with it). It points
one browser per page at `tests/serve.py`, waits for that page's beacons and checks
them, so a regression fails rather than needing to be looked at. The `Check`
workflow runs both.

## Handhelds

CI builds these on its own runners; the scripts below exist for building locally,
in the same containers, against the same glibc floor.

```sh
tools/arm64/build.sh                 # aarch64 per-core binaries -> dist/arm64/
tools/arm64/package-portmaster.sh    # builds, then dist/portmaster{,.zip}
tools/armhf/build.sh                 # Miyoo Mini (armv7) binary
```

[`tools/arm64/README.md`](../tools/arm64/README.md) covers the per-core binaries,
the caches, and why the base image is what it is;
[`tools/armhf/README.md`](../tools/armhf/README.md) the Miyoo Mini toolchain.
[`docs/HANDHELD_PORT.md`](HANDHELD_PORT.md) lists the devices and packages, and
[`docs/RENDERING.md`](RENDERING.md) how the picture gets on screen.
