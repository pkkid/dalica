//! The field model: the values Dalica owns in a file (`xmp-interop.md` §2–§3), each source's
//! values before precedence, and the precedence rules that pick one.
//!
//! Text from a file is tainted (`security.md` §4.4): every reader passes it through
//! [`clean_text`] or [`clean_tag`], which drop control characters, trim it and cap its length.
//! Numbers are validated: a place is finite and on the globe, a date is a real calendar date, an
//! offset is within ±18 hours.

use std::fmt;

use dalica_core::{Date, DateTime, Rect01};

/// A position on the globe, in decimal degrees (north and east positive).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    latitude: f64,
    longitude: f64,
}

impl Place {
    /// `None` unless both are finite, the latitude within ±90 and the longitude within ±180.
    pub fn new(latitude: f64, longitude: f64) -> Option<Place> {
        let ok = latitude.is_finite() && longitude.is_finite() && latitude.abs() <= 90.0 && longitude.abs() <= 180.0;
        ok.then_some(Place { latitude, longitude })
    }

    pub fn latitude(&self) -> f64 {
        self.latitude
    }

    pub fn longitude(&self) -> f64 {
        self.longitude
    }

    /// Whether two places are the same to within `degrees` on each axis (written places come back
    /// as degree/minute/second rationals, so an exact comparison is too strict).
    pub fn close_to(&self, other: &Place, degrees: f64) -> bool {
        (self.latitude - other.latitude).abs() <= degrees && (self.longitude - other.longitude).abs() <= degrees
    }
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (ns, ew) = (if self.latitude < 0.0 { 'S' } else { 'N' }, if self.longitude < 0.0 { 'W' } else { 'E' });
        write!(f, "{:.6}° {ns}, {:.6}° {ew}", self.latitude.abs(), self.longitude.abs())
    }
}

/// The most an offset from UTC may be, in minutes (ISO 8601 allows ±18:00).
const MAX_OFFSET_MINUTES: i16 = 18 * 60;

/// When a photo was taken: the local date and time, and when the file records them, its offset
/// from UTC and the fraction of a second (as the digits written, so `"050"` stays `"050"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateTaken {
    local: DateTime,
    offset_minutes: Option<i16>,
    subsec: Option<String>,
}

impl DateTaken {
    /// A date with no offset and no fraction.
    pub fn local(local: DateTime) -> DateTaken {
        DateTaken { local, offset_minutes: None, subsec: None }
    }

    /// `None` if the offset is past ±18:00 or the fraction is not 1–9 ASCII digits.
    pub fn new(local: DateTime, offset_minutes: Option<i16>, subsec: Option<&str>) -> Option<DateTaken> {
        if offset_minutes.is_some_and(|m| m.abs() > MAX_OFFSET_MINUTES) {
            return None;
        }
        let subsec = match subsec {
            Some(s) if !(1..=9).contains(&s.len()) || !s.bytes().all(|b| b.is_ascii_digit()) => return None,
            other => other.map(str::to_owned),
        };
        Some(DateTaken { local, offset_minutes, subsec })
    }

    /// From EXIF's three fields (`DateTimeOriginal`, `OffsetTimeOriginal`, `SubSecTimeOriginal`):
    /// a blank or malformed offset or fraction is left out, a malformed date gives `None`.
    pub fn from_exif(date: &str, offset: Option<&str>, subsec: Option<&str>) -> Option<DateTaken> {
        let local = date.trim().parse::<DateTime>().ok()?;
        let offset = offset.and_then(parse_offset);
        let subsec = subsec.map(|s| s.trim_matches([' ', '\0'])).filter(|s| (1..=9).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit()));
        DateTaken::new(local, offset, subsec)
    }

    /// From an ISO 8601 date as XMP writes them: `YYYY`, `YYYY-MM`, `YYYY-MM-DD`, then optionally
    /// `Thh:mm`, `:ss`, `.s…` and `Z` or `±hh:mm`. Missing parts are the first month, day or
    /// minute.
    pub fn parse_iso(text: &str) -> Option<DateTaken> {
        let s = text.trim();
        let (date, time) = match s.split_once('T') {
            Some((d, t)) => (d, Some(t)),
            None => (s, None),
        };
        let mut parts = date.split('-');
        let year = digits(parts.next()?, 4)?;
        let month = parts.next().map_or(Some(1), |m| digits(m, 2))?;
        let day = parts.next().map_or(Some(1), |d| digits(d, 2))?;
        if parts.next().is_some() {
            return None;
        }
        let date = Date::new(u16::try_from(year).ok()?, u8::try_from(month).ok()?, u8::try_from(day).ok()?).ok()?;
        let Some(time) = time else { return DateTaken::new(DateTime::new(date, 0, 0, 0).ok()?, None, None) };
        // Split off the zone designator.
        let (clock, offset) = if let Some(c) = time.strip_suffix('Z') {
            (c, Some(0))
        } else if let Some(at) = time.rfind(['+', '-']) {
            (time.get(..at)?, Some(parse_offset(time.get(at..)?)?))
        } else {
            (time, None)
        };
        let (clock, subsec) = match clock.split_once('.') {
            Some((c, f)) => (c, Some(f)),
            None => (clock, None),
        };
        let mut hms = clock.split(':');
        let hour = digits(hms.next()?, 2)?;
        let minute = digits(hms.next()?, 2)?;
        let second = hms.next().map_or(Some(0), |x| digits(x, 2))?;
        if hms.next().is_some() {
            return None;
        }
        let local = DateTime::new(date, u8::try_from(hour).ok()?, u8::try_from(minute).ok()?, u8::try_from(second).ok()?).ok()?;
        DateTaken::new(local, offset, subsec)
    }

    pub fn date_time(&self) -> DateTime {
        self.local
    }

    /// Minutes east of UTC, when the file says.
    pub fn offset_minutes(&self) -> Option<i16> {
        self.offset_minutes
    }

    /// The fraction of a second as written (`"123"` is 0.123 s).
    pub fn subsec(&self) -> Option<&str> {
        self.subsec.as_deref()
    }

    /// The offset as EXIF and ISO 8601 write it: `+09:00`, `-03:30`.
    pub fn offset_text(&self) -> Option<String> {
        self.offset_minutes.map(format_offset)
    }
}

