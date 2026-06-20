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
| HiDPI scaling (egui zoom + Servo hidpi from `RETSURF_SCALE`) | done — verified (see screen_rect fix below) |
| Touch drag→scroll + tap→click (`src/event/touch.rs`) | done — verified on device |
| System soft keyboard for the address bar (`SDL_StartTextInput`) | done — verified; home search no longer auto-pops the IME |
| Start page renders on device | done — see below (was a debug-build artifact + HiDPI layout bug) |
| Orientation / rotation relayout | done — layout follows rotation (see below) |
| Typing into in-page text fields | done for keycode input (Latin/digits/Enter/Backspace) — see below |
| Full IME → in-page fields (route SDL `TextInput` to Servo) | **TODO** — composition / non-ASCII / autocorrect |
| App lifecycle / GL surface recreation on background/resume | **TODO** (needs a device) |

### Verified on device
Runs on a phone; start page, touch, HiDPI scaling and rotation all work in a
**release** build. What was fixed (2026-06-14):

- **Empty/blank start page** had two unrelated causes:
  1. **Debug builds never initiate the initial load.** In a `--debug` cdylib the
     `retsurf:home` navigation never reaches the fetch stage (no `load_web_resource`,
     no `notify_url_changed`), so you get a white page. A **release** build loads
     it correctly. Use release on device; debug is only good for the build pipeline.
  2. **HiDPI layout bug (the real one).** egui-sdl2 computed its layout `screen_rect`
     once at construction with the default zoom (1.0), *before* `set_zoom_factor`
     (Android `RETSURF_SCALE` ≈ 2.1) was applied, and only refreshed it on a resize
     event. So egui laid the whole UI out for a screen ~2× too wide: the toolbar's
     right controls fell off-screen and the centered start-page overlay anchored
     off the visible area. Fixed upstream in **egui-sdl2 0.3.2** (`take_egui_input`
     now rebuilds `screen_rect` from the current zoom every frame).
- **Touch.** With `SDL_TOUCH_MOUSE_EVENTS=0` (set in `run_app` to kill phantom
  end-of-scroll clicks), egui got no pointer events, so the toolbar/overlays went
  dead. egui-sdl2 0.3.2 `on_touch` now synthesizes a primary-finger pointer stream
  (and scales SDL's normalized finger coords to pixels — they previously mapped to
  ~(0,0)). `handler.rs` only starts a web-view scroll/tap gesture for touches over
  the web view (`AppUi::point_over_webview`); toolbar touches are egui's.
- **Rotation.** Two issues: egui's cached size wasn't refreshed without a resize
  event (`AppUi::sync_window_size`, called per-frame on Android, fixes it), and the
  start-page `egui::Area` cached its size by id — `set_min_size` only grows it, so a
  landscape→portrait rotation left it stuck wide. `ui/home.rs` now also caps the
  size (`set_max_size`) so it shrinks back.
- **Home keyboard.** The start-page search field no longer auto-focuses on Android
  (`#[cfg(not(android))]` around its `request_focus`), so the soft keyboard only
  appears when the user taps the field, not on every home visit.

### Still TODO
- **Full IME for in-page fields.** Typing into a web field already works for
  ordinary key input: when a page field is focused and the last touch was over the
  web view (not the toolbar), `is_pointer_over_toolbar()` is false so egui doesn't
  consume the key, and `on_key` forwards a Servo `KeyboardEvent` built from the SDL
  *keycode* (`into_keyboard_event`). That covers Latin letters, digits, Enter,
  Backspace — an English Google search types fine. What's missing: `handler.rs` has
  no `TextInput`/`TextEditing` arm, so SDL's IME text events (composition for
  CJK/etc., accented/non-ASCII characters, and swipe/autocorrect text committed
  without a per-key `KEYDOWN`) reach only egui, never Servo. The fix is to forward
  those to Servo as IME/composition input when a page field (not an egui field) is
  focused.
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
Debug and release sign with the same `app/debug.keystore`, so `-r` updates in place —
no uninstall needed. (The first install after adopting this change may still need one
uninstall if the device holds an APK signed with an older/different key.)

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
5. **Signing** — both build types sign with one keystore (`app/debug.keystore`
   locally) so `adb install -r` updates in place instead of forcing a reinstall.
   CI restores a **stable** key from the `RETSURF_KEYSTORE_BASE64` secret (decoded
   to `app/release.keystore`, passed via `RETSURF_KEYSTORE`); without the secret it
   falls back to an ephemeral key (with a warning) so forks still build. One-time
   setup of the secret:
   ```sh
   keytool -genkeypair -keystore release.keystore -storepass android -keypass android \
     -alias androiddebugkey -keyalg RSA -keysize 2048 -validity 10000 \
     -dname "CN=retsurf,O=retsurf,C=US"
   base64 -w0 release.keystore   # save output as repo secret RETSURF_KEYSTORE_BASE64
   ```
   (If you use a non-default password/alias, also set the `RETSURF_KEYSTORE_PASS` /
   `RETSURF_KEY_ALIAS` / `RETSURF_KEY_PASS` secrets.) Play distribution needs a real
   upload key via the same env, consumed by `app/build.gradle`.
