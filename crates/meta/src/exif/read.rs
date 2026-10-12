//! Reading an EXIF structure through `dalica_image`'s bounded IFD walker: the owned fields
//! (ImageDescription as a caption unless it is a camera placeholder, the GPS place, the date taken
//! with its offset and fraction) and the Properties summary.
//!
//! Strict, like the walker: an IFD loop, an IFD or a value that the walker finds outside the block,
//! or a count past the limits is an `Err` for the whole block. A value that is merely invalid (a
//! zero denominator, a date that is not a date, a latitude past 90°) is left out.

use dalica_image::ifd::{Entry, Ifd, Tiff, Visited, types};

use super::*;
use crate::fields::{DateTaken, ExifOwned, Place, clean_text};
use crate::{Error, Limits, Result};

/// The IFDs of one EXIF structure, each with its kind.
#[derive(Debug)]
pub struct Walked<'a> {
    pub tiff: Tiff<'a>,
    pub ifds: Vec<(IfdKind, Ifd)>,
}

impl Walked<'_> {
    /// The entry for `tag` in the first IFD of `kind`.
    pub fn get(&self, kind: IfdKind, tag: u16) -> Option<&Entry> {
        self.ifds.iter().find(|(k, _)| *k == kind).and_then(|(_, ifd)| ifd.get(tag))
    }
}

/// What an EXIF block says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExifRead {
    pub owned: ExifOwned,
    pub summary: Summary,
}

/// Walks IFD0, the EXIF, GPS and Interoperability IFDs, and the next-IFD chain (IFD1, the
/// thumbnail, and any after it), within the limits. Accepts the raw-file header variants too.
pub fn walk<'a>(block: &'a [u8], limits: &Limits) -> Result<Walked<'a>> {
    let tiff = Tiff::new_raw(block)?;
    let il = limits.ifd();
    let mut visited = Visited::default();
    let ifd0 = tiff.ifd(tiff.first_ifd()?, &mut visited, &il)?;
    let mut out = Walked { tiff, ifds: Vec::new() };
    let pointer = |ifd: &Ifd, tag| ifd.get(tag).and_then(|e| tiff.uint(e)).filter(|&o| o != 0);
    let deeper = |depth: usize| if depth > limits.max_ifd_depth { Err(Error::TooMany("IFD depth")) } else { Ok(()) };
    if let Some(at) = pointer(&ifd0, TAG_EXIF_IFD) {
        deeper(2)?;
        let exif = tiff.ifd(at, &mut visited, &il)?;
        let interop = pointer(&exif, TAG_INTEROP_IFD);
        out.ifds.push((IfdKind::Exif, exif));
        if let Some(at) = interop {
            deeper(3)?;
            out.ifds.push((IfdKind::Interop, tiff.ifd(at, &mut visited, &il)?));
        }
    }
    if let Some(at) = pointer(&ifd0, TAG_GPS_IFD) {
        deeper(2)?;
        out.ifds.push((IfdKind::Gps, tiff.ifd(at, &mut visited, &il)?));
    }
    // The next-IFD chain: IFD1 is the thumbnail; later ones are only checked (loops, offsets).
    let mut next = ifd0.next;
    let mut first = true;
    while next != 0 {
        let ifd = tiff.ifd(next, &mut visited, &il)?;
        next = ifd.next;
        if first {
            out.ifds.push((IfdKind::Thumbnail, ifd));
            first = false;
        }
    }
    out.ifds.insert(0, (IfdKind::Image, ifd0));
    Ok(out)
}

