package com.retsurf;

import android.net.ConnectivityManager;
import android.net.ProxyInfo;
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

        // Phones are high-DPI; without this the UI + page render at 1:1 pixels and
        // look tiny. density is ~1.0 (mdpi) .. ~3.5 (xxxhdpi). Applied to egui's
        // zoom factor and Servo's hidpi scale (see config::device_scale).
        setEnv("RETSURF_SCALE", String.valueOf(getResources().getDisplayMetrics().density));

        // No stderr on Android — keep a panic log alongside our data.
        setEnv("RETSURF_PANIC_FILE", new File(getFilesDir(), "retsurf-panic.log").getAbsolutePath());

        exportSystemProxy();

        super.onCreate(savedInstanceState);
    }

    /**
     * Passes the system proxy to Servo through the http_proxy/https_proxy/no_proxy
     * variables its default preferences read. A PAC proxy arrives as the local
     * port Android's PAC service listens on, so it needs no handling here.
     */
    private void exportSystemProxy() {
        ProxyInfo proxy;
        try {
            ConnectivityManager cm = getSystemService(ConnectivityManager.class);
            proxy = cm == null ? null : cm.getDefaultProxy();
        } catch (RuntimeException e) {
            Log.w("retsurf", "could not read the system proxy: " + e.getMessage());
            return;
        }
        // Port stays -1 while the PAC service has not started its local proxy.
        if (proxy == null || proxy.getHost() == null || proxy.getHost().isEmpty()
                || proxy.getPort() <= 0) {
            return;
        }

        String host = proxy.getHost();
        if (host.indexOf(':') >= 0) {
            host = "[" + host + "]";
        }
        // HTTPS goes through the same proxy as a CONNECT tunnel.
        String uri = "http://" + host + ":" + proxy.getPort();
        setEnv("http_proxy", uri);
        setEnv("https_proxy", uri);

        StringBuilder bypass = new StringBuilder();
        for (String entry : proxy.getExclusionList()) {
            // hyper-util matches subdomains from a leading dot; `*` alone is its only wildcard.
            String rule = entry.trim();
            if (rule.startsWith("*.")) {
                rule = rule.substring(1);
            }
            if (rule.isEmpty()) {
                continue;
            }
            if (bypass.length() > 0) {
                bypass.append(',');
            }
            bypass.append(rule);
        }
        if (bypass.length() > 0) {
            setEnv("no_proxy", bypass.toString());
        }
        Log.i("retsurf", "using system proxy " + uri);
    }

    private void setEnv(String key, String value) {
        try {
            Os.setenv(key, value, true);
        } catch (ErrnoException e) {
            Log.w("retsurf", "failed to set env " + key + ": " + e.getMessage());
        }
    }
}
