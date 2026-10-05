//! Small painted brand marks for the chrome: the app icon's striped sun setting
//! behind its waves, drawn from primitives so it stays sharp at any scale and
//! needs no texture. Geometry and colors are fractions and stops taken from
//! `resources/retsurf.svg`; the sea under the first wave occludes the sun, so
//! it sets behind a real wave rather than a flat cut.

use super::theme::{lerp_color, ACCENT, PANEL_FILL, SURF_WARM};
use egui_sdl2::egui;

/// The icon gradient's top stop, warmer than [`SURF_WARM`].
const SUN_TOP: egui::Color32 = egui::Color32::from_rgb(0xff, 0xb8, 0x70);

/// Where the gradient reaches [`SURF_WARM`], as a fraction of the diameter.
const WARM_STOP: f32 = 0.3;

/// The icon's two far waves.
const WAVE_FAR: egui::Color32 = egui::Color32::from_rgb(0x26, 0x57, 0x50);
const WAVE_MID: egui::Color32 = egui::Color32::from_rgb(0x33, 0x88, 0x78);

/// The sea under the horizon wave, a shade of the far wave over the panel.
const SEA: egui::Color32 = egui::Color32::from_rgb(0x1a, 0x2a, 0x2a);

/// The halo's reach as a multiple of the sun's radius, and its alpha at the
/// sun's edge; it fades to nothing at the reach.
const HALO_SCALE: f32 = 1.56;
const HALO_ALPHA: u8 = 56;

/// The slats cut across the sun: `(top, height)` as fractions of the visible
/// disk, so a set sun still wears all four.
const SLATS: [(f32, f32); 4] = [
    (0.586, 0.027),
    (0.680, 0.039),
    (0.793, 0.055),
    (0.926, 0.074),
];

/// Band height of the disk mesh (physical px): fine enough for a round edge.
const BAND_PX: f32 = 1.5;

/// Width of the transparent edge ring that stands in for antialiasing.
const FEATHER_PX: f32 = 1.0;

/// Segments of a wave's polyline, and of the halo ring.
const WAVE_SEGMENTS: usize = 48;
const HALO_SEGMENTS: usize = 32;

/// Periods of each wave across the band, and their crest height over the stroke.
const WAVE_PERIODS: f32 = 1.5;
const WAVE_AMP: f32 = 3.0;
const WAVE_STROKE: f32 = 2.0;
/// The near wave's drop under the horizon.
const WAVE_SPACING: f32 = 8.0;

/// The sun of a [`paint_horizon`]: its radius, how much of its diameter has set
/// below the horizon, and how far its colors are pulled toward the panel.
pub(super) struct Horizon {
    pub radius: f32,
    pub sink: f32,
    pub dim: f32,
}

impl Horizon {
    /// From the horizon line down to the near wave's lowest edge.
    pub fn depth() -> f32 {
        WAVE_SPACING + WAVE_AMP + WAVE_STROKE / 2.0
    }

    /// From the halo's top down to the horizon line.
    pub fn rise(&self) -> f32 {
        self.radius * (1.0 + HALO_SCALE) - self.sink * self.radius * 2.0
    }

    /// The mark's full height, halo top to near wave bottom.
    pub fn height(&self) -> f32 {
        self.rise() + Self::depth()
    }

    fn center(&self, horizon: f32, sun_x: f32) -> egui::Pos2 {
        egui::pos2(sun_x, horizon - self.radius + self.sink * self.radius * 2.0)
    }

    fn tint(&self, color: egui::Color32) -> egui::Color32 {
        lerp_color(color, PANEL_FILL, self.dim)
    }
}

/// Paint `sun` centered at `sun_x` behind a wave at `horizon`, the sea under
/// that wave down to `bottom`, and a near wave on the sea; all span `x_range`.
pub(super) fn paint_horizon(
    painter: &egui::Painter,
    x_range: egui::Rangef,
    horizon: f32,
    bottom: f32,
    sun_x: f32,
    sun: &Horizon,
) {
    let ppp = painter.pixels_per_point();
    let center = sun.center(horizon, sun_x);
    painter.add(egui::Shape::mesh(halo_mesh(center, sun)));
    painter.add(egui::Shape::mesh(sun_mesh(ppp, center, sun)));
    let far = wave_points(x_range, horizon, 0.0);
    painter.add(egui::Shape::mesh(sea_mesh(&far, bottom)));
    painter.add(egui::Shape::line(
        far,
        egui::Stroke::new(WAVE_STROKE, WAVE_FAR),
    ));
    let near = wave_points(x_range, horizon + WAVE_SPACING, 0.25);
    painter.add(egui::Shape::line(
        near,
        egui::Stroke::new(WAVE_STROKE, WAVE_MID),
    ));
}

fn wave_points(x_range: egui::Rangef, y: f32, phase: f32) -> Vec<egui::Pos2> {
    (0..=WAVE_SEGMENTS)
        .map(|i| {
            let t = i as f32 / WAVE_SEGMENTS as f32;
            let a = (t * WAVE_PERIODS + phase) * std::f32::consts::TAU;
            egui::pos2(x_range.min + x_range.span() * t, y + WAVE_AMP * a.sin())
        })
        .collect()
}

