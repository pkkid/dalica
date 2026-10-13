//! EXIF: the tags Dalica reads (Exif 2.32, TIFF 6.0), the IFDs they live in, and the summary the
//! Properties pane shows. Reading is in [`read`]; editing in place in [`edit`].
//!
//! Provenance: tag numbers and value meanings from the public Exif 2.32 (CIPA DC-008) and TIFF 6.0
//! specifications, and the XMP Part 3 / IPTC-NAA tag numbers for TIFF (700, 33723, 34377). No
//! other reader's code was consulted.

pub mod edit;
pub mod read;

use std::fmt;

use crate::fields::DateTaken;

// IFD0.
pub const TAG_IMAGE_DESCRIPTION: u16 = 0x010e;
pub const TAG_MAKE: u16 = 0x010f;
pub const TAG_MODEL: u16 = 0x0110;
pub const TAG_ORIENTATION: u16 = 0x0112;
pub const TAG_SOFTWARE: u16 = 0x0131;
pub const TAG_DATE_TIME: u16 = 0x0132;
pub const TAG_ARTIST: u16 = 0x013b;
pub const TAG_XMP: u16 = 0x02bc;
pub const TAG_COPYRIGHT: u16 = 0x8298;
pub const TAG_IPTC: u16 = 0x83bb;
pub const TAG_PHOTOSHOP: u16 = 0x8649;
pub const TAG_EXIF_IFD: u16 = 0x8769;
pub const TAG_ICC: u16 = 0x8773;
pub const TAG_GPS_IFD: u16 = 0x8825;
// The EXIF IFD.
pub const TAG_EXPOSURE_TIME: u16 = 0x829a;
pub const TAG_F_NUMBER: u16 = 0x829d;
pub const TAG_ISO: u16 = 0x8827;
pub const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
pub const TAG_DATE_TIME_DIGITIZED: u16 = 0x9004;
pub const TAG_OFFSET_TIME: u16 = 0x9010;
pub const TAG_OFFSET_TIME_ORIGINAL: u16 = 0x9011;
pub const TAG_OFFSET_TIME_DIGITIZED: u16 = 0x9012;
pub const TAG_FLASH: u16 = 0x9209;
pub const TAG_FOCAL_LENGTH: u16 = 0x920a;
pub const TAG_MAKER_NOTE: u16 = 0x927c;
pub const TAG_USER_COMMENT: u16 = 0x9286;
pub const TAG_SUB_SEC_TIME: u16 = 0x9290;
pub const TAG_SUB_SEC_TIME_ORIGINAL: u16 = 0x9291;
pub const TAG_SUB_SEC_TIME_DIGITIZED: u16 = 0x9292;
pub const TAG_COLOR_SPACE: u16 = 0xa001;
pub const TAG_INTEROP_IFD: u16 = 0xa005;
pub const TAG_FOCAL_LENGTH_35: u16 = 0xa405;
pub const TAG_LENS_MAKE: u16 = 0xa433;
pub const TAG_LENS_MODEL: u16 = 0xa434;
// The Interoperability IFD.
pub const TAG_INTEROP_INDEX: u16 = 0x0001;
// The GPS IFD.
pub const TAG_GPS_VERSION: u16 = 0x0000;
pub const TAG_GPS_LATITUDE_REF: u16 = 0x0001;
pub const TAG_GPS_LATITUDE: u16 = 0x0002;
pub const TAG_GPS_LONGITUDE_REF: u16 = 0x0003;
pub const TAG_GPS_LONGITUDE: u16 = 0x0004;

/// Which IFD of an EXIF structure an entry belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IfdKind {
    /// IFD0: the main image.
    Image,
    /// IFD1: the embedded thumbnail.
    Thumbnail,
    Exif,
    Gps,
    Interop,
}

