//! Toasts: a one-line notice over the page for something that happened with no
//! visible effect of its own, gone again after a moment. One at a time; a new
//! one replaces whatever is up.

use super::panel::ROW_RADIUS;
use super::theme::{BORDER, INK, PANEL_FILL, ROW_FONT};
use super::AppUi;
use crate::browser::BrowserNotice;
use crate::config::ToolbarPosition;
use egui_sdl2::egui;
use std::time::{Duration, Instant};

/// The toast's bottom edge above the floor; clears the overlays' bottom hint lines.
const LIFT: f32 = 36.0;

/// How long a toast stays up unless its caller asks for longer.
pub const TOAST: Duration = Duration::from_secs(2);

pub(super) struct Toast {
    text: String,
    until: Instant,
    /// Not drawn yet. A toast raised outside input needs forced passes of its
    /// own, or its first sizing frame is the only one it gets.
    fresh: bool,
}

impl AppUi {
    /// Show `text` for [`TOAST`].
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast_for(text, TOAST);
    }

    pub fn toast_for(&mut self, text: impl Into<String>, shown: Duration) {
        self.toast = Some(Toast {
            text: text.into(),
            until: Instant::now() + shown,
            fresh: true,
        });
    }

    /// Whether a toast went up since the last call.
    pub(super) fn take_fresh_toast(&mut self) -> bool {
        self.toast
            .as_mut()
            .is_some_and(|t| std::mem::take(&mut t.fresh))
    }

    /// Time left on the current toast, or `None` once it has gone.
    pub(super) fn toast_visible_for(&self) -> Option<Duration> {
        self.toast
            .as_ref()
            .and_then(|t| t.until.checked_duration_since(Instant::now()))
    }

    /// Draw the toast as a snackbar: bottom centre, clear of the hint bars and
    /// of whatever holds the bottom edge (a bottom toolbar, the keyboard).
    /// Returns whether a toast was drawn.
    pub(super) fn add_toast(&self, ctx: &egui::Context) -> bool {
        let Some(toast) = self
            .toast
            .as_ref()
            .filter(|_| self.toast_visible_for().is_some())
        else {
            return false;
        };
        let screen = ctx.content_rect();
        let mut floor = screen.bottom();
        if self.toolbar_position == ToolbarPosition::Bottom && self.toolbar_rect.is_positive() {
            floor = floor.min(self.toolbar_rect.top());
        }
        if self.osk.visible {
            floor = floor.min(screen.bottom() - self.osk_height);
        }
        egui::Area::new(egui::Id::new("toast"))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(egui::pos2(screen.center().x, floor - LIFT))
            // Above every overlay: within one order egui raises whichever area
            // was last clicked, which would bury a toast under the menu.
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(PANEL_FILL)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .corner_radius(ROW_RADIUS)
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        let text = egui::RichText::new(&toast.text).color(INK).size(ROW_FONT);
                        ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
                    });
            });
        true
    }
}

/// How a [`BrowserNotice`] reads on screen.
pub fn notice_text(notice: BrowserNotice) -> String {
    match notice {
        BrowserNotice::OldestTabClosed { cap } => {
            format!("Tab limit ({cap}): closed the oldest tab")
        }
        BrowserNotice::PopupRefused { cap } => {
            format!("Tab limit ({cap}): the page could not open a tab")
        }
    }
}
