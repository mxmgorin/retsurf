//! The built-in start page's *backing* URL, `retsurf:home`. The start page UI
//! itself is drawn natively by egui (see [`crate::ui::home`] /
//! [`crate::overlay::home`]) so it's navigable with the gamepad like the other
//! overlays; this module only keeps `retsurf:home` a real, navigable URL so the
//! tab list, address bar, and back/forward keep working. A navigation here is
//! intercepted by the `load_web_resource` delegate hook (see [`super::delegate`])
//! and answered with a blank dark page — never fetched — over which the egui
//! overlay is composited.

/// The internal URL the start page lives at. Used as the default `home_page`
/// (see [`crate::config`]); navigations here are intercepted, never fetched, and
/// [`crate::browser::AppBrowser::on_home_page`] matches on it.
pub const HOME_URL: &str = "retsurf:home";

/// Whether `url` is the start-page sentinel.
pub fn is_home(url: &url::Url) -> bool {
    url.as_str() == HOME_URL
}

/// The blank dark page served behind the egui start-page overlay (so there's no
/// white flash and no network request).
pub fn render() -> String {
    "<!doctype html><html><head><meta charset=\"utf-8\">\
     <style>html,body{margin:0;height:100%;background:#16171a}</style>\
     </head><body></body></html>"
        .to_string()
}