impl IfdKind {
    /// The listing group of this IFD's rows.
    pub fn group(self) -> &'static str {
        match self {
            IfdKind::Image => "EXIF",
            IfdKind::Thumbnail => "EXIF thumbnail",
            IfdKind::Exif => "EXIF",
            IfdKind::Gps => "GPS",
            IfdKind::Interop => "EXIF interoperability",
        }
    }
}

/// The name of a tag in an IFD, for the listing.
pub fn tag_name(ifd: IfdKind, tag: u16) -> Option<&'static str> {
    Some(match (ifd, tag) {
        (IfdKind::Gps, t) => match t {
            0x0000 => "GPS version",
            0x0001 => "Latitude reference",
            0x0002 => "Latitude",
            0x0003 => "Longitude reference",
            0x0004 => "Longitude",
            0x0005 => "Altitude reference",
            0x0006 => "Altitude",
            0x0007 => "GPS time (UTC)",
            0x0008 => "Satellites",
            0x0009 => "GPS status",
            0x000a => "Measure mode",
            0x000b => "Precision (DOP)",
            0x000c => "Speed unit",
            0x000d => "Speed",
            0x000e => "Track reference",
            0x000f => "Track",
            0x0010 => "Image direction reference",
            0x0011 => "Image direction",
            0x0012 => "Map datum",
            0x001b => "Processing method",
            0x001d => "GPS date",
            0x001f => "Horizontal error",
            _ => return None,
        },
        (IfdKind::Interop, 0x0001) => "Interoperability index",
        (IfdKind::Interop, 0x0002) => "Interoperability version",
        (IfdKind::Interop, _) => return None,
        (_, t) => match t {
            0x00fe => "New subfile type",
            0x0100 => "Image width",
            0x0101 => "Image height",
            0x0102 => "Bits per sample",
            0x0103 => "Compression",
            0x0106 => "Photometric interpretation",
            0x010e => "Image description",
            0x010f => "Make",
            0x0110 => "Model",
            0x0111 => "Strip offsets",
            0x0112 => "Orientation",
            0x0115 => "Samples per pixel",
            0x0116 => "Rows per strip",
            0x0117 => "Strip byte counts",
            0x011a => "X resolution",
            0x011b => "Y resolution",
            0x011c => "Planar configuration",
            0x0128 => "Resolution unit",
            0x0131 => "Software",
            0x0132 => "Date and time",
            0x013b => "Artist",
            0x013e => "White point",
            0x013f => "Primary chromaticities",
            0x0201 => "Thumbnail offset",
            0x0202 => "Thumbnail length",
            0x0211 => "YCbCr coefficients",
            0x0213 => "YCbCr positioning",
            0x0214 => "Reference black and white",
            0x02bc => "XMP packet",
            0x8298 => "Copyright",
            0x829a => "Exposure time",
            0x829d => "F-number",
            0x83bb => "IPTC block",
            0x8649 => "Photoshop resources",
            0x8769 => "EXIF IFD pointer",
            0x8773 => "ICC profile",
            0x8822 => "Exposure program",
            0x8824 => "Spectral sensitivity",
            0x8825 => "GPS IFD pointer",
            0x8827 => "ISO",
            0x8830 => "Sensitivity type",
            0x8832 => "Recommended exposure index",
            0x9000 => "EXIF version",
            0x9003 => "Date taken",
            0x9004 => "Date digitized",
            0x9010 => "Time zone offset",
            0x9011 => "Time zone offset (taken)",
            0x9012 => "Time zone offset (digitized)",
            0x9101 => "Components configuration",
            0x9102 => "Compressed bits per pixel",
            0x9201 => "Shutter speed (APEX)",
            0x9202 => "Aperture (APEX)",
            0x9203 => "Brightness (APEX)",
            0x9204 => "Exposure bias",
            0x9205 => "Maximum aperture (APEX)",
            0x9206 => "Subject distance",
            0x9207 => "Metering mode",
            0x9208 => "Light source",
            0x9209 => "Flash",
            0x920a => "Focal length",
            0x9214 => "Subject area",
            0x927c => "Maker note",
            0x9286 => "User comment",
            0x9290 => "Subseconds",
            0x9291 => "Subseconds (taken)",
            0x9292 => "Subseconds (digitized)",
            0xa000 => "FlashPix version",
            0xa001 => "Color space",
            0xa002 => "Pixel width",
            0xa003 => "Pixel height",
            0xa005 => "Interoperability IFD pointer",
            0xa20e => "Focal plane X resolution",
            0xa20f => "Focal plane Y resolution",
            0xa210 => "Focal plane resolution unit",
            0xa215 => "Exposure index",
            0xa217 => "Sensing method",
            0xa300 => "File source",
            0xa301 => "Scene type",
            0xa302 => "CFA pattern",
            0xa401 => "Custom rendered",
            0xa402 => "Exposure mode",
            0xa403 => "White balance",
            0xa404 => "Digital zoom ratio",
            0xa405 => "Focal length (35 mm)",
            0xa406 => "Scene capture type",
            0xa407 => "Gain control",
            0xa408 => "Contrast",
            0xa409 => "Saturation",
            0xa40a => "Sharpness",
            0xa40c => "Subject distance range",
            0xa420 => "Image unique ID",
            0xa430 => "Camera owner",
            0xa431 => "Body serial number",
            0xa432 => "Lens specification",
            0xa433 => "Lens make",
            0xa434 => "Lens model",
            0xa435 => "Lens serial number",
            _ => return None,
        },
    })
}

