//! XMP: reading the owned properties of a packet, listing every property, and the merge writer
//! (`xmp-interop.md` §1–§3).
//!
//! - [`xml`]: our own bounded XML tokenizer that keeps byte spans (`security.md` §4.2: no
//!   `DOCTYPE` or entities, depth ≤ [`MAX_DEPTH`], ≤ [`MAX_NODES`] nodes).
//! - `rdf`: the top-level descriptions and their properties in attribute and element form, with
//!   `rdf:Alt` (`x-default` and the other languages), `rdf:Bag`, `rdf:Seq` and structures.
//! - [`read`]: the owned properties ([`XmpOwned`]); [`rows`]: Show All Metadata rows.
//! - [`regions`]: MWG face regions read and written (`xmp-interop.md` §6).
//! - [`merge()`]: writes Dalica's properties into a packet (or a new one), keeping everything else
//!   byte for byte.
//!
//! Text from a packet is tainted (`security.md` §4.4): captions and tags pass through
//! [`clean_text`] and [`clean_tags`], numbers and dates are validated.

pub mod merge;
mod rdf;
pub mod regions;
pub mod xml;

pub use merge::{JPEG_MAX_PACKET, Merged, NEW_PADDING, merge};
pub use xml::{MAX_DEPTH, MAX_NODES};

use self::rdf::{ArrayKind, DC, EXIF, PHOTOSHOP, Rdf, Value, XMP, label};
use crate::fields::{DateTaken, Place, XmpOwned, clean_tags, clean_text};
use crate::listing::Row;
use crate::{Error, Limits, Result};

/// The owned properties of a standard packet and, for JPEG, its reassembled extended part (which
/// only fills what the standard packet leaves empty). A packet of nothing but white space holds
/// nothing; one that is not well-formed XMP is an `Err`.
pub fn read(packet: &[u8], extended: Option<&[u8]>, limits: &Limits) -> Result<XmpOwned> {
    let mut out = read_one(packet, limits)?;
    if let Some(ext) = extended {
        let more = read_one(ext, limits)?;
        out.caption = out.caption.or(more.caption);
        if out.tags.is_empty() {
            out.tags = more.tags;
        }
        out.rating = out.rating.or(more.rating);
        out.place = out.place.or(more.place);
        out.date_time_original = out.date_time_original.or(more.date_time_original);
        out.date_created = out.date_created.or(more.date_created);
        out.create_date = out.create_date.or(more.create_date);
        if out.faces.is_empty() {
            out.faces = more.faces;
        }
    }
    Ok(out)
}

fn read_one(bytes: &[u8], limits: &Limits) -> Result<XmpOwned> {
    if xml::is_blank(bytes) {
        return Ok(XmpOwned::default());
    }
    let src = text_of(bytes, limits)?;
    let doc = xml::parse(src)?;
    Ok(owned(&Rdf::new(&doc), limits))
}

/// A packet as UTF-8 text, within [`Limits::max_xmp`].
fn text_of<'b>(bytes: &'b [u8], limits: &Limits) -> Result<&'b str> {
    if bytes.len() > limits.max_xmp {
        return Err(Error::TooMany("XMP bytes"));
    }
    std::str::from_utf8(bytes).map_err(|_| Error::Malformed("XMP: not UTF-8".to_owned()))
}

/// The owned properties of a parsed packet: the first top-level occurrence of each wins.
fn owned(rdf: &Rdf, limits: &Limits) -> XmpOwned {
    let text = |ns: &str, local: &str| rdf.first(ns, local).and_then(|p| p.field.value.text().map(str::to_owned));
    let date = |ns: &str, local: &str| text(ns, local).and_then(|t| DateTaken::parse_iso(&t));
    let latitude = text(EXIF, "GPSLatitude").and_then(|t| parse_coordinate(&t, true));
    let longitude = text(EXIF, "GPSLongitude").and_then(|t| parse_coordinate(&t, false));
    XmpOwned {
        caption: rdf.first(DC, "description").and_then(|p| caption_of(&p.field.value)).and_then(|c| clean_text(c, limits.max_string)),
        tags: rdf.first(DC, "subject").map(|p| tags_of(&p.field.value, limits)).unwrap_or_default(),
        rating: text(XMP, "Rating").and_then(|t| parse_rating(&t)),
        place: latitude.zip(longitude).and_then(|(la, lo)| Place::new(la, lo)),
        date_time_original: date(EXIF, "DateTimeOriginal"),
        date_created: date(PHOTOSHOP, "DateCreated"),
        create_date: date(XMP, "CreateDate"),
        faces: regions::read(rdf, limits),
    }
}

/// The caption of a `dc:description`: its `x-default` alternative, else an item with no language
/// (seen in the wild), else a simple value. Items in other languages are never the caption.
fn caption_of(v: &Value) -> Option<&str> {
    match v {
        Value::Array(_, items) => items
            .iter()
            .find(|i| i.lang.as_deref().is_some_and(|l| l.eq_ignore_ascii_case("x-default")))
            .or_else(|| items.iter().find(|i| i.lang.is_none()))
            .and_then(|i| i.value.text()),
        other => other.text(),
    }
}

/// The tags of a `dc:subject`: every item of the array (or a simple value as one tag).
fn tags_of(v: &Value, limits: &Limits) -> Vec<String> {
    match v {
        Value::Array(_, items) => clean_tags(items.iter().filter_map(|i| i.value.text()), limits.max_string, limits.max_tags),
        other => clean_tags(other.text(), limits.max_string, limits.max_tags),
    }
}