impl fmt::Display for DateTaken {
    /// ISO 8601: `2026-06-04T19:42:10.123+09:00`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.local)?;
        if let Some(s) = &self.subsec {
            write!(f, ".{s}")?;
        }
        if let Some(o) = self.offset_minutes {
            write!(f, "{}", format_offset(o))?;
        }
        Ok(())
    }
}

/// Exactly `len` ASCII digits as a number.
fn digits(s: &str, len: usize) -> Option<u32> {
    (s.len() == len && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

/// `+HH:MM`, `-HH:MM`, `+HHMM` or `+HH` as minutes east of UTC, within ±18:00.
pub fn parse_offset(text: &str) -> Option<i16> {
    let s = text.trim_matches([' ', '\0']);
    let (sign, rest) = match s.as_bytes().first()? {
        b'+' => (1i16, s.get(1..)?),
        b'-' => (-1i16, s.get(1..)?),
        _ => return None,
    };
    let (h, m) = match rest.len() {
        2 => (rest, "00"),
        4 => (rest.get(..2)?, rest.get(2..)?),
        5 if rest.as_bytes().get(2) == Some(&b':') => (rest.get(..2)?, rest.get(3..)?),
        _ => return None,
    };
    let (h, m) = (i16::try_from(digits(h, 2)?).ok()?, i16::try_from(digits(m, 2)?).ok()?);
    let minutes = h.checked_mul(60)?.checked_add(m)?;
    (m < 60 && minutes <= MAX_OFFSET_MINUTES).then_some(sign * minutes)
}

/// Minutes east of UTC as `+HH:MM`.
pub fn format_offset(minutes: i16) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let m = minutes.unsigned_abs();
    format!("{sign}{:02}:{:02}", m / 60, m % 60)
}

/// Tainted text made safe to keep: NUL and other control characters (tab and line breaks
/// excepted) and the noncharacters U+FFFE and U+FFFF dropped, surrounding white space trimmed, at
/// most `max` bytes (cut at a character boundary). `None` when nothing is left.
///
/// What is left is text XML 1.0 allows, so a value written into XMP reads back as it was cleaned
/// here (the write verification compares the two).
pub fn clean_text(text: &str, max: usize) -> Option<String> {
    let kept: String =
        text.chars().filter(|&c| (!c.is_control() || matches!(c, '\t' | '\n' | '\r')) && !matches!(c, '\u{fffe}' | '\u{ffff}')).collect();
    let trimmed = kept.trim();
    let mut end = trimmed.len().min(max);
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    let out = trimmed.get(..end).unwrap_or_default().trim_end();
    (!out.is_empty()).then(|| out.to_owned())
}

/// A tag made safe to keep: as [`clean_text`], with tabs and line breaks turned into spaces.
pub fn clean_tag(text: &str, max: usize) -> Option<String> {
    let flat: String = text.chars().map(|c| if matches!(c, '\t' | '\n' | '\r') { ' ' } else { c }).collect();
    clean_text(&flat, max)
}

/// Tags cleaned, with case-insensitive duplicates removed (the first spelling kept), at most `max`.
pub fn clean_tags<'a>(tags: impl IntoIterator<Item = &'a str>, max_len: usize, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        if out.len() >= max {
            break;
        }
        if let Some(t) = clean_tag(tag, max_len)
            && !out.iter().any(|o| o.to_lowercase() == t.to_lowercase())
        {
            out.push(t);
        }
    }
    out
}

