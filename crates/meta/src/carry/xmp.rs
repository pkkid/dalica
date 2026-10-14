//! The XMP packet of a rendered copy (`data-model.md` §6): `tiff:Orientation` 1 and the new size
//! where the packet states them, every `exif:GPS*` property cut when the location is removed, and
//! every other byte kept.
//!
//! Only the values of simple properties of the top-level descriptions change, in attribute form
//! (`tiff:Orientation="6"`) or element form (`<tiff:Orientation>6</tiff:Orientation>`): the
//! orientation, `tiff:ImageWidth` / `tiff:ImageLength`, `exif:PixelXDimension` /
//! `exif:PixelYDimension` and `exif:ColorSpace` (sRGB, as the copy's pixels are). A property the
//! packet does not have is not added: the EXIF block states it. Removing the location cuts every
//! top-level property of the EXIF namespace whose name starts with `GPS` (attribute or element,
//! with the white space before it); place names in other namespaces stay. The packet's padding
//! absorbs the change in size where it can, and the result is parsed again before it is used.
//!
//! Provenance: written for Dalica from the XMP Specification Part 1 (ISO 16684-1: RDF forms,
//! the packet wrapper and its padding) and Part 2 (the `tiff:` and `exif:` properties). No other
//! writer's code was consulted.

use std::ops::Range;

use crate::xmp::xml::{self, Doc};
use crate::{Error, Limits, Result};

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub const TIFF: &str = "http://ns.adobe.com/tiff/1.0/";
pub const EXIF: &str = "http://ns.adobe.com/exif/1.0/";

/// The packet of a copy `width` × `height` pixels, oriented as stored, sRGB. `None` for a packet
/// of nothing but white space (there is nothing to carry). Past `max_len` the padding is given up
/// first; `Err(TooMany)` when the packet still does not fit.
pub fn carry(packet: &[u8], width: u32, height: u32, remove_location: bool, max_len: usize, limits: &Limits) -> Result<Option<Vec<u8>>> {
    if xml::is_blank(packet) {
        return Ok(None);
    }
    let values = [
        (TIFF, "Orientation", "1".to_owned()),
        (TIFF, "ImageWidth", width.to_string()),
        (TIFF, "ImageLength", height.to_string()),
        (EXIF, "PixelXDimension", width.to_string()),
        (EXIF, "PixelYDimension", height.to_string()),
        (EXIF, "ColorSpace", "1".to_owned()),
    ];
    let out = edit(packet, &values, remove_location, limits)?.unwrap_or_else(|| packet.to_vec());
    fit(out, max_len, limits).map(Some)
}

/// The packet with `tiff:Orientation` set to `orientation` where it states one; `None` when it
/// does not (nothing to change).
pub fn set_orientation(packet: &[u8], orientation: u16, max_len: usize, limits: &Limits) -> Result<Option<Vec<u8>>> {
    if xml::is_blank(packet) {
        return Ok(None);
    }
    match edit(packet, &[(TIFF, "Orientation", orientation.to_string())], false, limits)? {
        Some(out) => fit(out, max_len, limits).map(Some),
        None => Ok(None),
    }
}

/// Whether a packet holds a top-level `exif:GPS*` property.
pub fn has_gps(packet: &[u8], limits: &Limits) -> Result<bool> {
    if xml::is_blank(packet) {
        return Ok(false);
    }
    let src = text(packet, limits)?;
    let doc = xml::parse(src)?;
    Ok(properties(&doc).iter().any(|p| is_gps(&doc, p)))
}

/// The values of the top-level simple property `ns`:`local`, in document order.
pub fn values(packet: &[u8], ns: &str, local: &str, limits: &Limits) -> Result<Vec<String>> {
    if xml::is_blank(packet) {
        return Ok(Vec::new());
    }
    let src = text(packet, limits)?;
    let doc = xml::parse(src)?;
    let mut out = Vec::new();
    for p in properties(&doc) {
        match p {
            Prop::Attr(el, i) => {
                if let Some(a) = doc.element(el).and_then(|e| e.attrs.get(i))
                    && doc.is(&a.name, ns, local)
                {
                    out.push(a.value.clone());
                }
            }
            Prop::Element(el) => {
                if let Some(e) = doc.element(el)
                    && doc.is(&e.name, ns, local)
                {
                    out.push(e.text.trim().to_owned());
                }
            }
        }
    }
    Ok(out)
}

