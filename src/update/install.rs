//! Download -> verify -> atomically swap the release in place. Runs on a worker
//! thread (see [`super::Updater::install`]); drives the shared [`UpdateState`]
//! through Downloading -> Installing -> Installed / Error, waking the loop after
//! each transition.
//!
//! Only executables are ever replaced — the three `retsurf.a{35,53,55}` + the
//! `Retsurf.sh` launcher for a PortMaster port, or the single running binary for a
//! desktop install — never `data/`, `downloads/`, or the config (which on the
//! handheld also live under the gamedir). Each swap renames a fully-written sibling
//! over the target, so a running/mmap'd binary keeps its old inode and a mid-swap
//! crash leaves the install fully-old or fully-new (the current target is moved to a
//! `.bak-update` we can roll back from).

use super::{publish, Kind, UpdateState, USER_AGENT};
use crate::event::user::UserEventSender;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The per-core binaries a PortMaster release ships, named under the extracted
/// `retsurf/` subfolder and directly in the gamedir.
const BINARIES: [&str; 3] = ["retsurf.a35", "retsurf.a53", "retsurf.a55"];

/// Throttle for download-progress wakeups (matches the download worker).
const NOTIFY_EVERY: Duration = Duration::from_millis(250);

/// Drive the whole install, publishing the terminal state. Any error leaves the
/// live install untouched (see the swap rollback) and surfaces as [`UpdateState::Error`].
pub(super) fn run(
    kind: &Kind,
    version: &str,
    url: &str,
    sha256: Option<&str>,
    auth: Option<&str>,
    state: &Mutex<UpdateState>,
    sender: &UserEventSender,
) {
    match install(kind, url, sha256, auth, state, sender) {
        Ok(()) => publish(
            state,
            UpdateState::Installed {
                version: version.to_string(),
            },
            sender,
        ),
        Err(e) => {
            log::warn!("update install failed: {e}");
            publish(state, UpdateState::Error(e), sender);
        }
    }
}

fn install(
    kind: &Kind,
    url: &str,
    sha256: Option<&str>,
    auth: Option<&str>,
    state: &Mutex<UpdateState>,
    sender: &UserEventSender,
) -> Result<(), String> {
    // Stage next to the swap targets so the final rename stays on one filesystem.
    let dir = target_dir(kind).ok_or("no in-place install path for this install")?;
    let staging = dir.join(".update-tmp");
    // Start from a clean staging dir (drop any stale attempt).
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| format!("create staging: {e}"))?;

    // 1) Stream the zip to staging, hashing as we go (never held whole in memory —
    //    the binaries make this tens-to-hundreds of MB, even on a 1 GB device).
    let zip_path = staging.join("update.zip");
    let digest = download(url, &zip_path, auth, state, sender).inspect_err(|_| {
        let _ = fs::remove_dir_all(&staging);
    })?;

    publish(state, UpdateState::Installing, sender);

    // 2) Verify the checksum when a sidecar was published; mismatch touches nothing.
    if let Some(expected) = sha256 {
        if !digest.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("checksum mismatch (expected {expected}, got {digest})"));
        }
    }

    // 3) Extract, then transactionally swap the executable(s).
    let result = extract(&zip_path, &staging).and_then(|()| swap(kind, &staging));
    let _ = fs::remove_dir_all(&staging);
    result
}

/// The directory to stage under and fsync after the swap: the gamedir for a
/// PortMaster port, the exe's directory for a single-binary install.
fn target_dir(kind: &Kind) -> Option<PathBuf> {
    match kind {
        Kind::PortMaster { gamedir, .. } => Some(gamedir.clone()),
        Kind::Single { exe, .. } => exe.parent().map(Path::to_path_buf),
        Kind::Manual => None,
    }
}

/// The `(extracted src, live dst)` pairs to swap for this kind. A release that omits
/// a variant just leaves its src absent, and the swap skips it.
fn swap_pairs(kind: &Kind, staging: &Path) -> Vec<(PathBuf, PathBuf)> {
    match kind {
        Kind::PortMaster { gamedir, launcher } => {
            // Binaries live under the extracted `retsurf/`; the launcher at the root.
            let extracted = staging.join("retsurf");
            let mut pairs: Vec<(PathBuf, PathBuf)> = BINARIES
                .iter()
                .map(|name| (extracted.join(name), gamedir.join(name)))
                .collect();
            pairs.push((staging.join("Retsurf.sh"), launcher.clone()));
            pairs
        }
        // Single binary: the extracted `bin` at the zip root -> the running exe.
        Kind::Single { exe, bin, .. } => vec![(staging.join(bin), exe.clone())],
        Kind::Manual => Vec::new(),
    }
}