/// Which source a photo's date taken came from (`xmp-interop.md` §2, in precedence order). With
/// none of them the caller falls back to the file's time and flags it "date from file".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateSource {
    /// EXIF DateTimeOriginal (with OffsetTimeOriginal and SubSecTimeOriginal).
    Exif,
    /// XMP `exif:DateTimeOriginal`.
    XmpDateTimeOriginal,
    /// XMP `photoshop:DateCreated`.
    XmpDateCreated,
    /// XMP `xmp:CreateDate`.
    XmpCreateDate,
    /// IPTC 2:55 DateCreated with 2:60 TimeCreated.
    Iptc,
}

/// The values Dalica owns in a file, after precedence.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OwnedFields {
    pub caption: Option<String>,
    pub tags: Vec<String>,
    /// The file's rating, −1 (rejected) to 5. Only 5 makes a photo starred.
    pub rating: Option<i8>,
    pub place: Option<Place>,
    pub date_taken: Option<DateTaken>,
    pub date_source: Option<DateSource>,
    /// The face regions of the file (`xmp-interop.md` §6), in the upright frame, in file order.
    pub faces: Vec<FaceRegion>,
}

impl OwnedFields {
    /// A rating of 5 imports as Dalica's star (`xmp-interop.md` §2).
    pub fn starred(&self) -> bool {
        self.rating == Some(5)
    }
}

/// The owned fields an EXIF block holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExifOwned {
    /// ImageDescription, unless it is a known camera placeholder.
    pub caption: Option<String>,
    /// The GPS IFD's latitude and longitude.
    pub place: Option<Place>,
    /// DateTimeOriginal with its offset and fraction.
    pub date_taken: Option<DateTaken>,
}

/// The owned fields an IPTC block holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IptcOwned {
    /// 2:120 Caption-Abstract.
    pub caption: Option<String>,
    /// Every 2:25 Keywords dataset.
    pub keywords: Vec<String>,
    /// 2:55 DateCreated with 2:60 TimeCreated.
    pub date_created: Option<DateTaken>,
}

/// The owned fields an XMP packet holds (read by M6.9).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XmpOwned {
    /// `dc:description`, the `x-default` alternative.
    pub caption: Option<String>,
    /// `dc:subject`.
    pub tags: Vec<String>,
    /// `xmp:Rating`.
    pub rating: Option<i8>,
    /// `exif:GPSLatitude` / `exif:GPSLongitude`.
    pub place: Option<Place>,
    /// `exif:DateTimeOriginal`.
    pub date_time_original: Option<DateTaken>,
    /// `photoshop:DateCreated`.
    pub date_created: Option<DateTaken>,
    /// `xmp:CreateDate`.
    pub create_date: Option<DateTaken>,
    /// The `Face` entries of `mwg-rs:Regions` that hold a usable box (at most
    /// [`MAX_FACES`](crate::xmp::regions::MAX_FACES)).
    pub faces: Vec<FaceRegion>,
}

/// A face region of a file (`xmp-interop.md` §6): its box on the photo after its EXIF orientation
/// (the stored frame of `data-model.md` §3.3), clipped to the photo, and the name given to it.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceRegion {
    /// `mwg-rs:Name`, cleaned ([`clean_text`]); `None` for an unnamed face.
    pub name: Option<String>,
    pub rect: Rect01,
}

/// Each source's values before precedence.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sources {
    pub exif: ExifOwned,
    pub iptc: IptcOwned,
    pub xmp: XmpOwned,
}

