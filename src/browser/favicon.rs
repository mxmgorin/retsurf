//! Page icons: the page's favicon, shrunk to row size on arrival. Servo hands it
//! over at its decoded size (a 512 px PNG or a 250 px SVG raster is common), and
//! only a copy this small is worth keeping.

use servo::PixelFormat;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Longest side of a stored icon (px); smaller icons keep their size.
pub const FAVICON_PX: usize = 32;
/// Mean luma (0..=255) of the opaque pixels below which an icon counts as dark.
const DARK_LUMA: u32 = 72;
/// Alpha from which a pixel counts towards [`DARK_LUMA`] and the tint.
const OPAQUE_ALPHA: u8 = 128;
/// Hue buckets the tint is voted in (30 degrees each).
const HUE_BINS: usize = 12;
/// Channel spread (max - min) below which a pixel is grey and has no vote.
const CHROMA_MIN: u8 = 48;
/// Coloured pixels must be at least 1 in this many opaque ones to set the tint.
const CHROMATIC_SHARE: usize = 8;

/// One page icon, straight-alpha RGBA8.
#[derive(Clone)]
pub struct Favicon {
    /// Unique per icon built, so a replaced icon is told apart.
    pub id: u64,
    pub width: usize,
    pub height: usize,
    pub rgba: Rc<[u8]>,
    /// Too dark to read on the dark chrome without a light plate behind it.
    pub dark: bool,
    /// The icon's dominant colour: the mean of its most common hue, or of all
    /// opaque pixels when too few are coloured; black if none are opaque.
    pub tint: [u8; 3],
}

impl Favicon {
    /// `None` when `rgba` is not `width * height` pixels.
    pub fn new(width: usize, height: usize, rgba: Vec<u8>) -> Option<Self> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        if width == 0 || height == 0 || rgba.len() != width * height * 4 {
            return None;
        }
        let opaque: Vec<[u8; 3]> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] >= OPAQUE_ALPHA)
            .map(|&[r, g, b, _]| [r, g, b])
            .collect();
        let mean = mean(&opaque);
        Some(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            dark: mean.is_some_and(|m| luma(m) < DARK_LUMA),
            tint: dominant(&opaque).or(mean).unwrap_or_default(),
            rgba: rgba.into(),
        })
    }

    /// Shrink `image` to fit [`FAVICON_PX`]; `None` for an empty or short buffer.
    pub fn from_image(image: &servo::Image) -> Option<Self> {
        downscale(
            image.width as usize,
            image.height as usize,
            &image.format,
            image.data(),
        )
    }
}

fn mean(pixels: &[[u8; 3]]) -> Option<[u8; 3]> {
    let mut sum = [0u32; 3];
    for p in pixels {
        for (s, c) in sum.iter_mut().zip(p) {
            *s += u32::from(*c);
        }
    }
    let n = pixels.len() as u32;
    (n > 0).then(|| sum.map(|s| (s / n) as u8))
}

/// Mean of the largest hue bucket; `None` when coloured pixels fall short of
/// [`CHROMATIC_SHARE`]. A tie goes to the lower hue.
fn dominant(pixels: &[[u8; 3]]) -> Option<[u8; 3]> {
    let mut bins: [Vec<[u8; 3]>; HUE_BINS] = Default::default();
    for &p in pixels {
        if let Some(bin) = hue_bin(p) {
            bins[bin].push(p);
        }
    }
    let coloured: usize = bins.iter().map(Vec::len).sum();
    if coloured == 0 || coloured * CHROMATIC_SHARE < pixels.len() {
        return None;
    }
    let mut best = &bins[0];
    for bin in &bins[1..] {
        if bin.len() > best.len() {
            best = bin;
        }
    }
    mean(best)
}

/// The hue bucket of `p`, or `None` for a grey below [`CHROMA_MIN`].
fn hue_bin(p: [u8; 3]) -> Option<usize> {
    let (max, min) = (p.into_iter().max()?, p.into_iter().min()?);
    if max - min < CHROMA_MIN {
        return None;
    }
    let [r, g, b] = p.map(f32::from);
    let c = f32::from(max - min);
    // Hue in sextants, 0..6, from whichever channel is the largest.
    let hue = if max == p[0] {
        ((g - b) / c).rem_euclid(6.0)
    } else if max == p[1] {
        (b - r) / c + 2.0
    } else {
        (r - g) / c + 4.0
    };
    Some(((hue / 6.0 * HUE_BINS as f32) as usize).min(HUE_BINS - 1))
}

/// Rec. 709 luma in fixed point; the weights sum to 256.
fn luma([r, g, b]: [u8; 3]) -> u32 {
    (54 * u32::from(r) + 183 * u32::from(g) + 19 * u32::from(b)) >> 8
}