/// An unsigned rational with a non-zero denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rational {
    pub num: u32,
    pub den: u32,
}

impl Rational {
    /// `None` for a zero denominator, which EXIF does not allow.
    pub fn new(num: u32, den: u32) -> Option<Rational> {
        (den != 0).then_some(Rational { num, den })
    }

    pub fn value(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }
}

/// The colour space EXIF declares (ColorSpace, refined by the interoperability index "R03").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    Srgb,
    AdobeRgb,
    Uncalibrated,
}

/// The read-only facts of the Properties pane (`ui-layout.md` §7) that EXIF holds. Absent or
/// unreadable values are `None`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub exposure_time: Option<Rational>,
    pub f_number: Option<Rational>,
    pub iso: Option<u32>,
    pub focal_length: Option<Rational>,
    pub focal_length_35: Option<u32>,
    /// The Flash tag's bits (bit 0: fired).
    pub flash: Option<u16>,
    /// 1–8.
    pub orientation: Option<u8>,
    pub color_space: Option<ColorSpace>,
    pub date_digitized: Option<DateTaken>,
}

impl Summary {
    /// Make and model, without the make repeated when the model already starts with it.
    pub fn camera(&self) -> Option<String> {
        match (self.make.as_deref(), self.model.as_deref()) {
            (Some(make), Some(model)) if model.to_lowercase().starts_with(&make.to_lowercase()) => Some(model.to_owned()),
            (Some(make), Some(model)) => Some(format!("{make} {model}")),
            (one, other) => one.or(other).map(str::to_owned),
        }
    }

    /// `1/250 s`, `2 s`, `0.5 s`.
    pub fn exposure(&self) -> Option<String> {
        let t = self.exposure_time?;
        let v = t.value();
        Some(if v > 0.0 && v < 1.0 {
            let inverse = 1.0 / v;
            if (inverse - inverse.round()).abs() < 0.05 { format!("1/{} s", inverse.round()) } else { format!("{v:.3} s") }
        } else {
            format!("{} s", trim(v, 1))
        })
    }

    /// `f/2.8`.
    pub fn aperture(&self) -> Option<String> {
        Some(format!("f/{}", trim(self.f_number?.value(), 1)))
    }

