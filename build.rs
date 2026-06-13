//! Bakes the facts shown on the settings → About tab into the binary as
//! compile-time env vars (read with `env!` in `src/overlay/settings.rs`):
//!
//! * `RETSURF_GIT_HASH` / `RETSURF_BUILD_DATE` — the short hash and committer
//!   date of `HEAD`, pinning the exact source the build came from.
//! * `RETSURF_VER_*` — the *resolved* versions of the headline components, read
//!   from `Cargo.lock` so they track the actual dependency graph rather than the
//!   looser semver ranges in `Cargo.toml`.
//!
//! All vars are always emitted (falling back to `"unknown"`) so the `env!`s never
//! fail to compile, on a git checkout or a source tarball alike.

use std::process::Command;

/// Components surfaced on the About tab, as `(Cargo.lock package name, env-var
/// suffix)`. The display label lives next to `about_info()` in the overlay.
const COMPONENTS: &[(&str, &str)] = &[
    ("servo", "SERVO"),
    ("egui", "EGUI"),
    ("surfman", "SURFMAN"),
    ("sdl2", "SDL2"),
];

fn main() {
    let hash = git(&["rev-parse", "--short", "HEAD"]);
    let date = git(&["show", "-s", "--format=%cs", "HEAD"]);
    println!("cargo:rustc-env=RETSURF_GIT_HASH={hash}");
    println!("cargo:rustc-env=RETSURF_BUILD_DATE={date}");

    let lock = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    for (pkg, suffix) in COMPONENTS {
        let ver = lock_version(&lock, pkg).unwrap_or_else(|| "unknown".to_string());
        println!("cargo:rustc-env=RETSURF_VER_{suffix}={ver}");
    }

    // Re-run only when the things we read can change.
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=.git/HEAD");
}

/// Run `git <args>`, returning the trimmed stdout or `"unknown"` (no repo, no git
/// on PATH, or a failing command — e.g. a source tarball without `.git`).
fn git(args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Pull a package's resolved `version` out of `Cargo.lock` text. Each entry is a
/// `[[package]]` block with `name`/`version` lines; we match the version that
/// follows the wanted name. Hand-rolled to keep build deps at zero.
fn lock_version(lock: &str, pkg: &str) -> Option<String> {
    let needle = format!("name = \"{pkg}\"");
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim() == needle {
            for next in lines.by_ref() {
                if let Some(v) = next.trim().strip_prefix("version = \"") {
                    return v.strip_suffix('"').map(str::to_string);
                }
                // A package block always lists `version` right after `name`; bail
                // if we somehow hit the next entry first.
                if next.trim() == "[[package]]" {
                    break;
                }
            }
        }
    }
    None
}
