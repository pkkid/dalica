//! The XMP merge writer (`xmp-interop.md` §1 principle 1, §2, §3): Dalica's properties go into one
//! `rdf:Description` of its own and everything else in the packet stays byte for byte.
//!
//! - A property this write changes is removed from every other top-level description, in attribute
//!   or element form; those descriptions are otherwise left exactly as they were (one emptied by
//!   the removals stays, empty).
//! - Dalica's description is the top-level description holding nothing but owned properties, in
//!   the element form Dalica writes, and `xmp:MetadataDate` (every Dalica write sets it). It is
//!   rewritten in place; properties it already holds and this write does not change keep their
//!   values. Without one, a new description goes just before `</rdf:RDF>`, indented like its
//!   neighbours.
//! - Clearing a value removes the property. `dc:description` keeps its other languages: only the
//!   `x-default` item is Dalica's.
//! - The place and date mirrors are never the reason for a new packet: a place goes into a packet
//!   that exists, the date mirrors only where the packet already has them (`xmp-interop.md` §2).
//!   A new packet (started for a caption, tags or a rating) gets the place and the date
//!   (`exif:DateTimeOriginal`) in the same merge, so writing the same values again changes nothing.
//! - Text is escaped, and characters XML 1.0 does not allow are dropped (`security.md` §4.4).
//! - The packet wrapper is kept and its padding absorbs the change in size, so a rewrite stays the
//!   same size while the padding lasts. A file without a packet gets a new one with
//!   [`NEW_PADDING`] bytes of padding.
//! - Writing again the values Dalica's description holds changes nothing ([`merge`] returns `None`), and the
//!   same write twice gives the same bytes.
//! - Extended XMP is never written. [`Merged`] reports the packet's size and how much padding could
//!   go, for the APP1 limit ([`JPEG_MAX_PACKET`]).
//!
//! The result is parsed again before it is returned: each property written must read back as
//! requested.

use std::ops::Range;

use super::rdf::{ArrayKind, At, DC, EXIF, PHOTOSHOP, RDF, Rdf, Value, X, XMP};
use super::xml::{self, Doc, is_xml_char};
use super::{owned, parse_coordinate, text_of};
use crate::fields::{DateTaken, Place, XmpOwned, clean_tags, clean_text};
use crate::write::{Changes, PLACE_TOLERANCE};
use crate::{Error, Limits, Result};

/// The largest standard packet one JPEG APP1 segment holds: 65,533 payload bytes less the 29-byte
/// `http://ns.adobe.com/xap/1.0/\0` header.
pub const JPEG_MAX_PACKET: usize = 65_504;

/// The padding a new packet gets (the XMP specification's suggestion), so later rewrites can
/// stay in place.
pub const NEW_PADDING: usize = 2048;

/// The packet id of the XMP specification's wrapper.
const PACKET_ID: &str = "W5M0MpCehiHzreSzNTczkc9d";

/// The properties Dalica owns (`xmp-interop.md` §3), in the order it writes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum P {
    Description,
    Subject,
    Rating,
    Latitude,
    Longitude,
    DateTimeOriginal,
    DateCreated,
    CreateDate,
    MetadataDate,
}

const ALL: [P; 9] =
    [P::Description, P::Subject, P::Rating, P::Latitude, P::Longitude, P::DateTimeOriginal, P::DateCreated, P::CreateDate, P::MetadataDate];