/// A top-level property: an attribute of a description, or a child element of one.
#[derive(Clone, Copy, Debug)]
enum Prop {
    Attr(usize, usize),
    Element(usize),
}

fn text<'p>(packet: &'p [u8], limits: &Limits) -> Result<&'p str> {
    if packet.len() > limits.max_xmp {
        return Err(Error::TooMany("XMP bytes"));
    }
    std::str::from_utf8(packet).map_err(|_| Error::Malformed("an XMP packet that is not UTF-8".into()))
}

/// Every top-level property of every `rdf:Description` directly under `rdf:RDF`.
fn properties(doc: &Doc<'_>) -> Vec<Prop> {
    let mut out = Vec::new();
    let Some(rdf) = doc.elements.iter().position(|e| doc.is(&e.name, RDF, "RDF")) else { return out };
    let Some(rdf) = doc.element(rdf) else { return out };
    for &d in &rdf.children {
        let Some(desc) = doc.element(d).filter(|e| doc.is(&e.name, RDF, "Description")) else { continue };
        for (i, a) in desc.attrs.iter().enumerate() {
            if !a.is_xmlns && !matches!(doc.ns(&a.name), None | Some(RDF) | Some(xml::XML_NS)) {
                out.push(Prop::Attr(d, i));
            }
        }
        out.extend(desc.children.iter().map(|&c| Prop::Element(c)));
    }
    out
}

fn is_gps(doc: &Doc<'_>, p: &Prop) -> bool {
    let name = match *p {
        Prop::Attr(el, i) => doc.element(el).and_then(|e| e.attrs.get(i)).map(|a| a.name),
        Prop::Element(el) => doc.element(el).map(|e| e.name),
    };
    name.is_some_and(|n| doc.ns(&n) == Some(EXIF) && n.local.starts_with("GPS"))
}

/// The packet with the listed simple properties set where they are present and, when asked, the
/// GPS properties cut. `None` when nothing changes.
fn edit(packet: &[u8], values: &[(&str, &str, String)], remove_gps: bool, limits: &Limits) -> Result<Option<Vec<u8>>> {
    let src = text(packet, limits)?;
    let doc = xml::parse(src)?;
    let bytes = src.as_bytes();
    let mut splices: Vec<(Range<usize>, String)> = Vec::new();
    for p in properties(&doc) {
        if remove_gps && is_gps(&doc, &p) {
            match p {
                Prop::Attr(el, i) => {
                    if let Some(a) = doc.element(el).and_then(|e| e.attrs.get(i)) {
                        splices.push((a.span.clone(), String::new()));
                    }
                }
                Prop::Element(el) => {
                    if let Some(e) = doc.element(el) {
                        splices.push((doc.space_before(e.span.start).start..e.span.end, String::new()));
                    }
                }
            }
            continue;
        }
        match p {
            Prop::Attr(el, i) => {
                let Some(a) = doc.element(el).and_then(|e| e.attrs.get(i)) else { continue };
                let Some((_, _, v)) = values.iter().find(|(ns, local, _)| doc.is(&a.name, ns, local)) else { continue };
                if a.value != *v
                    && let Some(r) = attr_value(bytes, &a.span)
                {
                    splices.push((r, v.clone()));
                }
            }
            Prop::Element(el) => {
                let Some(e) = doc.element(el) else { continue };
                let Some((_, _, v)) = values.iter().find(|(ns, local, _)| doc.is(&e.name, ns, local)) else { continue };
                // A simple value only: no child elements, no `rdf:resource`, not empty.
                let simple = e.children.is_empty() && !e.empty && e.attrs.iter().all(|a| a.is_xmlns || doc.ns(&a.name) != Some(RDF));
                if simple && e.text.trim() != v && e.open_end <= e.close_start {
                    splices.push((e.open_end..e.close_start, v.clone()));
                }
            }
        }
    }
    if splices.is_empty() {
        return Ok(None);
    }
    splices.sort_by_key(|(r, _)| r.start);
    let mut out = String::with_capacity(src.len());
    let mut at = 0usize;
    for (r, with) in &splices {
        if r.start < at {
            continue;
        }
        out.push_str(src.get(at..r.start).unwrap_or_default());
        out.push_str(with);
        at = r.end;
    }
    out.push_str(src.get(at..).unwrap_or_default());
    let out = pad(&doc, src, out);
    // The result must still be a well-formed packet.
    xml::parse(&out)?;
    Ok(Some(out.into_bytes()))
}

