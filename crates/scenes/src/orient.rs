//! EXIF orientation (TIFF tag 0x0112): how to turn the stored pixels to show them upright.
//!
//! | value | to display the stored image | stored size |
//! |---|---|---|
//! | 1 | as is | W × H |
//! | 2 | mirror left–right | W × H |
//! | 3 | rotate 180° | W × H |
//! | 4 | mirror top–bottom | W × H |
//! | 5 | transpose (mirror on the main diagonal) | H × W |
//! | 6 | rotate 90° clockwise | H × W |
//! | 7 | transverse (mirror on the other diagonal) | H × W |
//! | 8 | rotate 90° counter-clockwise | H × W |
//!
//! [`store`] makes the pixels a camera would store for an orientation, given the upright picture;
//! [`display`] applies an orientation to stored pixels. `display(store(x, o), o) == x`.

use crate::render::Rgb8;
use crate::{Error, Result};

/// The stored pixel shown at display position `(x, y)` of a `w × h` display image.
fn stored_at(o: u8, x: u32, y: u32, w: u32, h: u32) -> (u32, u32) {
    match o {
        2 => (w - 1 - x, y),
        3 => (w - 1 - x, h - 1 - y),
        4 => (x, h - 1 - y),
        5 => (y, x),
        6 => (y, w - 1 - x),
        7 => (h - 1 - y, w - 1 - x),
        8 => (h - 1 - y, x),
        _ => (x, y),
    }
}

fn swaps(o: u8) -> bool {
    (5..=8).contains(&o)
}

fn check(o: u8) -> Result<()> {
    if (1..=8).contains(&o) { Ok(()) } else { Err(Error::Orientation(o)) }
}

/// The pixels to store so that orientation `o` displays `upright`.
pub fn store(upright: &Rgb8, o: u8) -> Result<Rgb8> {
    check(o)?;
    let (w, h) = (upright.width, upright.height);
    let mut out = if swaps(o) { Rgb8::new(h, w)? } else { Rgb8::new(w, h)? };
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = stored_at(o, x, y, w, h);
            if let Some(px) = upright.get(x, y) {
                out.set(sx, sy, px);
            }
        }
    }
    Ok(out)
}

/// Stored pixels shown with orientation `o`.
pub fn display(stored: &Rgb8, o: u8) -> Result<Rgb8> {
    check(o)?;
    let (w, h) = if swaps(o) { (stored.height, stored.width) } else { (stored.width, stored.height) };
    let mut out = Rgb8::new(w, h)?;
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = stored_at(o, x, y, w, h);
            if let Some(px) = stored.get(sx, sy) {
                out.set(x, y, px);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scene;

    #[test]
    fn display_undoes_store_and_every_orientation_differs() {
        let up = Scene::new(11, 9, 5).render().unwrap();
        let mut seen = Vec::new();
        for o in 1..=8 {
            let s = store(&up, o).unwrap();
            assert_eq!((s.width, s.height), if o >= 5 { (5, 9) } else { (9, 5) });
            assert_eq!(display(&s, o).unwrap(), up, "orientation {o}");
            assert!(!seen.contains(&s), "orientation {o} repeats another");
            seen.push(s);
        }
        assert!(matches!(store(&up, 0), Err(Error::Orientation(0))));
        assert!(matches!(display(&up, 9), Err(Error::Orientation(9))));
    }

    /// Checked against an independent implementation of the EXIF rules.
    #[test]
    fn matches_the_image_crate() {
        let up = Scene::new(5, 7, 4).render().unwrap();
        for o in 1..=8u8 {
            let s = store(&up, o).unwrap();
            let mut img = image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(s.width, s.height, s.data.clone()).unwrap());
            img.apply_orientation(image::metadata::Orientation::from_exif(o).unwrap());
            let shown = img.to_rgb8();
            assert_eq!((shown.width(), shown.height(), shown.as_raw()), (up.width, up.height, &up.data), "orientation {o}");
        }
    }
}