impl P {
    fn ns(self) -> &'static str {
        match self {
            P::Description | P::Subject => DC,
            P::Rating | P::CreateDate | P::MetadataDate => XMP,
            P::Latitude | P::Longitude | P::DateTimeOriginal => EXIF,
            P::DateCreated => PHOTOSHOP,
        }
    }

    fn local(self) -> &'static str {
        match self {
            P::Description => "description",
            P::Subject => "subject",
            P::Rating => "Rating",
            P::Latitude => "GPSLatitude",
            P::Longitude => "GPSLongitude",
            P::DateTimeOriginal => "DateTimeOriginal",
            P::DateCreated => "DateCreated",
            P::CreateDate => "CreateDate",
            P::MetadataDate => "MetadataDate",
        }
    }

    fn of(ns: &str, local: &str) -> Option<P> {
        ALL.into_iter().find(|p| p.ns() == ns && p.local() == local)
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The prefix Dalica declares for each namespace it writes, in declaration order.
const PREFIXES: [(&str, &str); 4] = [(DC, "dc"), (XMP, "xmp"), (EXIF, "exif"), (PHOTOSHOP, "photoshop")];

fn prefix_of(ns: &str) -> &'static str {
    PREFIXES.iter().find(|(n, _)| *n == ns).map_or("", |(_, p)| p)
}

/// A value as Dalica writes it.
#[derive(Clone, Debug, PartialEq)]
enum V {
    /// `rdf:Alt` of (language, text), `x-default` first.
    Alt(Vec<(String, String)>),
    /// `rdf:Bag` of texts.
    Bag(Vec<String>),
    Text(String),
}

/// Dalica's description: one optional value per owned property.
type Model = [Option<V>; 9];

/// A merged packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merged {
    packet: Vec<u8>,
    padding: Range<usize>,
}

impl Merged {
    pub fn bytes(&self) -> &[u8] {
        &self.packet
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.packet
    }

    pub fn len(&self) -> usize {
        self.packet.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packet.is_empty()
    }

    /// Bytes of padding before `<?xpacket end…?>`.
    pub fn padding(&self) -> usize {
        self.padding.len()
    }

    /// The size with all the padding dropped.
    pub fn min_len(&self) -> usize {
        self.packet.len().saturating_sub(self.padding.len())
    }

    /// The packet in at most `max` bytes, dropping as much padding as that takes; `None` when it
    /// does not fit even without padding (the write must then fail; nothing else is dropped).
    pub fn fit(&self, max: usize) -> Option<Vec<u8>> {
        let over = self.packet.len().checked_sub(max).filter(|&o| o > 0);
        let Some(over) = over else { return Some(self.packet.clone()) };
        if over > self.padding.len() {
            return None;
        }
        let cut = self.padding.end - over..self.padding.end;
        Some([self.packet.get(..cut.start)?, self.packet.get(cut.end..)?].concat())
    }
}

/// Merges `changes` (already filtered by the write options) into `packet`, or into a new packet when
/// there is none. `metadata_date` is the time of the write (`xmp:MetadataDate`). `Ok(None)`: the
/// packet needs no change. A packet that is not well-formed XMP is an `Err` and is never replaced.
pub fn merge(packet: Option<&[u8]>, changes: &Changes, metadata_date: &DateTaken, limits: &Limits) -> Result<Option<Merged>> {
    merge_at(packet, changes, Some(metadata_date), limits)
}

