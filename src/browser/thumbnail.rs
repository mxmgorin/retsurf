//! Tab thumbnails: a tab's last painted frame, shrunk to a card of the tab
//! grid. Taken from the rendering context while the tab is still the one
//! painted there, since a hidden webview paints nothing.

use servo::RgbaImage;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Widest a thumbnail is stored (px); its height keeps the frame's aspect.
pub const THUMB_W: usize = 192;

/// One tab's picture, opaque RGBA8.
#[derive(Clone)]
pub struct Thumbnail {
    /// Unique per picture taken, so a retaken one is told apart.
    pub id: u64,
    pub width: usize,
    pub height: usize,
    pub rgba: Rc<[u8]>,
}

impl Thumbnail {
    /// `frame` box-filtered down to [`THUMB_W`] across; `None` for an empty one.
    pub fn from_frame(frame: &RgbaImage) -> Option<Self> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let (sw, sh) = (frame.width() as usize, frame.height() as usize);
        if sw == 0 || sh == 0 {
            return None;
        }
        let dw = sw.min(THUMB_W);
        let dh = (sh * dw / sw).max(1);
        let data = frame.as_raw();
        let mut rgba = Vec::with_capacity(dw * dh * 4);
        for dy in 0..dh {
            // `dh <= sh`, so every destination row covers at least one source row.
            let (y0, y1) = (dy * sh / dh, (dy + 1) * sh / dh);
            for dx in 0..dw {
                let (x0, x1) = (dx * sw / dw, (dx + 1) * sw / dw);
                let mut sum = [0u32; 3];
                for y in y0..y1 {
                    let row = &data[(y * sw + x0) * 4..(y * sw + x1) * 4];
                    for px in row.as_chunks::<4>().0 {
                        for (s, c) in sum.iter_mut().zip(px) {
                            *s += u32::from(*c);
                        }
                    }
                }
                let n = ((y1 - y0) * (x1 - x0)) as u32;
                rgba.extend(sum.map(|s| ((s + n / 2) / n) as u8));
                // The page under the chrome is opaque; WebRender's alpha is not meant for it.
                rgba.push(u8::MAX);
            }
        }
        Some(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            width: dw,
            height: dh,
            rgba: rgba.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_frame_is_averaged_down_to_the_width_and_keeps_its_aspect() {
        let (w, h) = (THUMB_W * 2, THUMB_W);
        // Alternating black and white columns average to mid-grey.
        let pixels = (0..w * h)
            .flat_map(|i| match i % 2 {
                0 => [0, 0, 0, 0],
                _ => [255, 255, 255, 0],
            })
            .collect();
        let frame = RgbaImage::from_raw(w as u32, h as u32, pixels).expect("w * h pixels");
        let thumb = Thumbnail::from_frame(&frame).expect("frame is not empty");
        assert_eq!((thumb.width, thumb.height), (THUMB_W, THUMB_W / 2));
        assert_eq!(&thumb.rgba[..4], &[128, 128, 128, u8::MAX]);
    }

    #[test]
    fn a_small_frame_keeps_its_size() {
        let frame = RgbaImage::new(40, 30);
        let thumb = Thumbnail::from_frame(&frame).expect("frame is not empty");
        assert_eq!((thumb.width, thumb.height), (40, 30));
    }

    #[test]
    fn retaken_thumbnails_are_told_apart() {
        let frame = RgbaImage::new(4, 4);
        let (a, b) = (Thumbnail::from_frame(&frame), Thumbnail::from_frame(&frame));
        assert_ne!(a.map(|t| t.id), b.map(|t| t.id));
    }
}
