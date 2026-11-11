//! GPU copies of the tab thumbnails on screen, keyed by picture. Each frame
//! names the pictures it is about to draw and everything else is freed, so no
//! texture outlives the grid that showed it.

use crate::browser::Thumbnail;
use egui_sdl2::egui;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct TabThumbs {
    textures: HashMap<u64, egui::TextureHandle>,
}

impl TabThumbs {
    /// Hold textures for exactly the `wanted` pictures.
    pub fn sync<'a>(&mut self, ctx: &egui::Context, wanted: impl Iterator<Item = &'a Thumbnail>) {
        let mut kept = HashMap::new();
        for thumb in wanted {
            let texture = self.textures.remove(&thumb.id).unwrap_or_else(|| {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [thumb.width, thumb.height],
                    &thumb.rgba,
                );
                ctx.load_texture(
                    format!("tab-thumb-{}", thumb.id),
                    image,
                    egui::TextureOptions::LINEAR,
                )
            });
            kept.insert(thumb.id, texture);
        }
        self.textures = kept;
    }

    /// The texture of `thumb`, if [`Self::sync`] named it.
    pub fn texture(&self, thumb: &Thumbnail) -> Option<&egui::TextureHandle> {
        self.textures.get(&thumb.id)
    }
}