    /// `35 mm (52 mm in 35 mm film)`.
    pub fn focal(&self) -> Option<String> {
        let f = self.focal_length.map(|f| format!("{} mm", trim(f.value(), 1)));
        match (f, self.focal_length_35.filter(|&v| v > 0)) {
            (Some(f), Some(e)) => Some(format!("{f} ({e} mm in 35 mm film)")),
            (Some(f), None) => Some(f),
            (None, Some(e)) => Some(format!("{e} mm in 35 mm film")),
            (None, None) => None,
        }
    }

    /// "Fired" or "Did not fire".
    pub fn flash_text(&self) -> Option<&'static str> {
        self.flash.map(|f| if f & 1 == 1 { "Fired" } else { "Did not fire" })
    }

    pub fn orientation_text(&self) -> Option<&'static str> {
        Some(match self.orientation? {
            1 => "Normal",
            2 => "Mirrored",
            3 => "Rotated 180°",
            4 => "Mirrored vertically",
            5 => "Mirrored, rotated 90° counter-clockwise",
            6 => "Rotated 90° clockwise",
            7 => "Mirrored, rotated 90° clockwise",
            8 => "Rotated 90° counter-clockwise",
            _ => return None,
        })
    }

    pub fn color_space_text(&self) -> Option<&'static str> {
        self.color_space.map(|c| match c {
            ColorSpace::Srgb => "sRGB",
            ColorSpace::AdobeRgb => "Adobe RGB",
            ColorSpace::Uncalibrated => "Uncalibrated",
        })
    }

    /// The Properties rows this summary fills, in the pane's order (`ui-layout.md` §7), each with
    /// a value.
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        let rows = [
            ("Date digitized", self.date_digitized.as_ref().map(ToString::to_string)),
            ("Camera", self.camera()),
            ("Lens", self.lens.clone()),
            ("Exposure", self.exposure()),
            ("Aperture", self.aperture()),
            ("ISO", self.iso.map(|i| i.to_string())),
            ("Focal length", self.focal()),
            ("Flash", self.flash_text().map(str::to_owned)),
            ("Orientation", self.orientation_text().map(str::to_owned)),
            ("Color space", self.color_space_text().map(str::to_owned)),
        ];
        rows.into_iter().filter_map(|(label, v)| Some((label, v?))).collect()
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

/// A number with at most `places` decimals and no trailing zeros.
fn trim(v: f64, places: usize) -> String {
    let s = format!("{v:.places$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_texts() {
        let r = |n, d| Rational::new(n, d);
        let s = Summary {
            make: Some("Canon".into()),
            model: Some("Canon EOS 5D".into()),
            exposure_time: r(1, 250),
            f_number: r(28, 10),
            iso: Some(400),
            focal_length: r(35, 1),
            focal_length_35: Some(52),
            flash: Some(0x10),
            orientation: Some(6),
            color_space: Some(ColorSpace::Srgb),
            ..Summary::default()
        };
        assert_eq!(s.camera().as_deref(), Some("Canon EOS 5D"));
        assert_eq!(s.exposure().as_deref(), Some("1/250 s"));
        assert_eq!(s.aperture().as_deref(), Some("f/2.8"));
        assert_eq!(s.focal().as_deref(), Some("35 mm (52 mm in 35 mm film)"));
        assert_eq!(s.flash_text(), Some("Did not fire"));
        assert_eq!(
            s.rows().iter().map(|r| r.0).collect::<Vec<_>>(),
            ["Camera", "Exposure", "Aperture", "ISO", "Focal length", "Flash", "Orientation", "Color space"]
        );
        let t = Summary { make: Some("NIKON".into()), model: Some("D750".into()), exposure_time: r(2, 1), f_number: r(8, 1), ..Summary::default() };
        assert_eq!(t.camera().as_deref(), Some("NIKON D750"));
        assert_eq!(t.exposure().as_deref(), Some("2 s"));
        assert_eq!(t.aperture().as_deref(), Some("f/8"));
        assert_eq!(Summary { exposure_time: r(3, 10), ..Summary::default() }.exposure().as_deref(), Some("0.300 s"));
        assert_eq!(Rational::new(1, 0), None);
    }
}