/// The water from a wave's crest line down to `bottom`: one quad per segment.
fn sea_mesh(wave: &[egui::Pos2], bottom: f32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    for p in wave {
        mesh.colored_vertex(*p, SEA);
        mesh.colored_vertex(egui::pos2(p.x, bottom), SEA);
    }
    for i in 0..wave.len() as u32 - 1 {
        let (t, b) = (2 * i, 2 * i + 1);
        mesh.add_triangle(t, t + 2, b + 2);
        mesh.add_triangle(t, b + 2, b);
    }
    mesh
}

/// The coral glow ring from the sun's edge out to [`HALO_SCALE`].
fn halo_mesh(center: egui::Pos2, sun: &Horizon) -> egui::Mesh {
    let inner = sun.tint(SURF_WARM);
    let inner = egui::Color32::from_rgba_unmultiplied(inner.r(), inner.g(), inner.b(), HALO_ALPHA);
    let mut mesh = egui::Mesh::default();
    for i in 0..HALO_SEGMENTS as u32 {
        let a = i as f32 / HALO_SEGMENTS as f32 * std::f32::consts::TAU;
        let dir = egui::vec2(a.cos(), a.sin());
        mesh.colored_vertex(center + dir * sun.radius, inner);
        mesh.colored_vertex(
            center + dir * sun.radius * HALO_SCALE,
            egui::Color32::TRANSPARENT,
        );
    }
    for i in 0..HALO_SEGMENTS as u32 {
        let (a, b) = (2 * i, 2 * ((i + 1) % HALO_SEGMENTS as u32));
        mesh.add_triangle(a, b, b + 1);
        mesh.add_triangle(a, b + 1, a + 1);
    }
    mesh
}

/// The visible part of the disk as horizontal bands with the slats left out,
/// colored by the icon gradient, each band's ends feathered.
fn sun_mesh(ppp: f32, center: egui::Pos2, sun: &Horizon) -> egui::Mesh {
    let radius = sun.radius;
    let d = radius * 2.0;
    let top = center.y - radius;
    let cut = 1.0 - sun.sink;
    let step = (BAND_PX / ppp) / d;
    let feather = FEATHER_PX / ppp;
    let half_width = |f: f32| {
        let dy = (f - 0.5) * d;
        (radius * radius - dy * dy).max(0.0).sqrt()
    };
    let mut mesh = egui::Mesh::default();
    let mut solid = |a: f32, b: f32| {
        let mut f0 = a;
        while f0 < b {
            let f1 = (f0 + step).min(b);
            let rows = [f0, f1].map(|f| (top + f * d, half_width(f), sun.tint(gradient(f))));
            let base = mesh.vertices.len() as u32;
            for (y, w, color) in rows {
                for (dx, c) in [
                    (-w - feather, egui::Color32::TRANSPARENT),
                    (-w, color),
                    (w, color),
                    (w + feather, egui::Color32::TRANSPARENT),
                ] {
                    mesh.colored_vertex(egui::pos2(center.x + dx, y), c);
                }
            }
            for i in 0..3 {
                let (t, b) = (base + i, base + 4 + i);
                mesh.add_triangle(t, t + 1, b + 1);
                mesh.add_triangle(t, b + 1, b);
            }
            f0 = f1;
        }
    };
    let mut from = 0.0;
    for (slat_top, slat_h) in SLATS {
        solid(from, slat_top * cut);
        from = (slat_top + slat_h) * cut;
    }
    solid(from, cut);
    mesh
}

/// The icon gradient at `f` of the way down the disk.
fn gradient(f: f32) -> egui::Color32 {
    match f < WARM_STOP {
        true => lerp_color(SUN_TOP, SURF_WARM, f / WARM_STOP),
        false => lerp_color(SURF_WARM, ACCENT, (f - WARM_STOP) / (1.0 - WARM_STOP)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slats_are_ordered_within_the_disk() {
        let mut last_bottom = 0.0;
        for (top, h) in SLATS {
            assert!(top > last_bottom && h > 0.0 && top + h <= 1.0, "{top} {h}");
            last_bottom = top + h;
        }
    }

    /// Every band stays inside the disk's box, and none dips below the cut.
    #[test]
    fn sun_mesh_stays_within_the_visible_disk() {
        let sun = Horizon {
            radius: 18.0,
            sink: 0.2,
            dim: 0.0,
        };
        let center = egui::pos2(100.0, 100.0);
        let cut_y = center.y - sun.radius + (1.0 - sun.sink) * sun.radius * 2.0;
        for ppp in [1.0, 2.0] {
            let feather = FEATHER_PX / ppp;
            let mesh = sun_mesh(ppp, center, &sun);
            assert!(!mesh.vertices.is_empty());
            for v in &mesh.vertices {
                assert!((v.pos.x - center.x).abs() <= sun.radius + feather + 1e-3);
                assert!(v.pos.y >= center.y - sun.radius - 1e-3);
                assert!(v.pos.y <= cut_y + 1e-3);
            }
        }
    }

    #[test]
    fn height_is_rise_plus_depth() {
        let sun = Horizon {
            radius: 10.0,
            sink: 0.0,
            dim: 0.0,
        };
        assert_eq!(sun.rise(), 10.0 * (1.0 + HALO_SCALE));
        assert_eq!(sun.height(), sun.rise() + Horizon::depth());
    }
}
