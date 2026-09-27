//! GPU copies of the site icons on screen, keyed by host (see
//! [`crate::data::page_icons`]). Each frame names the hosts it is about to draw
//! and everything else is freed, so no texture outlives the list that showed it.
//! A dark icon also gets a copy baked onto a light plate, for surfaces that
//! cannot paint one behind it.

use super::theme::INK;
use crate::browser::Favicon;
use crate::data::page_icons;
use egui_sdl2::egui;
use std::collections::{HashMap, HashSet};

/// Plate margin around a dark icon (texels).
const PLATE_PAD: usize = 3;
/// Plate corner radius (texels).
const PLATE_RADIUS: f32 = 6.0;

pub(super) struct PageIcons {
    enabled: bool,
    entries: HashMap<String, Entry>,
}

/// One host's icon; `textures: None` records that the disk had none, so a
/// missing file is not re-read every frame.
struct Entry {
    id: Option<u64>,
    textures: Option<Textures>,
}

struct Textures {
    row: egui::TextureHandle,
    /// Only a dark icon has one; the others are drawn as `row`.
    bare: Option<egui::TextureHandle>,
    tint: egui::Color32,
    dark: bool,
}

/// A site icon as drawn: `row` for list rows, `bare` with no plate for surfaces
/// that colour themselves after `tint` or, when `dark`, a light fill.
#[derive(Clone, Copy)]
pub(super) struct Icon<'a> {
    pub row: &'a egui::TextureHandle,
    pub bare: &'a egui::TextureHandle,
    pub tint: egui::Color32,
    pub dark: bool,
}

impl PageIcons {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            entries: HashMap::new(),
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.entries.clear();
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Hold textures for exactly the `(url, live icon)` pairs named. A live icon
    /// wins over the disk copy, so a host an open tab shows stays current.
    pub fn sync(&mut self, ctx: &egui::Context, mut wanted: Vec<(&str, Option<&Favicon>)>) {
        if !self.enabled {
            wanted.clear();
        }
        wanted.sort_by_key(|(_, live)| live.is_none());
        let mut seen = HashSet::new();
        for (url, live) in wanted {
            let Some(host) = page_icons::host_key(url) else {
                continue;
            };
            if !seen.insert(host.clone()) {
                continue;
            }
            match live {
                Some(icon) => {
                    if self.entries.get(&host).and_then(|e| e.id) != Some(icon.id) {
                        self.entries.insert(host, Entry::upload(ctx, Some(icon)));
                    }
                }
                None => {
                    self.entries.entry(host).or_insert_with_key(|host| {
                        Entry::upload(ctx, page_icons::load(host).as_ref())
                    });
                }
            }
        }
        self.entries.retain(|host, _| seen.contains(host));
    }

    /// The icon for `url`'s site, if [`Self::sync`] named it and one exists.
    pub fn icon(&self, url: &str) -> Option<Icon<'_>> {
        let host = page_icons::host_key(url)?;
        let t = self.entries.get(&host)?.textures.as_ref()?;
        Some(Icon {
            row: &t.row,
            bare: t.bare.as_ref().unwrap_or(&t.row),
            tint: t.tint,
            dark: t.dark,
        })
    }
}

impl Entry {
    fn upload(ctx: &egui::Context, icon: Option<&Favicon>) -> Self {
        Self {
            id: icon.map(|i| i.id),
            textures: icon.map(|icon| {
                let load = |name: &str, image| {
                    ctx.load_texture(
                        format!("page-icon-{name}-{}", icon.id),
                        image,
                        egui::TextureOptions::LINEAR,
                    )
                };
                let bare = || {
                    egui::ColorImage::from_rgba_unmultiplied([icon.width, icon.height], &icon.rgba)
                };
                let [r, g, b] = icon.tint;
                Textures {
                    row: load("row", if icon.dark { on_plate(icon) } else { bare() }),
                    bare: icon.dark.then(|| load("bare", bare())),
                    tint: egui::Color32::from_rgb(r, g, b),
                    dark: icon.dark,
                }
            }),
        }
    }
}

/// `icon` centred on a rounded [`INK`] square, [`PLATE_PAD`] wider than its
/// longest side.
fn on_plate(icon: &Favicon) -> egui::ColorImage {
    let side = icon.width.max(icon.height) + 2 * PLATE_PAD;
    let (ox, oy) = ((side - icon.width) / 2, (side - icon.height) / 2);
    let pixels = (0..side * side)
        .map(|i| {
            let (x, y) = (i % side, i / side);
            if !in_rounded_square(x, y, side) {
                return egui::Color32::TRANSPARENT;
            }
            let inside = (ox..ox + icon.width).contains(&x) && (oy..oy + icon.height).contains(&y);
            if !inside {
                return INK;
            }
            let at = ((y - oy) * icon.width + (x - ox)) * 4;
            let p = &icon.rgba[at..at + 4];
            let a = u16::from(p[3]);
            let blend = |c: u8, under: u8| {
                ((u16::from(c) * a + u16::from(under) * (255 - a) + 127) / 255) as u8
            };
            egui::Color32::from_rgb(
                blend(p[0], INK.r()),
                blend(p[1], INK.g()),
                blend(p[2], INK.b()),
            )
        })
        .collect();
    egui::ColorImage::new([side, side], pixels)
}

fn in_rounded_square(x: usize, y: usize, side: usize) -> bool {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (lo, hi) = (PLATE_RADIUS, side as f32 - PLATE_RADIUS);
    let (cx, cy) = (px.clamp(lo, hi), py.clamp(lo, hi));
    (px - cx).powi(2) + (py - cy).powi(2) <= PLATE_RADIUS.powi(2)
}
