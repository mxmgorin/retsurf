# Android port

retsurf runs on Android by reusing the SDL2 stack the desktop and handheld builds
already use. SDL2 has a mature Android port where the app ships as a Rust cdylib
(`libretsurf.so`) that SDL's Java `SDLActivity` loads and enters through the C
`SDL_main` symbol we export. Windowing, the GLES context, gamepad input, and the
FBO-compositing render path all carry over. The Android-specific work is build and
packaging, storage paths, app lifecycle (GL surface loss), and touch input.

Everything Android is gated behind `#[cfg(target_os = "android")]` or additive Cargo
entries, so the Linux, macOS, Windows, and handheld builds are unchanged.

## Status

Works on device: the start page and browsing, touch scrolling and tapping, HiDPI scaling,
rotation, the system keyboard for the address bar and for keycode typing into page fields,
WebGL (via surfman's `hardware_buffer` backend), hidden system bars, and returning from the
background.

Open: full IME for page fields. SDL's `TextInput`/`TextEditing` events reach egui but not
Servo, so composition (CJK), non-ASCII characters, and swipe or autocorrect text that
arrives without a per-key `KEYDOWN` never reach a web page. Latin letters, digits, Enter and
Backspace do, as keycodes.

## Building

See [Building from source](BUILDING.md#android).

## How the pieces fit

- Entry point: `src/lib.rs` exposes `run_app()` (shared with the desktop `src/main.rs`);
  the `android/lib` crate wraps it in `#[no_mangle] extern "C" fn SDL_main(...)` and is
  the only cdylib in the workspace, since the armhf build compiles non-PIC and cannot
  link one (`tools/armhf/build.sh`).
  `RetsurfActivity.getLibraries()` returns `{"SDL2", "retsurf"}`, so SDL loads
  `libretsurf.so` and calls our `SDL_main`.
- Storage: `RetsurfActivity.onCreate` sets the following before SDL starts.
  - `RETSURF_DATA_DIR` = `getFilesDir()` (internal: config, cookies, cache), which
    `config.rs::data_dir()` already honors.
  - `RETSURF_DOWNLOAD_DIR` = `getExternalFilesDir(DIRECTORY_DOWNLOADS)` (an app-specific
    external dir, no permission needed), read by the Android branch of
    `config.rs::system_download_dir()`.
  - `RETSURF_PANIC_FILE` = a file under `getFilesDir()`.

  These files are uninstall-scoped and not visible in the system Downloads app;
  MediaStore/SAF visibility is a future enhancement.
- GLES: `run_app()` forces `use_gles = true` on Android (Mali, Adreno, and PowerVR expose
  only GLES), and the existing `window.rs` GLES 3.0 path is correct. The desktop-only
  `SDL_VIDEODRIVER=wayland` alignment is skipped on Android.
- Input: `SDL_TOUCH_MOUSE_EVENTS=0` (set in `src/platform/startup.rs`) stops SDL from
  turning the end of a scroll into a click. A touch starts a page scroll or tap only over the
  web view (`AppUi::point_over_webview`); touches on the chrome belong to egui. The system
  keyboard rises for a focused field unless `[input] system_keyboard` is off, and the start
  page's search field does not take focus on its own, so it does not pop on every visit.
- Rotation: SDL sends no resize event when the device rotates, so `AppUi::sync_window_size`
  refreshes egui's window size every frame.
- Background and resume: SDL keeps the GL context and replaces its EGL surfaces. The WebGL
  composite path (`src/platform/render/webgl.rs`) wraps SDL's draw and read surfaces, so it
  rewraps them when `eglGetCurrentSurface` reports a new one.
- Logging: `android_logger` routes `log` to logcat (`adb logcat -s retsurf`).

## SDL version coupling

`sdl2-sys 0.38` vendors SDL 2.26.4. `android/scripts/sync-sdl.sh` copies the
`org.libsdl.app` Java glue and builds `libSDL2.so` from that same source, so the Java glue,
the runtime `.so`, and the Rust bindings all match. The synced files (Java glue, wrapper
jar, `jniLibs/`, mipmaps) are git-ignored and regenerated.

## Signing

Both build types sign with one keystore (`app/debug.keystore` locally) so
`adb install -r` updates in place instead of forcing a reinstall. CI restores a stable
key from the `RETSURF_KEYSTORE_BASE64` secret (decoded to `app/release.keystore`, passed
via `RETSURF_KEYSTORE`); without the secret it falls back to an ephemeral key (with a
warning) so forks still build. One-time setup of the secret:

```sh
keytool -genkeypair -keystore release.keystore -storepass android -keypass android \
  -alias androiddebugkey -keyalg RSA -keysize 2048 -validity 10000 \
  -dname "CN=retsurf,O=retsurf,C=US"
base64 -w0 release.keystore   # save output as repo secret RETSURF_KEYSTORE_BASE64
```
(If you use a non-default password or alias, also set the `RETSURF_KEYSTORE_PASS`,
`RETSURF_KEY_ALIAS`, and `RETSURF_KEY_PASS` secrets.) Play distribution needs a real
upload key via the same env, consumed by `app/build.gradle`.