/// [`merge`] with the time of the write optional. Without one (callers without a clock: tests and
/// tools; the engine always passes the time), Dalica's description keeps the `xmp:MetadataDate` it
/// has and a new description gets none; such a description, holding only owned properties in
/// Dalica's form, is then taken for Dalica's even without the date, so the same write twice still
/// gives the same bytes.
pub fn merge_at(packet: Option<&[u8]>, changes: &Changes, metadata_date: Option<&DateTaken>, limits: &Limits) -> Result<Option<Merged>> {
    let Some(bytes) = packet.filter(|p| !xml::is_blank(p)) else { return new_packet(changes, metadata_date, limits) };
    let src = text_of(bytes, limits)?;
    let doc = xml::parse(src)?;
    let rdf = Rdf::new(&doc);
    let rdf_el = rdf.rdf.and_then(|i| doc.element(i)).ok_or_else(|| Error::Malformed("XMP: no rdf:RDF element to write into".to_owned()))?;
    if rdf_el.empty {
        return Err(Error::Malformed("XMP: an empty rdf:RDF element".to_owned()));
    }
    let current = owned(&rdf, limits);
    let changed = wanted(changes, Some(&rdf), &current, limits);
    if changed.is_empty() {
        return Ok(None);
    }
    let ours = find_ours(&doc, &rdf, metadata_date.is_some());
    let mut model: Model = ours.as_ref().map(|(_, m)| m.clone()).unwrap_or_default();
    for (p, v) in &changed {
        set(&mut model, *p, v.clone());
    }
    // The same values with the packet's own metadata date (or none, without a clock): if nothing
    // else differs, nothing changes.
    if let Some((_, old)) = &ours {
        set(&mut model, P::MetadataDate, old.get(P::MetadataDate.index()).cloned().flatten());
        if splice(&doc, &rdf, ours.as_ref().map(|(d, _)| *d), &changed, &model)?.packet == bytes {
            return Ok(None);
        }
    }
    if let Some(date) = metadata_date {
        set(&mut model, P::MetadataDate, Some(V::Text(date.to_string())));
    }
    let merged = splice(&doc, &rdf, ours.as_ref().map(|(d, _)| *d), &changed, &model)?;
    check(&merged.packet, &changed, limits)?;
    Ok(Some(merged))
}

fn set(model: &mut Model, p: P, v: Option<V>) {
    if let Some(slot) = model.get_mut(p.index()) {
        *slot = v;
    }
}

/// Text Dalica writes: characters XML 1.0 does not allow dropped.
fn xml_clean(s: &str) -> String {
    s.chars().filter(|&c| is_xml_char(c)).collect()
}

/// The owned properties this write sets (`Some`) or removes (`None`). `rdf` is the packet's RDF, or
/// `None` for a new packet.
fn wanted(changes: &Changes, rdf: Option<&Rdf>, current: &XmpOwned, limits: &Limits) -> Vec<(P, Option<V>)> {
    let present = |p: P| rdf.is_some_and(|r| r.first(p.ns(), p.local()).is_some());
    let mut out = Vec::new();
    if let Some(caption) = &changes.caption {
        let text = caption.as_deref().and_then(|c| clean_text(&xml_clean(c), limits.max_string));
        let others: Vec<(String, String)> = match rdf.and_then(|r| r.first(DC, "description")).map(|p| &p.field.value) {
            Some(Value::Array(_, items)) => items
                .iter()
                .filter_map(|i| {
                    let lang = i.lang.as_deref().filter(|l| !l.eq_ignore_ascii_case("x-default"))?;
                    Some((xml_clean(lang), xml_clean(i.value.text()?)))
                })
                .collect(),
            _ => Vec::new(),
        };
        let mut items: Vec<(String, String)> = text.map(|t| ("x-default".to_owned(), t)).into_iter().collect();
        items.extend(others);
        out.push((P::Description, (!items.is_empty()).then_some(V::Alt(items))));
    }
    if let Some(tags) = &changes.tags {
        let cleaned: Vec<String> = tags.iter().map(|t| xml_clean(t)).collect();
        let tags = clean_tags(cleaned.iter().map(String::as_str), limits.max_string, limits.max_tags);
        out.push((P::Subject, (!tags.is_empty()).then_some(V::Bag(tags))));
    }
    match changes.star {
        Some(true) => out.push((P::Rating, Some(V::Text("5".to_owned())))),
        // Unstarring clears only the rating Dalica sets (`xmp-interop.md` §2).
        Some(false) if current.rating == Some(5) => out.push((P::Rating, Some(V::Text("0".to_owned())))),
        _ => {}
    }
    // The mirrors: into the packet there is, or into a new one that the fields above start.
    match changes.place {
        Some(Some(place)) => {
            out.push((P::Latitude, Some(V::Text(coordinate(place.latitude(), 'N', 'S')))));
            out.push((P::Longitude, Some(V::Text(coordinate(place.longitude(), 'E', 'W')))));
        }
        Some(None) => out.extend([(P::Latitude, None), (P::Longitude, None)]),
        None => {}
    }
    if let Some(date) = &changes.date_taken {
        for p in [P::DateTimeOriginal, P::DateCreated, P::CreateDate] {
            // A new packet gets the first of them; a packet only the ones it has.
            if present(p) || (rdf.is_none() && p == P::DateTimeOriginal) {
                out.push((p, Some(V::Text(date.to_string()))));
            }
        }
    }
    // Removing what is not there changes nothing.
    out.retain(|(p, v)| v.is_some() || present(*p));
    out
}