/// Stream `url` into `dest` via a `.part` temp, returning the lowercase hex SHA-256
/// of the bytes written. Publishes `Downloading{received,total}` on a throttle so
/// the About tab shows progress.
fn download(
    url: &str,
    dest: &Path,
    auth: Option<&str>,
    state: &Mutex<UpdateState>,
    sender: &UserEventSender,
) -> Result<String, String> {
    // The CI channel downloads from the GitHub API and needs the token; ureq drops
    // the Authorization header on the 302 to blob storage (redirect_auth_headers =
    // Never), so it never leaks to the CDN. Release URLs pass `None`.
    let mut request = ureq::get(url).header("User-Agent", USER_AGENT);
    if let Some(token) = auth {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let response = request.call().map_err(|e| e.to_string())?;
    let total = response
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    publish(state, UpdateState::Downloading { received: 0, total }, sender);

    let part = with_suffix(dest, ".part");
    let mut reader = response.into_body().into_reader();
    let mut file = fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut received = 0u64;
    let mut last_notify = Instant::now();
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
        received += n as u64;
        if last_notify.elapsed() >= NOTIFY_EVERY {
            last_notify = Instant::now();
            publish(state, UpdateState::Downloading { received, total }, sender);
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&part, dest).map_err(|e| format!("rename: {e}"))?;
    Ok(hex(&hasher.finalize()))
}

/// Extract the zip into `dest`. `enclosed_name` rejects absolute/`..` paths, so a
/// malformed archive can't write outside the staging dir.
fn extract(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut writer = fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut writer).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Transactionally swap the executable(s) from the extracted tree over the live
/// ones. On any failure, every pair already swapped is rolled back, so the install
/// is left fully-old. A pair whose src is absent (an omitted variant) is skipped.
fn swap(kind: &Kind, staging: &Path) -> Result<(), String> {
    let pairs = swap_pairs(kind, staging);
    // Each entry: the target we replaced and the backup to restore it from.
    let mut done: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    for (src, dst) in &pairs {
        if !src.is_file() {
            continue; // a release may not ship every variant
        }
        if let Err(e) = make_executable(src) {
            rollback(&done);
            return Err(format!("chmod {}: {e}", src.display()));
        }
        // Flush the new file to disk before it becomes the live target.
        if let Ok(f) = fs::File::open(src) {
            let _ = f.sync_all();
        }
        // Move the current target aside so a failed swap (or later pair) can roll back.
        let backup = if dst.exists() {
            let b = with_suffix(dst, ".bak-update");
            if let Err(e) = fs::rename(dst, &b) {
                rollback(&done);
                return Err(format!("backup {}: {e}", dst.display()));
            }
            Some(b)
        } else {
            None
        };
        if let Err(e) = fs::rename(src, dst) {
            if let Some(b) = &backup {
                let _ = fs::rename(b, dst); // restore this target
            }
            rollback(&done);
            return Err(format!("swap {}: {e}", dst.display()));
        }
        done.push((dst.clone(), backup));
    }
    if done.is_empty() {
        return Err("update archive held none of the expected executables".to_string());
    }

    // Success: persist the directory entries, then drop the backups.
    if let Some(dir) = target_dir(kind) {
        if let Ok(f) = fs::File::open(&dir) {
            let _ = f.sync_all();
        }
    }
    for (_dst, backup) in &done {
        if let Some(b) = backup {
            let _ = fs::remove_file(b);
        }
    }
    Ok(())
}

/// Restore every already-swapped target from its backup (best effort), newest
/// first — mirrors the swap order in reverse.
fn rollback(done: &[(PathBuf, Option<PathBuf>)]) {
    for (dst, backup) in done.iter().rev() {
        let _ = fs::remove_file(dst);
        if let Some(b) = backup {
            let _ = fs::rename(b, dst);
        }
    }
}

/// `set_permissions(0o755)` on Unix; a no-op elsewhere (single-binary installs are
/// Unix-only, but the module still compiles for the Windows/macOS builds).
fn make_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// Append `suffix` to a path's file name (`foo` + `.part` -> `foo.part`).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Lowercase hex of a byte slice (for checksum comparison).
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
