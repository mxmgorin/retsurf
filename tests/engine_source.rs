//! Guards where the engine comes from.
//!
//! When a version requirement stops matching the fork's workspace version — a
//! Servo minor bump, typically — cargo does not fail. It warns `patch ... was not
//! used in the crate graph`, exits 0, and links the unpatched crates.io release,
//! silently dropping the fixes in `patches/`. See docs/SERVO_WORKFLOW.md.

/// Source prefix cargo records in `Cargo.lock` for crates taken from the fork.
const FORK_SOURCE: &str = "git+https://github.com/mxmgorin/servo";

/// Crates retsurf patches directly; the rest of the workspace follows them. The
/// media pair belongs here only while we track `main` — drop it on the 0.4 line.
const PATCHED_CRATES: &[&str] = &["servo", "servo-base", "servo-media", "servo-media-dummy"];

/// The `source` of `package` as recorded in `Cargo.lock`, or `None` when the
/// package has no source entry (a path dependency).
fn locked_source(lock: &str, package: &str) -> Option<String> {
    let name_line = format!("name = \"{package}\"");
    lock.split("[[package]]")
        .find(|block| block.lines().any(|line| line.trim() == name_line))?
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("source = \"")?
                .strip_suffix('"')
                .map(str::to_owned)
        })
}

/// Panics unless every patched crate in `lock` comes from the fork, pinned by rev.
fn assert_patched_by_fork(lock: &str) {
    for &crate_name in PATCHED_CRATES {
        let source = locked_source(lock, crate_name)
            .unwrap_or_else(|| panic!("`{crate_name}` has no source in Cargo.lock"));
        assert!(
            source.starts_with(FORK_SOURCE),
            "`{crate_name}` resolved to `{source}` instead of the fork. The \
             `[patch.crates-io]` override did not apply — check that the version \
             requirement in Cargo.toml still matches the fork's workspace version, \
             and re-run `cargo build` to refresh Cargo.lock."
        );
        assert!(
            source.contains("?rev="),
            "`{crate_name}` comes from the fork but is not pinned by rev: `{source}`"
        );
    }
}

#[test]
fn engine_comes_from_the_fork() {
    let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock"))
        .expect("Cargo.lock sits next to the manifest and is committed");
    assert_patched_by_fork(&lock);
}

/// Lock entries in the shape cargo writes, one per patched crate, so the guard is
/// exercised against the failure it exists for rather than only the passing tree.
fn lock_with_source(source: &str) -> String {
    PATCHED_CRATES
        .iter()
        .map(|name| {
            format!(
                "[[package]]\nname = \"{name}\"\nversion = \"0.4.0\"\nsource = \"{source}\"\n\n"
            )
        })
        .collect()
}

/// The case this guard exists for: the patch silently did not apply and cargo
/// resolved the unpatched release from crates.io.
#[test]
#[should_panic(expected = "instead of the fork")]
fn registry_source_is_rejected() {
    assert_patched_by_fork(&lock_with_source(
        "registry+https://github.com/rust-lang/crates.io-index",
    ));
}

/// Following a branch instead of a rev builds a different engine on every fetch.
#[test]
#[should_panic(expected = "not pinned by rev")]
fn unpinned_fork_source_is_rejected() {
    assert_patched_by_fork(&lock_with_source(
        "git+https://github.com/mxmgorin/servo?branch=retsurf-0.4#df7b6a5c",
    ));
}
