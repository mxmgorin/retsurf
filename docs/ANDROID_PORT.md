# Android port

retsurf runs on Android by reusing the SDL2 stack the desktop/handheld builds
already use: SDL2 has a mature Android port where the app ships as a Rust
**cdylib** (`libretsurf.so`) that SDL's Java `SDLActivity` loads and enters via
the C `SDL_main` symbol we export. Windowing, the GLES context, gamepad input,
and the FBO-compositing render path all carry over; the Android-specific work is
build/packaging, storage paths, app lifecycle (GL surface loss), and touch input.

Everything Android is gated behind `#[cfg(target_os = "android")]` or additive
Cargo entries, so the Linux/macOS/Windows/handheld builds are unchanged.

## Status

| Area | State |
| --- | --- |
| cdylib + `SDL_main` entry point (`src/lib.rs`) | done |
| Storage paths (internal data + external Download via env) | done |
| Gradle/SDL APK shell (`android/`) | done |
| CI (`.github/workflows/build-android.yml`) | done |
| WebGL (feature ON; surfman `hardware_buffer` backend) | enabled, needs on-device verify |
| HiDPI scaling (egui zoom + Servo hidpi from `RETSURF_SCALE`) | done — confirmed bigger on device |
| Touch drag→scroll + tap→click (`src/event/touch.rs`) | done — **needs on-device verify** |
| System soft keyboard for the address bar (`SDL_StartTextInput`) | done — needs on-device verify |
| Empty start page on device | **investigating** — see below |
| System IME → in-page text fields (route `TextInput` to Servo) | **TODO** |
| App lifecycle / GL surface recreation on background/resume | **TODO** (needs a device) |

### Verified on device so far
Runs on a phone; UI/page scale correctly after the HiDPI fix. Outstanding:

- **Empty start page.** `retsurf:home` shows a blank dark screen. egui works (the
  toolbar scaled), so the suspect is `home_active` being false on device — i.e.
  the tab isn't reporting `retsurf:home` (custom-scheme load), so the egui
  start-page overlay never draws and you see the dark blank Servo page. A
  diagnostic `log::info!("home overlay active = {active}")` was added in
  `AppUi::set_home_active`. **Next step:** capture `adb logcat | grep -i "retsurf|home overlay"`
  at launch — `active=false`/absent ⇒ load/scheme problem; `active=true` but blank
  ⇒ overlay render/layering bug.
