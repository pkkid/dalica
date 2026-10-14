//! The pixel steps of the output pipeline that are not rendering: reading the source under the
//! decode limits, the 16-bit path for sources whose pixels only move, flattening alpha for a JPEG
//! and packing the channels the encoder takes.

use dalica_edit::Dihedral;
use dalica_image::color::to8_u16;
use dalica_image::decode::guard;
use dalica_image::encode::Channels;
use dalica_image::{Budget, ColorModel, Decoded, Header, Limits, Rgba8, Samples, formats, read_header};

use super::{Error, Result};

/// The source as the file holds it, with the budget its buffers are charged to.
pub(super) struct Read {
    pub header: Header,
    pub decoded: Decoded,
    pub budget: Budget,
}

/// Reads the header, checks the declared size against the limits before any decoder runs, and
/// decodes the stored pixels (before orientation) inside the panic guard (`formats.md` §5).
pub(super) fn read(bytes: &[u8], limits: &Limits) -> Result<Read> {
    let header = read_header(bytes, limits)?;
    limits.check_size(header.width, header.height, 4)?;
    let mut budget = Budget::new(limits, header.width, header.height);
    let decoder = formats::decoder(header.format);
    let decoded = guard(header.format, || decoder(bytes, &header, &mut budget, limits))?;
    decoded.check()?;
    if (decoded.width, decoded.height) != (header.width, header.height) {
        return Err(Error::Decode(dalica_image::Error::Malformed(format!(
            "decoded {} × {}, header says {} × {}",
            decoded.width, decoded.height, header.width, header.height
        ))));
    }
    Ok(Read { header, decoded, budget })
}

/// Whether the source has transparency: an alpha channel with a sample below fully opaque.
pub(super) fn transparency(d: &Decoded) -> bool {
    if !d.alpha {
        return false;
    }
    let n = d.channels();
    match &d.samples {
        Samples::U8(s) => s.chunks_exact(n).any(|p| p.last().is_some_and(|&a| a < u8::MAX)),
        Samples::U16(s) => s.chunks_exact(n).any(|p| p.last().is_some_and(|&a| a < u16::MAX)),
        // Non-finite alpha reads as 0 (`Samples::F32`), so anything not at least 1.0 counts.
        Samples::F32(s) => s.chunks_exact(n).any(|p| p.last().is_some_and(|&a| a.is_nan() || a < 1.0)),
    }
}

/// 16-bit samples moved by a rotation or mirroring, at their own precision.
pub(super) struct Wide {
    pub width: u32,
    pub height: u32,
    pub channels: Channels,
    pub samples: Vec<u16>,
}

/// The source's 16-bit grey or RGB samples moved by `transform`, pixel for pixel, with their
/// alpha when `alpha` (dropped otherwise: the decoders add an opaque one to every PNG); `None`
/// when the samples are not 16-bit grey or RGB.
pub(super) fn wide(d: &Decoded, transform: Dihedral, alpha: bool, budget: &mut Budget) -> Result<Option<Wide>> {
    let Samples::U16(samples) = &d.samples else { return Ok(None) };
    let colours = match d.model {
        ColorModel::Gray => 1,
        ColorModel::Rgb => 3,
        ColorModel::Cmyk => return Ok(None),
    };
    let alpha = alpha && d.alpha;
    let channels = match (colours, alpha) {
        (1, false) => Channels::Grey,
        (1, true) => Channels::GreyAlpha,
        (_, false) => Channels::Rgb,
        (_, true) => Channels::Rgba,
    };
    let (n_in, n_out) = (d.channels(), channels.count());
    let (width, height) = (d.width, d.height);
    let (out_w, out_h) = transform.display_size(width, height);
    let pixels = width as usize * height as usize;
    let mut out = budget.vec(pixels.saturating_mul(n_out), 0u16)?;
    for y in 0..height {
        for x in 0..width {
            let (nx, ny) = transform.map_pixel(x, y, width, height);
            let from = (y as usize * width as usize + x as usize) * n_in;
            let to = (ny as usize * out_w as usize + nx as usize) * n_out;
            // The colour samples, then the alpha (the last sample) when kept.
            if let (Some(src), Some(dst)) = (samples.get(from..from + n_in), out.get_mut(to..to + n_out)) {
                for (i, slot) in dst.iter_mut().enumerate() {
                    let at = if alpha && i + 1 == n_out { n_in - 1 } else { i };
                    *slot = src.get(at).copied().unwrap_or(u16::MAX);
                }
            }
        }
    }
    Ok(Some(Wide { width: out_w, height: out_h, channels, samples: out }))
}

