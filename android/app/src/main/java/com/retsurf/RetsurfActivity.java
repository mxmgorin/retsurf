package com.retsurf;

import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.ContentResolver;
import android.content.ContentUris;
import android.content.ContentValues;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.database.Cursor;
import android.net.ConnectivityManager;
import android.net.ProxyInfo;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.os.FileUtils;
import android.provider.MediaStore;
import android.system.ErrnoException;
import android.system.Os;
import android.util.Log;
import android.view.WindowInsets;

import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
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
        followKeyboard();
    }

    /**
     * Keeps the app above the on-screen keyboard by shrinking SDL's layout to the
     * space it leaves, which SDL reports as a resize; a fullscreen window is not
     * resized for it. Needs Android 11's keyboard insets.
     */
    private void followKeyboard() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R || mLayout == null) {
            return;
        }
        getWindow().getDecorView().setOnApplyWindowInsetsListener((view, insets) -> {
            int keyboard = insets.getInsets(WindowInsets.Type.ime()).bottom;
            if (mLayout != null && mLayout.getPaddingBottom() != keyboard) {
                mLayout.setPadding(0, 0, 0, keyboard);
            }
            return view.onApplyWindowInsets(insets);
        });
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

    /**
     * Requests `requested` with each sensor orientation swapped for its user
     * variant, which honors the system rotation lock.
     */
    @Override
    public void setRequestedOrientation(int requested) {
        super.setRequestedOrientation(honoringRotationLock(requested));
    }

    /** The variant of `requested` that honors the rotation lock; `requested` when it has none. */
    private static int honoringRotationLock(int requested) {
        switch (requested) {
            case ActivityInfo.SCREEN_ORIENTATION_FULL_SENSOR:
                return ActivityInfo.SCREEN_ORIENTATION_FULL_USER;
            case ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE:
                return ActivityInfo.SCREEN_ORIENTATION_USER_LANDSCAPE;
            case ActivityInfo.SCREEN_ORIENTATION_SENSOR_PORTRAIT:
                return ActivityInfo.SCREEN_ORIENTATION_USER_PORTRAIT;
            default:
                return requested;
        }
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

    /**
     * Moves a finished download into the shared Downloads, where the system's file
     * apps list it. Returns its path there, or null when it stays where it was.
     * Blocks for the copy; must be called off the UI thread.
     */
    public String publishDownload(String path) {
        File file = new File(path);
        ContentResolver resolver = getContentResolver();
        ContentValues values = new ContentValues();
        values.put(MediaStore.Downloads.DISPLAY_NAME, file.getName());
        // Hidden from other apps until the copy is complete.
        values.put(MediaStore.Downloads.IS_PENDING, 1);
        Uri uri;
        try {
            uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values);
        } catch (RuntimeException e) {
            Log.w("retsurf", "no Downloads entry for " + file.getName() + ": " + e.getMessage());
            return null;
        }
        if (uri == null) {
            Log.w("retsurf", "no Downloads entry for " + file.getName());
            return null;
        }
        try (InputStream in = new FileInputStream(file);
                OutputStream out = resolver.openOutputStream(uri)) {
            if (out == null) {
                throw new IOException("no stream to write to");
            }
            FileUtils.copy(in, out);
        } catch (IOException | RuntimeException e) {
            Log.w("retsurf", "could not move " + file.getName() + " to Downloads: " + e.getMessage());
            resolver.delete(uri, null, null);
            return null;
        }
        values.clear();
        values.put(MediaStore.Downloads.IS_PENDING, 0);
        resolver.update(uri, values, null, null);
        // Read once published: a name already taken gets a numbered one only then.
        String name = displayName(resolver, uri, file.getName());
        if (!file.delete()) {
            Log.w("retsurf", "could not remove the staged " + file.getName());
        }
        File downloads = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS);
        return new File(downloads, name).getAbsolutePath();
    }

    /** Must match `OPENED` in src/platform/android.rs. */
    private static final int OPENED = 1;
    /** Must match `NO_APP` in src/platform/android.rs. */
    private static final int NO_APP = 0;
    /** Must match `NOT_SHARED` in src/platform/android.rs. */
    private static final int NOT_SHARED = -1;

    /**
     * Opens a file in the shared Downloads with the app Android picks for its
     * type. Returns OPENED, NO_APP when none takes it, or NOT_SHARED when the
     * file is not one of this app's entries there.
     */
    public int openFile(String path) {
        Uri uri = downloadEntry(new File(path).getName());
        if (uri == null) {
            return NOT_SHARED;
        }
        Intent view = new Intent(Intent.ACTION_VIEW);
        view.setDataAndType(uri, getContentResolver().getType(uri));
        view.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            startActivity(view);
            return OPENED;
        } catch (ActivityNotFoundException e) {
            return NO_APP;
        }
    }

    /** This app's newest entry in the shared Downloads named `name`; null without one. */
    private Uri downloadEntry(String name) {
        String[] columns = { MediaStore.Downloads._ID };
        String[] args = { name };
        String newest = MediaStore.Downloads.DATE_ADDED + " DESC";
        try (Cursor cursor = getContentResolver().query(MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                columns, MediaStore.Downloads.DISPLAY_NAME + " = ?", args, newest)) {
            if (cursor != null && cursor.moveToFirst()) {
                return ContentUris.withAppendedId(MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                        cursor.getLong(0));
            }
        } catch (RuntimeException e) {
            Log.w("retsurf", "could not look up " + name + " in Downloads: " + e.getMessage());
        }
        return null;
    }

    /** The name MediaStore keeps `uri` under; `fallback` when it cannot be read. */
    private static String displayName(ContentResolver resolver, Uri uri, String fallback) {
        String[] columns = { MediaStore.Downloads.DISPLAY_NAME };
        try (Cursor cursor = resolver.query(uri, columns, null, null, null)) {
            if (cursor != null && cursor.moveToFirst() && cursor.getString(0) != null) {
                return cursor.getString(0);
            }
        } catch (RuntimeException e) {
            Log.w("retsurf", "could not read the Downloads name: " + e.getMessage());
        }
        return fallback;
    }

    private void setEnv(String key, String value) {
        try {
            Os.setenv(key, value, true);
        } catch (ErrnoException e) {
            Log.w("retsurf", "failed to set env " + key + ": " + e.getMessage());
        }
    }
}