- **Touch scroll/tap and the address-bar keyboard** are implemented but not yet
  confirmed on device (does disabling SDL touch-mouse synthesis still leave egui
  toolbar taps working via egui's own `on_touch`?).

### Still TODO
- Route system-keyboard `TextInput` to focused **in-page** Servo fields (egui
  currently consumes it; only the address bar works).
- **Lifecycle / GL surface recreation** on background→foreground (Phase 5 of the
  plan): Android destroys the EGLSurface; on resume the FBO/color-texture/egui
  texture-registration GL names are stale and must be regenerated + re-registered
  or the page goes black. Will hit this the moment you switch apps and return.
- On-device WebGL verification (a shader demo) + background/resume cycle.

## Toolchain

- **NDK r27c** (`27.2.12479018`) — the version Servo's tree builds SpiderMonkey
  140 against.
- **API level 29** (Android 10) — reliable JIT executable mappings + GLES 3.x.
- Rust target `aarch64-linux-android` (rustup honors the pinned channel in
  `rust-toolchain.toml`).
- [`cargo-ndk`](https://github.com/bbqsrc/cargo-ndk) — cross-compiles the cdylib
  per ABI and drops `.so`s into `jniLibs/<abi>/`. (Not cargo-apk: it can't drive
  our custom `SDLActivity` Gradle project.)
- JDK 17 + Android SDK platform 34 / build-tools 34 for Gradle (AGP 8.5.2,
  Gradle 8.7).

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk --locked
sdkmanager --install "ndk;27.2.12479018" "platforms;android-34" "build-tools;34.0.0"
export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/27.2.12479018"
```

## Building locally

One-time setup:

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk --locked
# Android Studio (SDK + an NDK + CMake) or sdkmanager equivalents.
```

Then a single command does everything — builds `libSDL2.so` (first run only),
cross-compiles the Rust cdylib, and assembles the APK:

```sh
./android/scripts/build.sh           # debug APK  -> app/build/outputs/apk/debug/app-debug.apk
./android/scripts/build.sh release   # release APK (LTO, slower)
```

It auto-detects the SDK and the newest installed NDK (override with
`ANDROID_SDK_ROOT` / `ANDROID_NDK_HOME`). The first build compiles SpiderMonkey
from C++ source (~30–60 min); later builds are incremental.

Install to a connected device: `adb install -r android/app/build/outputs/apk/debug/app-debug.apk`.

### In Android Studio

`build.sh` is still needed once to produce `libretsurf.so` + `libSDL2.so` in
`app/src/main/jniLibs/` (Android Studio doesn't build Rust). After that, open the
`android/` folder in Android Studio and use Run ▶ to deploy/debug on a device or
emulator — Gradle just packages the prebuilt `.so`s. Re-run `build.sh` (or just
the `cargo ndk` step) whenever the Rust code changes.

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

## How the pieces fit

- **Entry point** — `src/lib.rs` exposes `run_app()` (shared with the desktop
  `src/main.rs`) and, on Android, `#[no_mangle] extern "C" fn SDL_main(...)`.
  `RetsurfActivity.getLibraries()` returns `{"SDL2", "retsurf"}`, so SDL loads
  `libretsurf.so` and calls our `SDL_main`.
- **Storage** — `RetsurfActivity.onCreate` sets, before SDL starts:
  - `RETSURF_DATA_DIR` = `getFilesDir()` (internal: config, cookies, cache) —
    `config.rs::data_dir()` already honors it.
  - `RETSURF_DOWNLOAD_DIR` = `getExternalFilesDir(DIRECTORY_DOWNLOADS)`
    (app-specific external dir, no permission) — read by the Android branch of
    `config.rs::system_download_dir()`.
  - `RETSURF_PANIC_FILE` = a file under `getFilesDir()`.

  These files are uninstall-scoped and not visible in the system Downloads app;
  MediaStore/SAF visibility is a future enhancement.
- **GLES** — `run_app()` forces `use_gles = true` on Android (Mali/Adreno/PowerVR
  expose only GLES); the existing `window.rs` GLES 3.0 path is correct. The
  desktop-only `SDL_VIDEODRIVER=wayland` alignment is skipped on Android.
- **Logging** — `android_logger` routes `log` to logcat (`adb logcat -s retsurf`).

## SDL version coupling

`sdl2-sys 0.38` vendors **SDL 2.26.4**. `android/scripts/sync-sdl.sh` copies the
`org.libsdl.app` Java glue and builds `libSDL2.so` from *that same* source, so the
Java glue, the runtime `.so`, and the Rust bindings all match. The synced files
(Java glue, wrapper jar, `jniLibs/`, mipmaps) are git-ignored and regenerated.

## Risks & fallbacks

1. **SpiderMonkey cross-compile** is the longest pole. If it fails to build/link:
   fallback A — drop the `js_jit` feature (interpreter-only SpiderMonkey, the
   most NDK/arch-sensitive piece removed); fallback B — a JS-disabled build to
   prove the pipeline, then iterate.
2. **`js_jit` blocked at runtime** by SELinux/W^X on some ROMs → crash on first
   JIT. Fallback: ship interpreter-only as the default Android artifact.
3. **GL surface loss on background** (the deferred lifecycle work): on resume the
   FBO/color-texture/egui-registration GL names are stale and must be regenerated
   and re-registered, or the page goes black. See plan Phase 5.
4. **WebGL surfman context survival** across background — its context lives in
   Servo's WebGL thread with no embedder handle; rely on lazy recreation, worst
   case recreate the WebView. The `create_surfman_connection()` `catch_unwind`
   probe keeps WebGL failures non-fatal (page still renders).
5. **Signing** — CI signs `assembleRelease` with a generated debug keystore for
   sideloading. Play distribution needs a real upload key (set `RETSURF_KEYSTORE`
   and friends, consumed by `app/build.gradle`).
