//! The read-only About tab of the settings overlay: build identity and the
//! resolved versions of the headline dependencies, all baked in at compile time
//! by `build.rs`. [`crate::ui::settings`] lays it out.

/// The read-only facts shown on the [`super::SettingsSection::About`] tab.
/// Everything is baked in at compile time by `build.rs` (see its docs):
/// `version` is the crate version, `git_hash`/`build_date` pin the source, and
/// `components` are the resolved versions of the headline dependencies.
/// `credits` is the attribution block rendered below the table.
pub struct AboutInfo {
    pub version: &'static str,
    pub git_hash: &'static str,
    pub build_date: &'static str,
    /// Short blurb under the title — what retsurf is and how you drive it —
    /// rendered one dim line per entry.
    pub description: &'static [&'static str],
    /// `(display label, resolved version)`, in display order.
    pub components: &'static [(&'static str, &'static str)],
    /// Attribution / licensing lines, shown one per row under the table.
    pub credits: &'static [&'static str],
    /// Clickable `(label, url)` links shown below the credits; selecting one
    /// saves & closes the overlay and loads the URL (see
    /// [`crate::app::SettingsAction::OpenLink`]).
    pub links: &'static [(&'static str, &'static str)],
}

/// Build the About tab's content from the `RETSURF_*` env vars `build.rs` emits.
pub fn about_info() -> AboutInfo {
    AboutInfo {
        version: env!("CARGO_PKG_VERSION"),
        git_hash: env!("RETSURF_GIT_HASH"),
        build_date: env!("RETSURF_BUILD_DATE"),
        description: &[
            "Lightweight web browser powered by the Servo engine.",
            "Full gamepad control: virtual cursor, link hints, on-screen keyboard.",
            "Keyboard, mouse, and touch too — runs on handhelds, desktop, and Android.",
        ],
        components: &[
            ("Servo engine", env!("RETSURF_VER_SERVO")),
            ("egui", env!("RETSURF_VER_EGUI")),
            ("surfman", env!("RETSURF_VER_SURFMAN")),
            ("SDL2", env!("RETSURF_VER_SDL2")),
        ],
        credits: &[
            "Web rendering by the Servo project (MPL-2.0).",
            "UI by egui (MIT OR Apache-2.0).",
            "Windowing & input by SDL2 (zlib).",
            "retsurf is licensed under GPL-3.0.",
        ],
        links: &[
            ("Servo", "https://servo.org"),
            ("egui", "https://egui.rs"),
            ("SDL", "https://libsdl.org"),
            ("Source code", "https://github.com/mxmgorin/retsurf"),
        ],
    }
}