/// The bytes of an attribute's value inside its span (`name="value"` or `name='value'`).
fn attr_value(bytes: &[u8], span: &Range<usize>) -> Option<Range<usize>> {
    let s = bytes.get(span.clone())?;
    let eq = s.iter().position(|&b| b == b'=')?;
    let open = eq + 1 + s.get(eq + 1..)?.iter().position(|&b| !xml::is_space(b))?;
    let quote = *s.get(open)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let close = open + 1 + s.get(open + 1..)?.iter().position(|&b| b == quote)?;
    Some(span.start + open + 1..span.start + close)
}

/// `edited` with the padding before `<?xpacket end…?>` grown or shrunk so the packet keeps the
/// length of `src`, as far as the padding allows.
fn pad(doc: &Doc<'_>, src: &str, edited: String) -> String {
    let Some(end) = &doc.packet_end else { return edited };
    let padding = doc.space_before(end.start);
    if padding.is_empty() {
        return edited;
    }
    // Every edit lies inside the root element, before the padding; the tail is unchanged.
    let tail = src.len().saturating_sub(padding.end);
    let Some(pad_end) = edited.len().checked_sub(tail) else { return edited };
    let (head, rest) = edited.split_at(pad_end);
    match edited.len().cmp(&src.len()) {
        std::cmp::Ordering::Greater => {
            let cut = (edited.len() - src.len()).min(padding.len());
            let keep = head.len().saturating_sub(cut);
            [head.get(..keep).unwrap_or(head), rest].concat()
        }
        std::cmp::Ordering::Less => {
            let add = " ".repeat(src.len() - edited.len());
            [head, &add, rest].concat()
        }
        std::cmp::Ordering::Equal => edited,
    }
}

/// `packet` within `max_len`, its padding given up as needed.
fn fit(packet: Vec<u8>, max_len: usize, limits: &Limits) -> Result<Vec<u8>> {
    if packet.len() <= max_len {
        return Ok(packet);
    }
    let src = text(&packet, limits)?;
    let doc = xml::parse(src)?;
    let Some(end) = &doc.packet_end else { return Err(Error::TooMany("XMP bytes")) };
    let padding = doc.space_before(end.start);
    let over = packet.len() - max_len;
    if padding.len() < over {
        return Err(Error::TooMany("XMP bytes"));
    }
    let keep = padding.end - over;
    Ok([packet.get(..keep).unwrap_or_default(), packet.get(padding.end..).unwrap_or_default()].concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKET: &str = concat!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n",
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n",
        " <rdf:Description rdf:about=\"\" xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\" xmlns:exif=\"http://ns.adobe.com/exif/1.0/\"",
        " tiff:Orientation=\"6\" exif:GPSLatitude=\"38,42.5N\">\n",
        "  <exif:PixelXDimension>4000</exif:PixelXDimension>\n",
        "  <exif:GPSLongitude>9,8.2W</exif:GPSLongitude>\n",
        " </rdf:Description>\n",
        "</rdf:RDF></x:xmpmeta>\n",
        "                                        \n",
        "<?xpacket end=\"w\"?>"
    );

    #[test]
    fn values_change_in_place_and_padding_absorbs_them() {
        let l = Limits::default();
        let out = carry(PACKET.as_bytes(), 123_456, 7, true, usize::MAX, &l).unwrap().unwrap();
        let s = std::str::from_utf8(&out).unwrap();
        assert_eq!(out.len(), PACKET.len());
        assert!(s.contains("tiff:Orientation=\"1\""));
        assert!(s.contains("<exif:PixelXDimension>123456</exif:PixelXDimension>"));
        assert!(!s.contains("GPS"));
        assert_eq!(values(&out, TIFF, "Orientation", &l).unwrap(), ["1"]);
        assert!(!has_gps(&out, &l).unwrap());
        assert!(has_gps(PACKET.as_bytes(), &l).unwrap());
    }

    #[test]
    fn attribute_values_are_found_with_either_quote() {
        let b = b" a:b = 'x\"y' ";
        assert_eq!(attr_value(b, &(0..b.len())), Some(8..11));
        assert_eq!(attr_value(b"a:b=x", &(0..5)), None);
    }

    #[test]
    fn fitting_gives_up_padding_only() {
        let l = Limits::default();
        let short = fit(PACKET.as_bytes().to_vec(), PACKET.len() - 10, &l).unwrap();
        assert_eq!(short.len(), PACKET.len() - 10);
        assert!(fit(PACKET.as_bytes().to_vec(), 100, &l).is_err());
    }
}