impl Wide {
    /// The same picture as 8-bit RGBA (for the EXIF thumbnail).
    pub fn rgba8(&self) -> Result<Rgba8> {
        let n = self.channels.count();
        let pixels = self.samples.len() / n.max(1);
        let mut data = try_vec(pixels.saturating_mul(4))?;
        for p in self.samples.chunks_exact(n) {
            let v = |i: usize| p.get(i).copied().map(to8_u16).unwrap_or(u8::MAX);
            let px = match self.channels {
                Channels::Grey => [v(0), v(0), v(0), u8::MAX],
                Channels::GreyAlpha => [v(0), v(0), v(0), v(1)],
                Channels::Rgb => [v(0), v(1), v(2), u8::MAX],
                Channels::Rgba => [v(0), v(1), v(2), v(3)],
            };
            data.extend_from_slice(&px);
        }
        Ok(Rgba8 { width: self.width, height: self.height, data })
    }
}

/// An empty vector with room for `len` items, reserved fallibly.
pub(super) fn try_vec<T>(len: usize) -> Result<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(len).map_err(|_| Error::OutOfMemory)?;
    Ok(v)
}

/// Composites every pixel over white and makes it opaque (a JPEG has no alpha).
pub(super) fn flatten(img: &mut Rgba8) {
    for p in img.data.as_chunks_mut::<4>().0 {
        let a = u32::from(p[3]);
        if a == 255 {
            continue;
        }
        for c in &mut p[..3] {
            *c = ((u32::from(*c) * a + 255 * (255 - a) + 127) / 255) as u8;
        }
        p[3] = 255;
    }
}

/// Whether every pixel is neutral (red = green = blue).
pub(super) fn is_grey(img: &Rgba8) -> bool {
    img.data.as_chunks::<4>().0.iter().all(|[r, g, b, _]| r == g && g == b)
}

/// The channels an 8-bit RGBA picture is written with.
pub(super) fn channels(grey: bool, alpha: bool) -> Channels {
    match (grey, alpha) {
        (true, false) => Channels::Grey,
        (true, true) => Channels::GreyAlpha,
        (false, false) => Channels::Rgb,
        (false, true) => Channels::Rgba,
    }
}

/// The samples of `img` for `channels` (grey takes the red sample, which equals the others).
pub(super) fn pack8(img: &Rgba8, channels: Channels) -> Result<Vec<u8>> {
    let keep: &[usize] = match channels {
        Channels::Grey => &[0],
        Channels::GreyAlpha => &[0, 3],
        Channels::Rgb => &[0, 1, 2],
        Channels::Rgba => &[0, 1, 2, 3],
    };
    let mut out = try_vec(img.data.len() / 4 * keep.len())?;
    for p in img.data.as_chunks::<4>().0 {
        out.extend(keep.iter().filter_map(|&i| p.get(i).copied()));
    }
    Ok(out)
}

/// [`pack8`] widened to 16 bits (v × 257, so 255 is 65535).
pub(super) fn pack16(img: &Rgba8, channels: Channels) -> Result<Vec<u16>> {
    let narrow = pack8(img, channels)?;
    let mut out = try_vec(narrow.len())?;
    out.extend(narrow.iter().map(|&v| u16::from(v) * 257));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_composites_over_white() {
        let mut img = Rgba8 { width: 3, height: 1, data: vec![0, 0, 0, 0, 0, 0, 0, 255, 100, 50, 0, 128] };
        flatten(&mut img);
        assert_eq!(img.data, vec![255, 255, 255, 255, 0, 0, 0, 255, 177, 152, 127, 255]);
    }

    #[test]
    fn packing_keeps_the_asked_channels() {
        let img = Rgba8 { width: 2, height: 1, data: vec![9, 9, 9, 200, 1, 2, 3, 255] };
        assert_eq!(pack8(&img, Channels::GreyAlpha).unwrap(), vec![9, 200, 1, 255]);
        assert_eq!(pack8(&img, Channels::Rgb).unwrap(), vec![9, 9, 9, 1, 2, 3]);
        assert_eq!(pack16(&img, Channels::Grey).unwrap(), vec![9 * 257, 257]);
        assert!(!is_grey(&img));
    }

    #[test]
    fn transparency_needs_a_sample_below_opaque() {
        let d = |alpha, samples| Decoded {
            width: 1,
            height: 2,
            model: ColorModel::Gray,
            alpha,
            samples,
            icc: None,
            orientation: dalica_image::Orientation::default(),
        };
        assert!(!transparency(&d(true, Samples::U8(vec![5, 255, 6, 255]))));
        assert!(transparency(&d(true, Samples::U8(vec![5, 255, 6, 254]))));
        assert!(transparency(&d(true, Samples::U16(vec![5, 0, 6, 65535]))));
        assert!(transparency(&d(true, Samples::F32(vec![0.5, 1.0, 0.5, f32::NAN]))));
        assert!(!transparency(&d(false, Samples::U8(vec![5, 6]))));
    }
}
