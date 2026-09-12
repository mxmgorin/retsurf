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
| `webgl` | on | WebGL/WebGPU through surfman over SDL's EGL display. Off only where there is no EGL to compose over, which in practice means a `software` build. |
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

With the Android SDK/NDK installed:

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk --locked
./android/scripts/build.sh release   # android/app/build/outputs/apk/release/app-release.apk
adb install -r android/app/build/outputs/apk/release/app-release.apk
```

See [`docs/ANDROID_PORT.md`](ANDROID_PORT.md) for how the port is put together.

## Handhelds

CI builds these on its own runners; the scripts below exist for building locally,
in the same containers, against the same glibc floor.

```sh
tools/arm64/build.sh                 # aarch64 per-core binaries -> dist/arm64/
tools/arm64/package-portmaster.sh    # builds, then dist/portmaster{,.zip}
tools/armhf/build.sh                 # Miyoo Mini (armv7) binary
```

[`tools/arm64/README.md`](../tools/arm64/README.md) covers the per-core binaries,
the caches, and why the base image is what it is.
[`docs/HANDHELD_PORT.md`](HANDHELD_PORT.md) covers the port itself.