/// An XMP GPS coordinate, `DDD,MM.mmmmmmmmk`, with trailing zeros of the minutes dropped.
fn coordinate(value: f64, positive: char, negative: char) -> String {
    const STEPS: u64 = 100_000_000; // per minute
    let dir = if value < 0.0 { negative } else { positive };
    let total = (value.abs() * 60.0 * STEPS as f64).round() as u64;
    let (degrees, rest) = (total / (60 * STEPS), total % (60 * STEPS));
    let (minutes, fraction) = (rest / STEPS, rest % STEPS);
    let digits = format!("{fraction:08}");
    let digits = digits.trim_end_matches('0');
    format!("{degrees},{minutes:02}.{}{dir}", if digits.is_empty() { "0" } else { digits })
}

/// Dalica's description, if the packet has one, and the values it holds.
///
/// Its `xmp:MetadataDate` is part of the shape. Without a clock (`dated` false) a description
/// without one also counts, since a write without a time writes none.
fn find_ours(doc: &Doc<'_>, rdf: &Rdf, dated: bool) -> Option<(usize, Model)> {
    rdf.descriptions.iter().rev().find_map(|d| {
        let el = doc.element(d.element)?;
        if el.has_text || el.attrs.iter().any(|a| !a.is_xmlns && !doc.is(&a.name, RDF, "about")) {
            return None;
        }
        let mut model: Model = Default::default();
        for prop in &d.props {
            let At::Element(_) = prop.at else { return None };
            let p = P::of(&prop.field.ns, &prop.field.local)?;
            let v = match (p, &prop.field.value) {
                (P::Description, Value::Array(ArrayKind::Alt, items)) => {
                    V::Alt(items.iter().map(|i| Some((i.lang.clone()?, text_item(&i.value)?))).collect::<Option<_>>()?)
                }
                (P::Subject, Value::Array(ArrayKind::Bag | ArrayKind::Seq, items)) => {
                    V::Bag(items.iter().map(|i| if i.lang.is_none() { text_item(&i.value) } else { None }).collect::<Option<_>>()?)
                }
                (P::Description | P::Subject, _) => return None,
                (_, Value::Text(t)) => V::Text(t.clone()),
                _ => return None,
            };
            let slot = model.get_mut(p.index())?;
            if slot.is_some() {
                return None;
            }
            *slot = Some(v);
        }
        (!dated || model.get(P::MetadataDate.index())?.is_some()).then_some((d.element, model))
    })
}

fn text_item(v: &Value) -> Option<String> {
    match v {
        Value::Text(t) => Some(t.clone()),
        _ => None,
    }
}