/// Reads the owned fields and the summary from an EXIF block (a TIFF structure from its header on).
pub fn read(block: &[u8], limits: &Limits) -> Result<ExifRead> {
    let w = walk(block, limits)?;
    let t = &w.tiff;
    let max = limits.max_string;
    let text = |kind, tag| -> Result<Option<String>> { w.get(kind, tag).map_or(Ok(None), |e| entry_text(t, e, max)) };
    let rational =
        |kind, tag| -> Result<Option<Rational>> { w.get(kind, tag).map_or(Ok(None), |e| Ok(rationals(t, e, 1)?.first().copied().flatten())) };
    let uint = |kind, tag| w.get(kind, tag).and_then(|e| t.uint(e));

    let make = text(IfdKind::Image, TAG_MAKE)?;
    let model = text(IfdKind::Image, TAG_MODEL)?;
    let description = text(IfdKind::Image, TAG_IMAGE_DESCRIPTION)?;
    let caption = description.filter(|d| !is_placeholder(d, make.as_deref(), model.as_deref()));
    let date = |date_tag, offset_tag, subsec_tag| -> Result<Option<DateTaken>> {
        let Some(d) = text(IfdKind::Exif, date_tag)? else { return Ok(None) };
        let offset = text(IfdKind::Exif, offset_tag)?;
        let subsec = text(IfdKind::Exif, subsec_tag)?;
        Ok(DateTaken::from_exif(&d, offset.as_deref(), subsec.as_deref()))
    };
    let date_taken = date(TAG_DATE_TIME_ORIGINAL, TAG_OFFSET_TIME_ORIGINAL, TAG_SUB_SEC_TIME_ORIGINAL)?;
    let date_digitized = date(TAG_DATE_TIME_DIGITIZED, TAG_OFFSET_TIME_DIGITIZED, TAG_SUB_SEC_TIME_DIGITIZED)?;
    let place = gps(&w, max)?;

    let lens = match (text(IfdKind::Exif, TAG_LENS_MAKE)?, text(IfdKind::Exif, TAG_LENS_MODEL)?) {
        (Some(make), Some(model)) if !model.to_lowercase().starts_with(&make.to_lowercase()) => Some(format!("{make} {model}")),
        (_, Some(model)) => Some(model),
        (make, None) => make,
    };
    let color_space = match uint(IfdKind::Exif, TAG_COLOR_SPACE) {
        Some(1) => Some(ColorSpace::Srgb),
        Some(2) => Some(ColorSpace::AdobeRgb),
        Some(0xffff) => match text(IfdKind::Interop, TAG_INTEROP_INDEX)?.as_deref() {
            Some("R03") => Some(ColorSpace::AdobeRgb),
            _ => Some(ColorSpace::Uncalibrated),
        },
        _ => None,
    };
    let summary = Summary {
        make,
        model,
        lens,
        exposure_time: rational(IfdKind::Exif, TAG_EXPOSURE_TIME)?,
        f_number: rational(IfdKind::Exif, TAG_F_NUMBER)?,
        iso: uint(IfdKind::Exif, TAG_ISO).filter(|&i| i > 0),
        focal_length: rational(IfdKind::Exif, TAG_FOCAL_LENGTH)?,
        focal_length_35: uint(IfdKind::Exif, TAG_FOCAL_LENGTH_35),
        flash: uint(IfdKind::Exif, TAG_FLASH).and_then(|f| u16::try_from(f).ok()),
        orientation: uint(IfdKind::Image, TAG_ORIENTATION).and_then(|o| u8::try_from(o).ok()).filter(|o| (1..=8).contains(o)),
        color_space,
        date_digitized,
    };
    Ok(ExifRead { owned: ExifOwned { caption, place, date_taken }, summary })
}

/// The GPS IFD's latitude and longitude, when both are there and valid. A missing reference is
/// taken as north or east.
fn gps(w: &Walked<'_>, max: usize) -> Result<Option<Place>> {
    let t = &w.tiff;
    let coordinate = |value_tag, ref_tag, negative: char| -> Result<Option<f64>> {
        let Some(e) = w.get(IfdKind::Gps, value_tag) else { return Ok(None) };
        let parts = rationals(t, e, 3)?;
        let Some(v) = degrees(&parts) else { return Ok(None) };
        let reference = w.get(IfdKind::Gps, ref_tag).map_or(Ok(None), |e| entry_text(t, e, max))?;
        let south_or_west = reference.is_some_and(|r| r.trim().eq_ignore_ascii_case(&negative.to_string()));
        Ok(Some(if south_or_west { -v } else { v }))
    };
    let lat = coordinate(TAG_GPS_LATITUDE, TAG_GPS_LATITUDE_REF, 'S')?;
    let lon = coordinate(TAG_GPS_LONGITUDE, TAG_GPS_LONGITUDE_REF, 'W')?;
    Ok(match (lat, lon) {
        (Some(lat), Some(lon)) => Place::new(lat, lon),
        _ => None,
    })
}

