//! Fitting the chrome to the panel: one zoom factor scales the whole design
//! (drawn against [`BASE_SIZE`]), and the page follows it so a CSS pixel and a
//! point stay the same size.

use super::AppUi;
use crate::browser::AppBrowser;
use egui_sdl2::egui;

/// The panel the chrome was drawn for; every other one is that, scaled. A pair
/// of edges, not a width and a height: [`wanted_scale`] fits it either way round.
const BASE_SIZE: egui::Vec2 = egui::vec2(640.0, 480.0);
/// Bounds on the zoom actually installed, whatever the fit and the setting
/// between them ask for.
const SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=4.0;
const SCALE_STEP: f32 = crate::config::bounds::SCALE_STEP as f32;
/// A panel reported past this is egui's placeholder rect, not a screen; the real
/// size arrives with the first frame.
const MAX_PANEL: f32 = 8192.0;
/// How far past a whole number a fit may reach before the fraction is worth it:
/// fractional zoom lands glyphs between pixels, and the spare pixels become page.
const WHOLE_ZOOM_REACH: f32 = 0.25;

impl AppUi {
    /// Scale the chrome to its panel, then by the setting. Every renderer size
    /// is points against the 640x480 design, so one zoom carries the lot; the
    /// page follows the same factor, so a CSS pixel and a point stay equal.
    pub(super) fn sync_scale(&mut self, browser: &AppBrowser) {
        let ctx = &self.egui_ctx;
        let installed = ctx.zoom_factor();
        let native = panel_points(ctx.content_rect().size(), installed);
        if !(1.0..=MAX_PANEL).contains(&native.x) || !(1.0..=MAX_PANEL).contains(&native.y) {
            return;
        }
        let wanted = wanted_scale(native, self.forced_scale, self.ui_scale);
        browser.set_hidpi(wanted);
        if (wanted - installed).abs() < f32::EPSILON {
            return;
        }
        // Two decimals: snapping to the step grid leaves float noise in the tail.
        log::info!("ui scale {wanted:.2} for {}x{} points", native.x, native.y);
        ctx.set_zoom_factor(wanted);
        self.forced_passes = self.forced_passes.max(1);
    }

    /// Adopt an edited `[display] scale`; the next frame installs it.
    pub fn set_ui_scale(&mut self, scale: f32) {
        self.ui_scale = scale;
    }
}

/// The panel behind a laid-out rect, in points at zoom 1: the drawn area with the
/// zoom divided back out, rounded so the measurement does not feed on its own
/// output and swap between two steps every frame.
fn panel_points(content: egui::Vec2, zoom: f32) -> egui::Vec2 {
    (content * zoom).round()
}