/// The packet with the changed properties cut from the other descriptions and Dalica's description
/// written from `model`, the padding absorbing the difference in size.
fn splice(doc: &Doc<'_>, rdf: &Rdf, ours: Option<usize>, changed: &[(P, Option<V>)], model: &Model) -> Result<Merged> {
    let src = doc.src;
    let rdf_index = rdf.rdf.ok_or_else(|| Error::Malformed("XMP: no rdf:RDF element".to_owned()))?;
    let rdf_el = doc.element(rdf_index).ok_or_else(|| Error::Malformed("XMP: no rdf:RDF element".to_owned()))?;
    let cut = |p: P| p == P::MetadataDate || changed.iter().any(|(c, _)| *c == p);
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for d in rdf.descriptions.iter().filter(|d| Some(d.element) != ours) {
        let Some(el) = doc.element(d.element) else { continue };
        for prop in &d.props {
            if !P::of(&prop.field.ns, &prop.field.local).is_some_and(cut) {
                continue;
            }
            let range = match prop.at {
                At::Attr(i) => el.attrs.get(i).map(|a| a.span.clone()),
                At::Element(c) => doc.element(c).map(|c| doc.space_before(c.span.start).start..c.span.end),
            };
            if let Some(r) = range {
                edits.push((r, String::new()));
            }
        }
    }
    // The RDF prefix as the packet binds it (ours redeclares `rdf` when it cannot be reused).
    let bound = rdf_el.name.prefix;
    let reuse = !bound.is_empty() && !matches!(bound, "dc" | "xmp" | "exif" | "photoshop" | "xml" | "xmlns");
    let (r, declare) = if reuse { (bound, false) } else { ("rdf", true) };
    let about = rdf.descriptions.first().and_then(|d| doc.attr(d.element, RDF, "about")).unwrap_or_default();
    match ours.and_then(|o| doc.element(o)) {
        Some(el) => {
            let indent = indent_at(src, doc.space_before(el.span.start));
            edits.push((el.span.clone(), serialize(model, r, declare, about, indent.as_deref())));
        }
        None => {
            let space = doc.space_before(rdf_el.close_start);
            match indent_at(src, space.clone()) {
                Some(close) => {
                    let indent = format!("{close} ");
                    let text = format!("{indent}{}", serialize(model, r, declare, about, Some(&indent)));
                    edits.push((space.start..space.start, text));
                }
                None => edits.push((rdf_el.close_start..rdf_el.close_start, serialize(model, r, declare, about, None))),
            }
        }
    }
    edits.sort_by_key(|(r, _)| (r.start, r.end));
    let mut out = String::with_capacity(src.len() + 1024);
    let mut at = 0;
    for (range, text) in &edits {
        if range.start < at {
            return Err(Error::Malformed("XMP: overlapping properties".to_owned()));
        }
        out.push_str(src.get(at..range.start).ok_or(Error::OutOfRange)?);
        out.push_str(text);
        at = range.end;
    }
    out.push_str(src.get(at..).ok_or(Error::OutOfRange)?);
    let mut packet = out.into_bytes();

    // The padding: the white space before `<?xpacket end…?>`, after every edit.
    let Some(end_pi) = &doc.packet_end else { return Ok(Merged { padding: packet.len()..packet.len(), packet }) };
    let old = doc.space_before(end_pi.start);
    let shift = packet.len() as i128 - src.len() as i128;
    let start = usize::try_from(old.start as i128 + shift).map_err(|_| Error::OutOfRange)?;
    let end = usize::try_from(old.end as i128 + shift).map_err(|_| Error::OutOfRange)?;
    if shift > 0 {
        let drop = usize::try_from(shift).map_err(|_| Error::OutOfRange)?.min(end - start);
        packet.drain(end - drop..end);
        Ok(Merged { padding: start..end - drop, packet })
    } else {
        let add = usize::try_from(-shift).map_err(|_| Error::OutOfRange)?;
        packet.splice(end..end, std::iter::repeat_n(b' ', add));
        Ok(Merged { padding: start..end + add, packet })
    }
}

/// The indentation for an element preceded by `space`: a line break and the white space after it,
/// or `None` when the packet is written without line breaks there.
fn indent_at(src: &str, space: Range<usize>) -> Option<String> {
    let ws = src.get(space)?;
    let line = ws.rfind('\n')?;
    Some(format!("\n{}", ws.get(line + 1..).unwrap_or_default()))
}

