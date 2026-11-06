//! A bundled SVG drawn at one logical height, rasterized for the current pixel
//! density on first use and again only when the density or the palette changes.
//! Its brand colors follow the palette (see [`theme::recolor_svg`]).

use super::theme::{self, Palette};
use egui_sdl2::egui;

/// An SVG and the texture it last rasterized to.
pub(super) struct SvgTexture {
    /// The egui texture name, also the log's name for the image.
    name: &'static str,
    svg: &'static [u8],
    /// Drawn height (logical px); the width follows the SVG's aspect.
    height: f32,
    /// The texture, and the pixel density and palette it was drawn for.
    texture: Option<(f32, &'static Palette, egui::TextureHandle)>,
    /// The SVG would not rasterize; not retried every frame.
    failed: bool,
}

impl SvgTexture {
    pub const fn new(name: &'static str, svg: &'static [u8], height: f32) -> Self {
        Self {
            name,
            svg,
            height,
            texture: None,
            failed: false,
        }
    }

    /// The texture for `ctx`'s pixel density; `None` when the SVG does not rasterize.
    pub fn texture(&mut self, ctx: &egui::Context) -> Option<&egui::TextureHandle> {
        let ppp = ctx.pixels_per_point();
        let palette = theme::palette();
        if self.failed {
            return None;
        }
        let stale =
            |(at, drawn, _): &(f32, &Palette, _)| *at != ppp || !std::ptr::eq(*drawn, palette);
        if self.texture.as_ref().is_none_or(stale) {
            let svg = theme::recolor_svg(self.svg);
            let Some(image) = rasterize(&svg, (self.height * ppp).round() as u32) else {
                log::warn!("{}: the SVG did not rasterize", self.name);
                self.failed = true;
                return None;
            };
            let texture = ctx.load_texture(self.name, image, egui::TextureOptions::LINEAR);
            self.texture = Some((ppp, palette, texture));
        }
        self.texture.as_ref().map(|(_, _, t)| t)
    }
}

/// `svg` at `height` px, its width to the SVG's aspect.
fn rasterize(svg: &[u8], height: u32) -> Option<egui::ColorImage> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = height as f32 / size.height();
    let width = (size.width() * scale).round() as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(egui::ColorImage::from_rgba_premultiplied(
        [width as usize, height as usize],
        pixmap.data(),
    ))
}