/// The zoom a panel of `native` points wants: what the design fits into it, or
/// `forced` where a launcher said so, taken `user` times over and snapped to a
/// step the layout can settle on.
fn wanted_scale(native: egui::Vec2, forced: Option<f32>, user: f32) -> f32 {
    // Long edge to long edge: a turned screen is the same screen, so the chrome
    // keeps its size and the page gets the spare measure instead.
    let fit =
        (native.max_elem() / BASE_SIZE.max_elem()).min(native.min_elem() / BASE_SIZE.min_elem());
    let fit = if fit >= 1.0 && fit.fract() <= WHOLE_ZOOM_REACH {
        fit.floor()
    } else {
        fit
    };
    let wanted = forced.unwrap_or(fit) * user;
    ((wanted / SCALE_STEP).round() * SCALE_STEP).clamp(*SCALE_RANGE.start(), *SCALE_RANGE.end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::bounds::SCALE;

    /// The panel the chrome was drawn for, the Miyoo Flip's, and a desktop window.
    const HANDHELD: egui::Vec2 = BASE_SIZE;
    const FLIP: egui::Vec2 = egui::vec2(752.0, 560.0);
    const DESKTOP: egui::Vec2 = egui::vec2(1280.0, 720.0);
    /// The same handheld held the other way up.
    const TURNED: egui::Vec2 = egui::vec2(BASE_SIZE.y, BASE_SIZE.x);

    /// Snapping multiplies the step back out, so the answers land a rounding
    /// short of the round number they read as.
    fn assert_scale(got: f32, want: f32) {
        assert!((got - want).abs() < SCALE_STEP / 2.0, "{got} is not {want}");
    }

    /// Every scale the setting can be stepped to.
    fn user_scales() -> impl Iterator<Item = f32> {
        let (min, max, step) = (SCALE.min as f32, SCALE.max as f32, SCALE_STEP);
        let steps = ((max - min) / step).round() as i32;
        (0..=steps).map(move |i| min + i as f32 * step)
    }

    #[test]
    fn the_design_lands_at_one_on_the_panel_it_was_drawn_for() {
        assert_scale(wanted_scale(HANDHELD, None, 1.0), 1.0);
        assert_scale(wanted_scale(DESKTOP, None, 1.0), 1.5);
    }

    /// The Flip's panel is 17% past the design, which is not worth rendering type
    /// between pixels for; the spare pixels widen the page instead.
    #[test]
    fn a_panel_a_little_past_the_design_keeps_a_whole_zoom() {
        assert_scale(wanted_scale(FLIP, None, 1.0), 1.0);
    }

    #[test]
    fn a_quarter_turn_hands_back_the_same_screen_and_the_same_sized_chrome() {
        assert_scale(wanted_scale(TURNED, None, 1.0), 1.0);
        for panel in [HANDHELD, FLIP, DESKTOP] {
            let turned = egui::vec2(panel.y, panel.x);
            for user in user_scales() {
                assert_eq!(
                    wanted_scale(panel, None, user),
                    wanted_scale(turned, None, user),
                    "{panel:?} at {user} resizes itself when turned"
                );
            }
        }
    }

    #[test]
    fn the_setting_is_read_against_whatever_the_panel_asked_for() {
        assert_scale(wanted_scale(HANDHELD, None, 1.2), 1.2);
        assert_scale(wanted_scale(DESKTOP, None, 1.2), 1.8);
        // A launcher's pin (Android's density) is a base like any other.
        assert_scale(wanted_scale(DESKTOP, Some(2.0), 1.2), 2.4);
    }

    #[test]
    fn neither_end_of_the_setting_can_push_the_zoom_out_of_range() {
        for panel in [HANDHELD, DESKTOP, egui::vec2(7680.0, 4320.0)] {
            for user in [SCALE.min as f32, SCALE.max as f32] {
                let scale = wanted_scale(panel, None, user);
                assert!(
                    SCALE_RANGE.contains(&scale),
                    "{panel:?} at {user} -> {scale}"
                );
            }
        }
    }

    /// What `Context::content_rect` hands back for a panel at `zoom`: the points
    /// it lays out in, snapped to egui's 1/32pt grid.
    fn content_rect(panel: egui::Vec2, zoom: f32) -> egui::Vec2 {
        use egui::emath::GuiRounding as _;
        (panel / zoom).round_ui()
    }

    /// One frame: measure the panel through the zoom in force, ask for the next.
    fn next_zoom(panel: egui::Vec2, zoom: f32, user: f32) -> f32 {
        wanted_scale(panel_points(content_rect(panel, zoom), zoom), None, user)
    }

    #[test]
    fn a_scale_settles_rather_than_swapping_between_two_steps_every_frame() {
        for panel in [HANDHELD, FLIP, TURNED, DESKTOP] {
            for user in user_scales() {
                let mut zoom = 1.0;
                for _ in 0..8 {
                    zoom = next_zoom(panel, zoom, user);
                }
                assert_eq!(
                    next_zoom(panel, zoom, user),
                    zoom,
                    "{panel:?} at {user} swaps"
                );
            }
        }
    }

    /// A setting stepping by 5% into a zoom snapped to 10% spends half its presses
    /// on nothing at all.
    #[test]
    fn every_step_of_the_setting_moves_the_panel_the_design_was_drawn_for() {
        let mut zooms: Vec<f32> = user_scales()
            .map(|user| wanted_scale(HANDHELD, None, user))
            .collect();
        let asked = zooms.len();
        zooms.dedup();
        assert_eq!(zooms.len(), asked, "{zooms:?} repeats a zoom");
    }
}