/// Box-filter `data` down to fit [`FAVICON_PX`]. Averaging is only right on
/// premultiplied pixels, which is what Servo's raster decoder stores.
fn downscale(sw: usize, sh: usize, format: &PixelFormat, data: &[u8]) -> Option<Favicon> {
    let bpp = match format {
        PixelFormat::K8 => 1,
        PixelFormat::KA8 => 2,
        PixelFormat::RGB8 => 3,
        PixelFormat::RGBA8 | PixelFormat::BGRA8 => 4,
    };
    if sw == 0 || sh == 0 || data.len() < sw * sh * bpp {
        return None;
    }
    let longest = sw.max(sh);
    let (dw, dh) = if longest <= FAVICON_PX {
        (sw, sh)
    } else {
        (
            (sw * FAVICON_PX / longest).max(1),
            (sh * FAVICON_PX / longest).max(1),
        )
    };

    let mut rgba = Vec::with_capacity(dw * dh * 4);
    for dy in 0..dh {
        // `dh <= sh`, so every destination row covers at least one source row.
        let (y0, y1) = (dy * sh / dh, (dy + 1) * sh / dh);
        for dx in 0..dw {
            let (x0, x1) = (dx * sw / dw, (dx + 1) * sw / dw);
            let mut sum = [0u32; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let at = (y * sw + x) * bpp;
                    let px = to_rgba(format, &data[at..at + bpp]);
                    for (s, c) in sum.iter_mut().zip(px) {
                        *s += u32::from(c);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            rgba.extend(unpremultiply(sum.map(|s| (s + n / 2) / n)));
        }
    }
    Favicon::new(dw, dh, rgba)
}

fn unpremultiply([r, g, b, a]: [u32; 4]) -> [u8; 4] {
    if a == 0 {
        return [0; 4];
    }
    let un = |c: u32| ((c * 255 + a / 2) / a).min(255) as u8;
    [un(r), un(g), un(b), a as u8]
}

fn to_rgba(format: &PixelFormat, p: &[u8]) -> [u8; 4] {
    match format {
        PixelFormat::K8 => [p[0], p[0], p[0], u8::MAX],
        PixelFormat::KA8 => [p[0], p[0], p[0], p[1]],
        PixelFormat::RGB8 => [p[0], p[1], p[2], u8::MAX],
        PixelFormat::RGBA8 => [p[0], p[1], p[2], p[3]],
        PixelFormat::BGRA8 => [p[2], p[1], p[0], p[3]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_icon_is_averaged_down_to_the_cap() {
        let side = FAVICON_PX * 2;
        // Alternating black and white columns average to mid-grey.
        let data: Vec<u8> = (0..side * side)
            .flat_map(|i| {
                let v = if i % 2 == 0 { 0 } else { u8::MAX };
                [v, v, v, u8::MAX]
            })
            .collect();
        let icon = downscale(side, side, &PixelFormat::RGBA8, &data).unwrap();

        assert_eq!((icon.width, icon.height), (FAVICON_PX, FAVICON_PX));
        assert_eq!(&icon.rgba[..4], &[128, 128, 128, u8::MAX]);
    }

    #[test]
    fn small_icon_keeps_its_size_and_wide_one_its_aspect() {
        let small = vec![0; 16 * 16];
        let icon = downscale(16, 16, &PixelFormat::K8, &small).unwrap();
        assert_eq!((icon.width, icon.height), (16, 16));

        let wide = vec![0; 128 * 32 * 3];
        let icon = downscale(128, 32, &PixelFormat::RGB8, &wide).unwrap();
        assert_eq!((icon.width, icon.height), (FAVICON_PX, FAVICON_PX / 4));
    }

    #[test]
    fn bgra_is_swapped_and_short_buffers_rejected() {
        let icon = downscale(1, 1, &PixelFormat::BGRA8, &[1, 2, 3, u8::MAX]).unwrap();
        assert_eq!(&icon.rgba[..], &[3, 2, 1, u8::MAX]);

        assert!(downscale(2, 2, &PixelFormat::RGBA8, &[0; 15]).is_none());
        assert!(downscale(0, 4, &PixelFormat::RGBA8, &[]).is_none());
    }

    #[test]
    fn premultiplied_input_comes_out_straight() {
        // Half-transparent white, premultiplied.
        let icon = downscale(1, 1, &PixelFormat::RGBA8, &[128, 128, 128, 128]).unwrap();
        assert_eq!(&icon.rgba[..], &[255, 255, 255, 128]);
    }

    #[test]
    fn dark_ignores_transparent_pixels() {
        let black = [0, 0, 0, u8::MAX];
        let clear_white = [u8::MAX, u8::MAX, u8::MAX, 0];
        let icon = Favicon::new(2, 1, [black, clear_white].concat()).unwrap();
        assert!(icon.dark);

        let white = [u8::MAX; 4];
        assert!(!Favicon::new(1, 1, white.to_vec()).unwrap().dark);
        assert!(!Favicon::new(1, 1, vec![0; 4]).unwrap().dark);
    }

    #[test]
    fn tint_takes_the_dominant_hue_not_the_mean() {
        let red = [220, 20, 20, u8::MAX];
        let yellow = [240, 200, 20, u8::MAX];
        let clear_blue = [0, 0, u8::MAX, 0];
        // Two reds to one yellow: red wins, rather than a red-yellow brown.
        let icon = Favicon::new(4, 1, [red, red, yellow, clear_blue].concat()).unwrap();
        assert_eq!(icon.tint, [220, 20, 20]);
    }

    #[test]
    fn tint_of_a_mostly_grey_icon_is_its_mean() {
        let white = [240, 240, 240, u8::MAX];
        let blue = [0, 0, 200, u8::MAX];
        let mut pixels = vec![white; CHROMATIC_SHARE];
        pixels.push(blue);
        let icon = Favicon::new(pixels.len(), 1, pixels.concat()).unwrap();
        assert_eq!(
            icon.tint,
            mean(
                &pixels
                    .iter()
                    .map(|&[r, g, b, _]| [r, g, b])
                    .collect::<Vec<_>>()
            )
            .unwrap()
        );
    }

    #[test]
    fn hue_bins_follow_the_colour_wheel() {
        assert_eq!(hue_bin([255, 0, 0]), Some(0));
        assert_eq!(hue_bin([0, 255, 0]), Some(HUE_BINS / 3));
        assert_eq!(hue_bin([0, 0, 255]), Some(2 * HUE_BINS / 3));
        assert_eq!(hue_bin([128, 128, 140]), None);
    }
}