fn escape(s: &str, attribute: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            // Kept through the reader's line-end and attribute normalisation.
            '\r' => out.push_str("&#xD;"),
            '"' if attribute => out.push_str("&quot;"),
            '\n' if attribute => out.push_str("&#xA;"),
            '\t' if attribute => out.push_str("&#x9;"),
            c if is_xml_char(c) => out.push(c),
            _ => {}
        }
    }
    out
}

/// Dalica's description. `indent` is the line break and indentation of the description itself
/// (`None`: no white space at all); nested elements are indented one more space per level.
fn serialize(model: &Model, r: &str, declare_rdf: bool, about: &str, indent: Option<&str>) -> String {
    let nl = |level: usize| indent.map_or_else(String::new, |i| format!("{i}{}", " ".repeat(level)));
    let mut s = format!("<{r}:Description {r}:about=\"{}\"", escape(about, true));
    if declare_rdf {
        s.push_str(&format!(" xmlns:rdf=\"{RDF}\""));
    }
    for (ns, prefix) in PREFIXES {
        if ALL.iter().any(|p| p.ns() == ns && model.get(p.index()).is_some_and(Option::is_some)) {
            s.push_str(&format!(" xmlns:{prefix}=\"{ns}\""));
        }
    }
    s.push('>');
    for p in ALL {
        let Some(Some(v)) = model.get(p.index()) else { continue };
        let name = format!("{}:{}", prefix_of(p.ns()), p.local());
        s.push_str(&format!("{}<{name}>", nl(1)));
        match v {
            V::Text(t) => s.push_str(&escape(t, false)),
            V::Alt(items) => {
                s.push_str(&format!("{}<{r}:Alt>", nl(2)));
                for (lang, t) in items {
                    s.push_str(&format!("{}<{r}:li xml:lang=\"{}\">{}</{r}:li>", nl(3), escape(lang, true), escape(t, false)));
                }
                s.push_str(&format!("{}</{r}:Alt>{}", nl(2), nl(1)));
            }
            V::Bag(items) => {
                s.push_str(&format!("{}<{r}:Bag>", nl(2)));
                for t in items {
                    s.push_str(&format!("{}<{r}:li>{}</{r}:li>", nl(3), escape(t, false)));
                }
                s.push_str(&format!("{}</{r}:Bag>{}", nl(2), nl(1)));
            }
        }
        s.push_str(&format!("</{name}>"));
    }
    s.push_str(&format!("{}</{r}:Description>", nl(0)));
    s
}

/// A new packet holding Dalica's properties, started only for a caption, tags or a rating; the place
/// and the date (`exif:DateTimeOriginal`) go in with them, never on their own.
fn new_packet(changes: &Changes, metadata_date: Option<&DateTaken>, limits: &Limits) -> Result<Option<Merged>> {
    let changed = wanted(changes, None, &XmpOwned::default(), limits);
    if !changed.iter().any(|(p, v)| v.is_some() && matches!(p, P::Description | P::Subject | P::Rating)) {
        return Ok(None);
    }
    let mut model: Model = Default::default();
    for (p, v) in &changed {
        set(&mut model, *p, v.clone());
    }
    if let Some(date) = metadata_date {
        set(&mut model, P::MetadataDate, Some(V::Text(date.to_string())));
    }
    let body = serialize(&model, "rdf", false, "", Some("\n  "));
    let padding: String = (0..NEW_PADDING).map(|i| if i % 100 == 99 { '\n' } else { ' ' }).collect();
    let head = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"{PACKET_ID}\"?>\n<x:xmpmeta xmlns:x=\"{X}\">\n <rdf:RDF xmlns:rdf=\"{RDF}\">\n  {body}\n </rdf:RDF>\n</x:xmpmeta>\n"
    );
    let packet = format!("{head}{padding}<?xpacket end=\"w\"?>").into_bytes();
    check(&packet, &changed, limits)?;
    Ok(Some(Merged { padding: head.len()..head.len() + padding.len(), packet }))
}

