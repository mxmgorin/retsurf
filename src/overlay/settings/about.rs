//! The read-only About tab of the settings overlay: build identity, the
//! project's links and the GPL legal notices. The build facts are baked in at
//! compile time by `build.rs`, the project facts come from `Cargo.toml`, and the
//! copyright line from `NOTICE`. [`crate::ui::settings`] lays it out.

use std::sync::OnceLock;

/// The project's NOTICE: copyright, license grant and the additional terms.
pub const NOTICE: &str = include_str!("../../../NOTICE");

/// A selectable row on the About tab that loads `url`, with `value` shown at its
/// trailing edge.
pub struct AboutLink {
    pub label: &'static str,
    pub value: &'static str,
    pub url: &'static str,
}

/// The read-only facts shown on the [`super::SettingsSection::About`] tab.
pub struct AboutInfo {
    pub version: &'static str,
    pub git_hash: &'static str,
    pub build_date: &'static str,
    /// One dim line under the title: what retsurf is and who makes it.
    pub tagline: &'static str,
    /// The resolved Servo version, named in the build line.
    pub engine: &'static str,
    /// `(display label, resolved version)` of the headline components.
    pub components: &'static [(&'static str, &'static str)],
    /// The project's own links, in display order.
    pub links: &'static [AboutLink],
    /// GPL Appropriate Legal Notices; NOTICE binds modified versions to keep them.
    pub legal: [&'static str; 3],
    /// The row under [`Self::legal`]: the license and credits page.
    pub license: AboutLink,
}

impl AboutInfo {
    /// The selectable rows in focus order: [`Self::links`], then [`Self::license`].
    pub fn focusable(&self) -> impl Iterator<Item = &AboutLink> {
        self.links.iter().chain(std::iter::once(&self.license))
    }
}

/// Build the About tab's content.
pub fn about_info() -> AboutInfo {
    AboutInfo {
        version: env!("RETSURF_VERSION"),
        git_hash: env!("RETSURF_GIT_HASH"),
        build_date: env!("RETSURF_BUILD_DATE"),
        tagline: concat!("Gamepad-first web browser by ", env!("CARGO_PKG_AUTHORS")),
        engine: env!("RETSURF_VER_SERVO"),
        components: &[
            ("Servo", env!("RETSURF_VER_SERVO")),
            ("egui", env!("RETSURF_VER_EGUI")),
            ("surfman", env!("RETSURF_VER_SURFMAN")),
            ("SDL2", env!("RETSURF_VER_SDL2")),
        ],
        links: &[
            AboutLink {
                label: "Website",
                value: "retsurf.app",
                url: env!("CARGO_PKG_HOMEPAGE"),
            },
            AboutLink {
                label: "Report a bug",
                value: "GitHub",
                url: concat!(
                    env!("CARGO_PKG_REPOSITORY"),
                    "/issues/new?template=bug_report.yml&version=",
                    env!("RETSURF_VERSION"),
                    "%20(",
                    env!("RETSURF_GIT_HASH"),
                    ")"
                ),
            },
            AboutLink {
                label: "Discussions",
                value: "GitHub",
                url: concat!(env!("CARGO_PKG_REPOSITORY"), "/discussions"),
            },
            AboutLink {
                label: "Support the project",
                value: "Ko-fi",
                url: "https://ko-fi.com/mxmgorin",
            },
        ],
        legal: [
            copyright(),
            concat!("Free software under ", env!("CARGO_PKG_LICENSE"), "."),
            "It comes with ABSOLUTELY NO WARRANTY.",
        ],
        license: AboutLink {
            label: "License and credits",
            value: "Open",
            url: crate::browser::ABOUT_URL,
        },
    }
}

/// NOTICE's copyright line.
fn copyright() -> &'static str {
    NOTICE
        .lines()
        .find(|line| line.starts_with("Copyright"))
        .expect("NOTICE carries a copyright line, checked by a test")
}

/// [`crate::config::data_dir`] for display, the home directory written as `~`.
pub fn data_folder() -> &'static str {
    static SHOWN: OnceLock<String> = OnceLock::new();
    SHOWN.get_or_init(|| {
        let home = std::env::var("HOME").ok();
        tilde(&crate::config::data_dir(), home.as_deref())
    })
}

/// `path` with a leading `home` directory written as `~`; an empty path is the
/// working directory.
fn tilde(path: &str, home: Option<&str>) -> String {
    if path.is_empty() {
        return "./".to_string();
    }
    let home = home
        .map(|h| h.trim_end_matches('/'))
        .filter(|h| !h.is_empty());
    match home.and_then(|h| path.strip_prefix(h)) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_has_a_copyright_line() {
        assert!(NOTICE.lines().any(|line| line.starts_with("Copyright")));
    }

    #[test]
    fn tilde_shortens_only_the_home_directory() {
        let home = Some("/home/me");
        assert_eq!(
            tilde("/home/me/.local/share/retsurf/", home),
            "~/.local/share/retsurf/"
        );
        assert_eq!(tilde("/home/me2/retsurf/", home), "/home/me2/retsurf/");
        assert_eq!(tilde("/roms/ports/retsurf/", home), "/roms/ports/retsurf/");
        assert_eq!(tilde("/data/retsurf/", Some("/")), "/data/retsurf/");
        assert_eq!(tilde("/data/retsurf/", None), "/data/retsurf/");
        assert_eq!(tilde("", home), "./");
    }
}
