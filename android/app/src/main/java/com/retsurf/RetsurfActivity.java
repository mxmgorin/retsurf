package com.retsurf;

import android.os.Bundle;
import android.os.Environment;
import android.system.ErrnoException;
import android.system.Os;
import android.util.Log;

import java.io.File;

import org.libsdl.app.SDLActivity;

/**
 * SDL entry activity for retsurf. SDL loads the libraries named here (in order)
 * and then calls the {@code SDL_main} we export from the Rust cdylib.
 */
public class RetsurfActivity extends SDLActivity {

    @Override
    protected String[] getLibraries() {
        // Order matters: SDL2 first, then our cdylib (libretsurf.so), whose
        // SDL_main becomes the app entry point.
        return new String[] { "SDL2", "retsurf" };
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        // Hand the Rust side its writable locations via the same env vars the
        // desktop/handheld builds already honor (see src/config.rs). Must run
        // before super.onCreate(), which loads the native libs and starts SDL.
        setEnv("RETSURF_DATA_DIR", getFilesDir().getAbsolutePath());

        File dl = getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS);
        if (dl != null) {
            setEnv("RETSURF_DOWNLOAD_DIR", dl.getAbsolutePath());
        }

        // No stderr on Android — keep a panic log alongside our data.
        setEnv("RETSURF_PANIC_FILE", new File(getFilesDir(), "retsurf-panic.log").getAbsolutePath());

        super.onCreate(savedInstanceState);
    }

    private void setEnv(String key, String value) {
        try {
            Os.setenv(key, value, true);
        } catch (ErrnoException e) {
            Log.w("retsurf", "failed to set env " + key + ": " + e.getMessage());
        }
    }
}
