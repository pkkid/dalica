//! EXIF orientation (tag 0x0112): what it means for the displayed size, and turning stored pixels
//! upright. Values (TIFF/EXIF): 1 as stored, 2 mirrored left–right, 3 rotated 180°, 4 mirrored
//! top–bottom, 5 transposed, 6 rotated 90° clockwise to display, 7 transverse, 8 rotated 90°
//! counter-clockwise to display. 5–8 swap width and height.

use crate::{Error, Result, Rgba8};

/// An EXIF orientation, 1–8. The default is upright (1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Orientation(u8);

impl Default for Orientation {
    fn default() -> Orientation {
        Orientation::UPRIGHT
    }
}

impl Orientation {
    pub const UPRIGHT: Orientation = Orientation(1);

    /// `None` outside 1–8.
    pub fn new(value: u8) -> Option<Orientation> {
        (1..=8).contains(&value).then_some(Orientation(value))
    }

    pub fn get(self) -> u8 {
        self.0.clamp(1, 8)
    }

    /// Whether displaying swaps width and height.
    pub fn swaps(self) -> bool {
        self.get() >= 5
    }

    /// The displayed size of a `width × height` stored image.
    pub fn display_size(self, width: u32, height: u32) -> (u32, u32) {
        if self.swaps() { (height, width) } else { (width, height) }
    }
}

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

/// Turns stored pixels upright for orientation `o`. Orientation 1 returns the image unchanged.
pub fn apply(img: Rgba8, o: Orientation) -> Result<Rgba8> {
    if o.get() == 1 {
        return Ok(img);
    }
    let (w, h) = o.display_size(img.width, img.height);
    let len = img.data.len();
    if len != img.width as usize * img.height as usize * 4 {
        return Err(Error::Malformed(format!("{} bytes for {} × {} RGBA", len, img.width, img.height)));
    }
    let mut out = vec![0u8; len];
    let stride = img.width as usize;
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = stored_at(o.get(), x, y, w, h);
            let src = (sy as usize * stride + sx as usize) * 4;
            let dst = (y as usize * w as usize + x as usize) * 4;
            if let (Some(s), Some(d)) = (img.data.get(src..src + 4), out.get_mut(dst..dst + 4)) {
                d.copy_from_slice(s);
            }
        }
    }
    Ok(Rgba8 { width: w, height: h, data: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(img: &dalica_scenes::Rgb8) -> Rgba8 {
        Rgba8 { width: img.width, height: img.height, data: img.data.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() }
    }

    /// The same transform as `dalica-scenes` (written independently): storing for `o` and
    /// applying `o` gives the upright picture back.
    #[test]
    fn apply_undoes_what_a_camera_stores() {
        let up = dalica_scenes::Scene::new(4, 11, 6).render().unwrap();
        for o in 1..=8 {
            let stored = dalica_scenes::orient::store(&up, o).unwrap();
            let shown = apply(rgba(&stored), Orientation::new(o).unwrap()).unwrap();
            assert_eq!(shown, rgba(&up), "orientation {o}");
        }
    }

    #[test]
    fn values_outside_one_to_eight_are_not_orientations() {
        assert!(Orientation::new(0).is_none() && Orientation::new(9).is_none());
        assert_eq!(Orientation::new(6).unwrap().display_size(40, 30), (30, 40));
        assert_eq!(Orientation::default(), Orientation::UPRIGHT);
        let bad = Rgba8 { width: 2, height: 2, data: vec![0; 15] };
        assert!(apply(bad, Orientation::new(3).unwrap()).is_err());
    }
}