/// The written packet parsed again: each changed property reads back as requested.
fn check(packet: &[u8], changed: &[(P, Option<V>)], limits: &Limits) -> Result<()> {
    let doc = xml::parse(text_of(packet, limits)?).map_err(|e| Error::Verify(format!("the written XMP does not parse: {e}")))?;
    let got = owned(&Rdf::new(&doc), limits);
    let text = |v: &Option<V>| match v {
        Some(V::Text(t)) => Some(t.clone()),
        _ => None,
    };
    let date = |v: &Option<V>| text(v).and_then(|t| DateTaken::parse_iso(&t));
    let place = |la: Option<f64>, lo: Option<f64>| la.zip(lo).and_then(|(a, b)| Place::new(a, b));
    let want_lat = changed.iter().find(|(p, _)| *p == P::Latitude).map(|(_, v)| text(v).and_then(|t| parse_coordinate(&t, true)));
    let want_lon = changed.iter().find(|(p, _)| *p == P::Longitude).map(|(_, v)| text(v).and_then(|t| parse_coordinate(&t, false)));
    let ok = changed.iter().all(|(p, v)| match p {
        P::Description => {
            let want = match v {
                Some(V::Alt(items)) => items.iter().find(|(l, _)| l == "x-default").map(|(_, t)| t.clone()),
                _ => None,
            };
            got.caption == want
        }
        P::Subject => got.tags == if let Some(V::Bag(t)) = v { t.clone() } else { Vec::new() },
        P::Rating => got.rating == text(v).and_then(|t| super::parse_rating(&t)),
        P::Latitude | P::Longitude => match (want_lat.flatten(), want_lon.flatten()) {
            (Some(a), Some(b)) => place(Some(a), Some(b)).is_some_and(|w| got.place.is_some_and(|g| g.close_to(&w, PLACE_TOLERANCE))),
            _ => got.place.is_none(),
        },
        P::DateTimeOriginal => got.date_time_original == date(v),
        P::DateCreated => got.date_created == date(v),
        P::CreateDate => got.create_date == date(v),
        P::MetadataDate => true,
    });
    if ok { Ok(()) } else { Err(Error::Verify("an XMP property does not read back as written".to_owned())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_round_trip() {
        assert_eq!(coordinate(-33.875, 'N', 'S'), "33,52.5S");
        assert_eq!(coordinate(151.0, 'E', 'W'), "151,00.0E");
        assert_eq!(coordinate(-0.000_000_01, 'E', 'W'), "0,00.0000006W");
        // Rounding up to a whole degree carries.
        assert_eq!(coordinate(12.999_999_999_999, 'N', 'S'), "13,00.0N");
        for v in [0.0, -90.0, 90.0, -180.0, 179.999_999_9, -12.345_678_9, 0.000_001] {
            let back = parse_coordinate(&coordinate(v, 'N', 'S'), true).unwrap();
            assert!((back - v).abs() < 1e-9, "{v} → {back}");
        }
    }

    #[test]
    fn escaping() {
        assert_eq!(escape("a&<>\"\r\n\t\u{1}\u{fffe}b", false), "a&amp;&lt;&gt;\"&#xD;\n\tb");
        assert_eq!(escape("a\"\n\t", true), "a&quot;&#xA;&#x9;");
    }

    #[test]
    fn fitting_drops_padding_only() {
        let m = Merged { packet: b"<a/>    <?x?>".to_vec(), padding: 4..8 };
        assert_eq!(m.fit(100).unwrap(), m.packet);
        assert_eq!(m.fit(11).unwrap(), b"<a/>  <?x?>");
        assert_eq!(m.fit(9).unwrap(), b"<a/><?x?>");
        assert_eq!(m.fit(8), None);
        assert_eq!((m.len(), m.min_len(), m.padding()), (13, 9, 4));
    }
}