impl Sources {
    /// The precedence rules of `xmp-interop.md` §2, the first source with a value winning: caption
    /// XMP → IPTC → EXIF; tags XMP → IPTC; rating XMP only; place EXIF → XMP; date taken EXIF →
    /// XMP `exif:DateTimeOriginal` → `photoshop:DateCreated` → `xmp:CreateDate` → IPTC; faces
    /// XMP only.
    pub fn resolve(&self) -> OwnedFields {
        let (exif, iptc, xmp) = (&self.exif, &self.iptc, &self.xmp);
        let tags = if xmp.tags.is_empty() { iptc.keywords.clone() } else { xmp.tags.clone() };
        let dates = [
            (&exif.date_taken, DateSource::Exif),
            (&xmp.date_time_original, DateSource::XmpDateTimeOriginal),
            (&xmp.date_created, DateSource::XmpDateCreated),
            (&xmp.create_date, DateSource::XmpCreateDate),
            (&iptc.date_created, DateSource::Iptc),
        ];
        let date = dates.into_iter().find_map(|(d, source)| d.clone().map(|d| (d, source)));
        OwnedFields {
            caption: xmp.caption.clone().or_else(|| iptc.caption.clone()).or_else(|| exif.caption.clone()),
            tags,
            rating: xmp.rating,
            place: exif.place.or(xmp.place),
            date_source: date.as_ref().map(|(_, s)| *s),
            date_taken: date.map(|(d, _)| d),
            faces: xmp.faces.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets() {
        assert_eq!(parse_offset("+09:00"), Some(540));
        assert_eq!(parse_offset("-03:30"), Some(-210));
        assert_eq!(parse_offset("+0530"), Some(330));
        assert_eq!(parse_offset("-08"), Some(-480));
        assert_eq!(parse_offset("+18:00"), Some(1080));
        for bad in ["+18:01", "09:00", "+9:00", "+09:60", "", "+", "+  :  ", "   :  "] {
            assert_eq!(parse_offset(bad), None, "{bad:?}");
        }
        assert_eq!(format_offset(-210), "-03:30");
        assert_eq!(format_offset(0), "+00:00");
    }

    #[test]
    fn iso_dates() {
        let d = DateTaken::parse_iso("2026-06-04T19:42:10.123+09:00").unwrap();
        assert_eq!(d.to_string(), "2026-06-04T19:42:10.123+09:00");
        assert_eq!(d.offset_minutes(), Some(540));
        assert_eq!(DateTaken::parse_iso("2026-06-04T19:42Z").unwrap().to_string(), "2026-06-04T19:42:00+00:00");
        assert_eq!(DateTaken::parse_iso("2026-06").unwrap().to_string(), "2026-06-01T00:00:00");
        assert_eq!(DateTaken::parse_iso("2026").unwrap().to_string(), "2026-01-01T00:00:00");
        for bad in ["", "26-06-04", "2026-13-01", "2026-02-30", "2026-06-04T25:00", "2026-06-04T10:00:00.12a", "2026-06-04-01"] {
            assert_eq!(DateTaken::parse_iso(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn exif_dates() {
        let d = DateTaken::from_exif("2026:06:04 19:42:10", Some("-03:00"), Some("050 ")).unwrap();
        assert_eq!(d.to_string(), "2026-06-04T19:42:10.050-03:00");
        // Blank optional fields are left out, a blank date is no date.
        assert_eq!(DateTaken::from_exif("2026:06:04 19:42:10", Some("   :  "), Some("    ")).unwrap().to_string(), "2026-06-04T19:42:10");
        assert_eq!(DateTaken::from_exif("    :  :     :  :  ", None, None), None);
        assert_eq!(DateTaken::from_exif("0000:00:00 00:00:00", None, None), None);
        assert!(DateTaken::new(d.date_time(), Some(2000), None).is_none());
        assert!(DateTaken::new(d.date_time(), None, Some("1234567890")).is_none());
    }

    #[test]
    fn text_cleaning() {
        assert_eq!(clean_text("  a\0b\u{1b}c\n d  ", 100).as_deref(), Some("abc\n d"));
        assert_eq!(clean_text(" \0 ", 100), None);
        // Cut at a character boundary.
        assert_eq!(clean_text("aé", 2).as_deref(), Some("a"));
        // The two noncharacters XML 1.0 forbids go too (so an XMP write reads back as cleaned);
        // the other noncharacters and private-use characters stay.
        assert_eq!(clean_text("a\u{fffe}b\u{ffff}c\u{fdd0}\u{e000}", 100).as_deref(), Some("abc\u{fdd0}\u{e000}"));
        assert_eq!(clean_text("\u{fffe}\u{ffff}", 100), None);
        assert_eq!(clean_tag("x\u{ffff}y", 100).as_deref(), Some("xy"));
        assert_eq!(clean_tag("beach\nday", 100).as_deref(), Some("beach day"));
        assert_eq!(clean_tags(["Beach", "beach", " ", "Sun", "BEACH"], 100, 10), vec!["Beach", "Sun"]);
        assert_eq!(clean_tags(["a", "b", "c"], 100, 2), vec!["a", "b"]);
    }

    #[test]
    fn places() {
        assert!(Place::new(90.0, -180.0).is_some());
        assert!(Place::new(90.1, 0.0).is_none() && Place::new(0.0, f64::NAN).is_none() && Place::new(f64::INFINITY, 0.0).is_none());
        let p = Place::new(-33.5, 151.25).unwrap();
        assert_eq!(p.to_string(), "33.500000° S, 151.250000° E");
        assert!(p.close_to(&Place::new(-33.5000001, 151.25).unwrap(), 1e-6));
    }
}
