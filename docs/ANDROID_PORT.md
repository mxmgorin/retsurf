# Android port

On Android retsurf is a Rust library (`libretsurf.so`) that SDL2's Java `SDLActivity` loads
and enters through `SDL_main`. Windowing, the GLES context, gamepad input and rendering are
the same SDL2 code as everywhere else; Android code sits behind
`#[cfg(target_os = "android")]`.

Building: see [Building from source](BUILDING.md#android).

## Status

Works: browsing, touch scrolling and tapping, HiDPI, rotation, WebGL (surfman's
`hardware_buffer` backend), hidden system bars, returning from the background, and the
system keyboard for the address bar and for keycode typing in page fields.

Open: full IME in page fields. SDL's `TextInput`/`TextEditing` reach egui but not Servo, so
composition (CJK), non-ASCII text and swipe or autocorrect input never reach a page.

## How it fits together

- **Entry**: the `android/lib` crate wraps `run_app()` from `src/lib.rs` in `SDL_main`. It is
  the workspace's only cdylib, since the armhf build cannot link one.
  `RetsurfActivity.getLibraries()` names `SDL2` and `retsurf`.
- **Environment**: `RetsurfActivity` sets `RETSURF_DATA_DIR` (internal storage),
  `RETSURF_DOWNLOAD_DIR` (the app's external Downloads dir, no permission needed),
  `RETSURF_PANIC_FILE` and `RETSURF_SCALE` (display density). Downloads do not show in the
  system Downloads app.
- **GLES**: always on (`src/platform/startup.rs`), and the Wayland alignment is skipped.
- **Input**: `SDL_TOUCH_MOUSE_EVENTS=0`, so a scroll does not end in a click. Touches start a
  page gesture only over the web view; the chrome's belong to egui. The system keyboard
  rises for a focused field unless `[input] system_keyboard` is off.
- **Rotation**: SDL sends no resize event, so `AppUi::sync_window_size` runs every frame.
- **Resume**: SDL keeps the GL context but replaces its EGL surfaces; the WebGL composite
  path rewraps them when `eglGetCurrentSurface` changes.
- **Logging**: logcat, `adb logcat -s retsurf`.

## SDL version

`sdl2-sys 0.38` vendors SDL 2.26.4. `android/scripts/sync-sdl.sh` takes the Java glue and
builds `libSDL2.so` from that same source, applying `android/patches/`, so the bindings, the
library and the glue match. The synced files are git-ignored.

## Signing

Debug and release sign with one keystore (`app/debug.keystore` locally), so
`adb install -r` updates in place. CI decodes the `RETSURF_KEYSTORE_BASE64` secret to
`app/release.keystore`; without it, forks build with a throwaway key. To create the secret:

```sh
keytool -genkeypair -keystore release.keystore -storepass android -keypass android \
  -alias androiddebugkey -keyalg RSA -keysize 2048 -validity 10000 \
  -dname "CN=retsurf,O=retsurf,C=US"
base64 -w0 release.keystore   # the RETSURF_KEYSTORE_BASE64 secret
```

A different password or alias goes in `RETSURF_KEYSTORE_PASS`, `RETSURF_KEY_ALIAS` and
`RETSURF_KEY_PASS`.