/// Degrees from one to three rationals (degrees, minutes, seconds); `None` if any is invalid.
fn degrees(parts: &[Option<Rational>]) -> Option<f64> {
    if parts.is_empty() {
        return None;
    }
    let mut v = 0.0;
    for (part, scale) in parts.iter().zip([1.0, 60.0, 3600.0]) {
        v += part.as_ref()?.value() / scale;
    }
    Some(v)
}

/// The first `max` values of a RATIONAL entry, each `None` where the denominator is zero. Other
/// types give no values. The whole value must lie inside the block.
pub fn rationals(t: &Tiff<'_>, e: &Entry, max: usize) -> Result<Vec<Option<Rational>>> {
    if e.kind != types::RATIONAL {
        return Ok(Vec::new());
    }
    let Some(value) = t.value(e)? else { return Ok(Vec::new()) };
    let little = t.is_little_endian();
    Ok(value
        .as_chunks::<8>()
        .0
        .iter()
        .take(max)
        .map(|&[a, b, c, d, e, f, g, h]| Rational::new(word([a, b, c, d], little), word([e, f, g, h], little)))
        .collect())
}

/// A four-byte word in the structure's byte order.
pub fn word(b: [u8; 4], little: bool) -> u32 {
    if little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
}

/// The text of an ASCII (or BYTE/UNDEFINED) entry up to its first NUL, UTF-8 when valid, else
/// Latin-1, cleaned and at most `max` bytes. The whole value must lie inside the block.
pub fn entry_text(t: &Tiff<'_>, e: &Entry, max: usize) -> Result<Option<String>> {
    if !matches!(e.kind, types::ASCII | types::BYTE | types::UNDEFINED) {
        return Ok(None);
    }
    let Some(value) = t.value(e)? else { return Ok(None) };
    let bytes = value.split(|&b| b == 0).next().unwrap_or_default();
    let bytes = bytes.get(..bytes.len().min(max)).unwrap_or_default();
    Ok(clean_text(&crate::container::text_utf8_or_latin1(bytes), max))
}

/// Descriptions cameras write when the user wrote none.
const PLACEHOLDERS: &[&str] = &[
    "OLYMPUS DIGITAL CAMERA",
    "SONY DSC",
    "DIGITAL CAMERA",
    "DIGITAL STILL CAMERA",
    "KONICA MINOLTA DIGITAL CAMERA",
    "MINOLTA DIGITAL CAMERA",
    "LEICA DIGITAL CAMERA",
    "PENTAX DIGITAL CAMERA",
    "SAMSUNG DIGITAL CAMERA",
    "KODAK DIGITAL STILL CAMERA",
    "SAMSUNG",
    "EXIF_JPEG_PICTURE",
    "DEFAULT",
    "IMAGE",
    "PICTURE",
    "PHOTO",
    "UNTITLED",
    "CAMERA",
];

/// Whether an ImageDescription is a camera's placeholder rather than a caption: a known
/// placeholder, the make or model, or the make followed by "DIGITAL CAMERA" (case and surrounding
/// space ignored).
pub fn is_placeholder(description: &str, make: Option<&str>, model: Option<&str>) -> bool {
    let d = description.trim().to_uppercase();
    if d.is_empty() || PLACEHOLDERS.contains(&d.as_str()) {
        return true;
    }
    [make, model].into_iter().flatten().map(|m| m.trim().to_uppercase()).any(|m| !m.is_empty() && (d == m || d == format!("{m} DIGITAL CAMERA")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders() {
        assert!(is_placeholder("OLYMPUS DIGITAL CAMERA         ", None, None));
        assert!(is_placeholder("sony dsc", None, None));
        assert!(is_placeholder("NIKON", Some("NIKON"), None));
        assert!(is_placeholder("Acme digital camera", Some("ACME"), Some("X1")));
        assert!(!is_placeholder("Sunset over the bay", Some("NIKON"), Some("D750")));
        assert!(!is_placeholder("Olympus digital camera club outing", None, None));
    }

    #[test]
    fn degree_parts() {
        let r = |n, d| Rational::new(n, d);
        assert_eq!(degrees(&[r(33, 1), r(30, 1), r(0, 1)]), Some(33.5));
        assert_eq!(degrees(&[r(33, 1), r(30, 1)]), Some(33.5));
        assert_eq!(degrees(&[r(33, 1), None, r(0, 1)]), None);
        assert_eq!(degrees(&[]), None);
    }
}