/// `xmp:Rating`: a whole number from −1 (rejected) to 5, written as an integer or a real (`"3.0"`).
pub fn parse_rating(text: &str) -> Option<i8> {
    let r = text.trim().parse::<f64>().ok()?;
    (r.is_finite() && r.fract() == 0.0 && (-1.0..=5.0).contains(&r)).then_some(r as i8)
}

/// An XMP GPS coordinate (`DDD,MM,SSk` or `DDD,MM.mmk`, `k` one of N/S for a latitude and E/W for a
/// longitude; a bare signed decimal is accepted too) as signed decimal degrees.
pub fn parse_coordinate(text: &str, latitude: bool) -> Option<f64> {
    let s = text.trim();
    let last = s.chars().last()?;
    let (body, sign) = match (last.to_ascii_uppercase(), latitude) {
        ('N', true) | ('E', false) => (s.get(..s.len() - 1)?, 1.0),
        ('S', true) | ('W', false) => (s.get(..s.len() - 1)?, -1.0),
        (c, _) if c.is_ascii_digit() || c == '.' => {
            return s.parse::<f64>().ok().filter(|v| v.is_finite());
        }
        _ => return None,
    };
    let mut parts = body.splitn(4, ',').map(|p| p.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0));
    let degrees = parts.next()??;
    let minutes = parts.next().unwrap_or(Some(0.0))?;
    let seconds = parts.next().unwrap_or(Some(0.0))?;
    if parts.next().is_some() || minutes >= 60.0 || seconds >= 60.0 {
        return None;
    }
    Some(sign * (degrees + minutes / 60.0 + seconds / 3600.0))
}

/// Every property of a packet as listing rows (group "XMP"; the extended part "XMP extended"),
/// at most [`Limits::max_rows`] + 1 of them (the listing marks itself truncated past the limit).
pub fn rows(packet: &[u8], extended: Option<&[u8]>, limits: &Limits) -> Result<Vec<Row>> {
    let mut out = Vec::new();
    rows_one(packet, "XMP", limits, &mut out)?;
    if let Some(ext) = extended {
        rows_one(ext, "XMP extended", limits, &mut out)?;
    }
    Ok(out)
}

fn rows_one(bytes: &[u8], group: &str, limits: &Limits, out: &mut Vec<Row>) -> Result<()> {
    if xml::is_blank(bytes) {
        return Ok(());
    }
    let doc = xml::parse(text_of(bytes, limits)?)?;
    let rdf = Rdf::new(&doc);
    let cap = limits.max_rows.saturating_add(1);
    for p in rdf.descriptions.iter().flat_map(|d| d.props.iter()) {
        push_rows(group, label(&p.field), &p.field.value, cap, out);
    }
    Ok(())
}

/// How many items of a simple array one row shows before "…".
const MAX_ITEMS: usize = 64;

fn push_rows(group: &str, label_text: String, v: &Value, cap: usize, out: &mut Vec<Row>) {
    if out.len() >= cap {
        return;
    }
    let mut row = |label: String, value: String| {
        if out.len() < cap {
            out.push(Row { group: group.to_owned(), label, value });
        }
    };
    match v {
        Value::Text(t) => row(label_text, t.clone()),
        Value::Array(ArrayKind::Alt, items) if items.iter().all(|i| matches!(i.value, Value::Text(_))) => {
            for i in items {
                let l = i.lang.as_deref().map_or_else(|| label_text.clone(), |lang| format!("{label_text} [{lang}]"));
                row(l, i.value.text().unwrap_or_default().to_owned());
            }
        }
        Value::Array(_, items) if items.iter().all(|i| matches!(i.value, Value::Text(_))) => {
            let mut shown: Vec<&str> = items.iter().take(MAX_ITEMS).filter_map(|i| i.value.text()).collect();
            if items.len() > MAX_ITEMS {
                shown.push("…");
            }
            row(label_text, shown.join("; "));
        }
        Value::Array(_, items) => {
            for (n, i) in items.iter().enumerate() {
                push_rows(group, format!("{label_text}[{}]", n + 1), &i.value, cap, out);
            }
        }
        Value::Struct(fields) => {
            for f in fields {
                push_rows(group, format!("{label_text}/{}", label(f)), &f.value, cap, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratings() {
        assert_eq!(parse_rating("5"), Some(5));
        assert_eq!(parse_rating(" 3.0 "), Some(3));
        assert_eq!(parse_rating("-1"), Some(-1));
        for bad in ["6", "-2", "2.5", "NaN", "inf", "", "five"] {
            assert_eq!(parse_rating(bad), None, "{bad}");
        }
    }

    #[test]
    fn coordinates() {
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        assert!(close(parse_coordinate("33,52.5S", true), -33.875));
        assert!(close(parse_coordinate("151,12,36E", false), 151.21));
        assert!(close(parse_coordinate(" 0,30W ", false), -0.5));
        assert!(close(parse_coordinate("-12.25", true), -12.25));
        assert!(close(parse_coordinate("12.5n", true), 12.5));
        for bad in ["33,52.5E", "1,60.0N", "1,2,60N", "1,2,3,4N", "-1,2N", "N", "", "abc", "1,xN"] {
            assert_eq!(parse_coordinate(bad, true), None, "{bad}");
        }
        assert_eq!(parse_coordinate("12,5N", false), None);
    }
}
