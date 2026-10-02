package com.retsurf;

import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.Intent;
import android.net.ConnectivityManager;
import android.net.ProxyInfo;
import android.net.Uri;
import android.os.Bundle;
import android.os.Environment;
import android.system.ErrnoException;
import android.system.Os;
import android.util.Log;

import java.io.File;
import java.util.ArrayList;

import org.libsdl.app.SDLActivity;

/**
 * SDL entry activity for retsurf. SDL loads the libraries named here (in order)
 * and then calls the {@code SDL_main} we export from the Rust cdylib.
 */
public class RetsurfActivity extends SDLActivity {

    /** Must match `COMMAND_SHARE_LOGS` in src/platform/android.rs. */
    private static final int COMMAND_SHARE_LOGS = COMMAND_USER + 1;

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
        setEnv("RETSURF_LOG_FILE", rotateLog().getAbsolutePath());

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

    /**
     * Moves the last session's log aside, so a crash's log survives the relaunch.
     * Returns the path this session logs to.
     */
    private File rotateLog() {
        File log = new File(getFilesDir(), "retsurf.log");
        File prev = new File(getFilesDir(), "retsurf.prev.log");
        if (log.exists() && !log.renameTo(prev)) {
            Log.w("retsurf", "could not keep the previous log");
        }
        return log;
    }

    @Override
    protected boolean onUnhandledMessage(int command, Object param) {
        if (command == COMMAND_SHARE_LOGS) {
            shareLogs();
            return true;
        }
        return super.onUnhandledMessage(command, param);
    }

    private void shareLogs() {
        ArrayList<Uri> uris = new ArrayList<>();
        for (String name : LogProvider.FILES) {
            if (new File(getFilesDir(), name).exists()) {
                uris.add(LogProvider.uriFor(this, name));
            }
        }
        if (uris.isEmpty()) {
            return;
        }
        Intent send = new Intent(Intent.ACTION_SEND_MULTIPLE);
        send.setType("text/plain");
        send.putExtra(Intent.EXTRA_SUBJECT, "retsurf logs");
        send.putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris);
        // The read grant travels with the ClipData, not with EXTRA_STREAM.
        ClipData clip = ClipData.newRawUri("retsurf logs", uris.get(0));
        for (int i = 1; i < uris.size(); i++) {
            clip.addItem(new ClipData.Item(uris.get(i)));
        }
        send.setClipData(clip);
        send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        try {
            startActivity(Intent.createChooser(send, "Share logs"));
        } catch (ActivityNotFoundException e) {
            Log.w("retsurf", "no app to share the logs with");
        }
    }

    private void setEnv(String key, String value) {
        try {
            Os.setenv(key, value, true);
        } catch (ErrnoException e) {
            Log.w("retsurf", "failed to set env " + key + ": " + e.getMessage());
        }
    }
}
