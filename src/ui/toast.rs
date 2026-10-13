//! Toasts: a one-line notice over the page for something that happened with no
//! visible effect of its own, gone again after a moment. One at a time; a new
//! one replaces whatever is up. Every toast is a pill at the bottom centre: an
//! icon, then words and key caps, fading out at the end of its time.

use super::panel::ROW_RADIUS;
use super::theme::{ACCENT, BORDER, INK, PANEL_FILL, ROW_FONT, SURFACE, WARN};
use super::AppUi;
use crate::browser::BrowserNotice;
use crate::config::ToolbarPosition;
use egui_phosphor::bold;
use egui_sdl2::egui;
use std::time::{Duration, Instant};

/// The toast's bottom edge above the floor; clears the overlays' bottom hint lines.
const LIFT: f32 = 36.0;

/// How long a toast stays up unless its caller asks for longer.
pub const TOAST: Duration = Duration::from_secs(2);

/// The tail of a toast's time over which it fades out.
pub(super) const FADE: Duration = Duration::from_millis(400);

/// What a toast says: an icon, then words and key caps in reading order.
#[derive(Clone, PartialEq, Debug)]
pub struct Notice {
    icon: &'static str,
    /// Something went wrong or was refused, which the icon's colour says.
    warn: bool,
    pub(super) parts: Vec<Part>,
}

/// One run of a [`Notice`]'s line.
#[derive(Clone, PartialEq, Debug)]
pub enum Part {
    Text(String),
    /// A key or button, drawn framed like a key.
    Key(String),
}

impl Notice {
    /// `text` after `icon`.
    pub fn new(icon: &'static str, text: impl Into<String>) -> Self {
        Self::parts(icon, vec![Part::Text(text.into())])
    }

    /// A line of words and keys after `icon`.
    pub fn parts(icon: &'static str, parts: Vec<Part>) -> Self {
        Self {
            icon,
            warn: false,
            parts,
        }
    }

    /// `text` as a warning: something did not happen, or happened against the
    /// user's wishes.
    pub fn warn(text: impl Into<String>) -> Self {
        Self {
            warn: true,
            ..Self::new(bold::WARNING, text)
        }
    }
}

pub(super) struct Toast {
    notice: Notice,
    until: Instant,
    /// Not drawn yet. A toast raised outside input needs forced passes of its
    /// own, or its first sizing frame is the only one it gets.
    fresh: bool,
}

impl AppUi {
    /// Show `notice` for [`TOAST`].
    pub fn toast(&mut self, notice: Notice) {
        self.toast_for(notice, TOAST);
    }

    pub fn toast_for(&mut self, notice: Notice, shown: Duration) {
        self.toast = Some(Toast {
            notice,
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

    /// Draw the toast at the bottom centre, clear of the hint bars and of
    /// whatever holds the bottom edge (a bottom toolbar, the keyboard).
    /// Returns whether a toast was drawn.
    pub(super) fn add_toast(&self, ctx: &egui::Context) -> bool {
        let (Some(toast), Some(left)) = (self.toast.as_ref(), self.toast_visible_for()) else {
            return false;
        };
        let opacity = (left.as_secs_f32() / FADE.as_secs_f32()).min(1.0);
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
                ui.set_opacity(opacity);
                add_pill(ui, &toast.notice);
            });
        true
    }
}

fn add_pill(ui: &mut egui::Ui, notice: &Notice) {
    egui::Frame::new()
        .fill(PANEL_FILL)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(egui::CornerRadius::same(u8::MAX))
        .inner_margin(egui::Margin::symmetric(16, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let tint = if notice.warn { WARN } else { ACCENT };
                ui.label(egui::RichText::new(notice.icon).color(tint).size(ROW_FONT));
                for part in &notice.parts {
                    match part {
                        Part::Text(text) => {
                            let text = egui::RichText::new(text).color(INK).size(ROW_FONT);
                            ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
                        }
                        Part::Key(key) => key_cap(ui, key),
                    }
                }
            });
        });
}

/// One key or button, framed like a key.
fn key_cap(ui: &mut egui::Ui, key: &str) {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(ROW_RADIUS)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(key).color(INK).size(ROW_FONT).strong());
        });
}

/// How a [`BrowserNotice`] reads on screen.
pub fn browser_notice(notice: BrowserNotice) -> Notice {
    Notice::warn(match notice {
        BrowserNotice::OldestTabClosed { cap } => {
            format!("Tab limit ({cap}): closed the oldest tab")
        }
        BrowserNotice::PopupRefused { cap } => {
            format!("Tab limit ({cap}): the page could not open a tab")
        }
    })
}
