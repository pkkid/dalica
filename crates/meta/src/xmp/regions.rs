//! MWG face regions (`xmp-interop.md` §6): the `Face` entries of `mwg-rs:Regions` read as boxes
//! on the upright photo, and the edits that write Dalica's named faces back into a packet.
//!
//! **Reading.** The first top-level `mwg-rs:Regions` counts. Each `Face` entry of its
//! `mwg-rs:RegionList` gives a box: `stArea:x`/`y` its centre and `w`/`h` its size, normalised to
//! the photo, or in pixels (`stArea:unit="pixel"`) divided by `mwg-rs:AppliedToDimensions` (an entry
//! in pixels without them is skipped). A `mwg-rs:Rotation` of −π/2, +π/2 or ±π says the box was
//! given in the sensor's frame (the convention LightCraft's reader documents for Lightroom
//! Classic: −π/2 for EXIF orientation 6, +π/2 for 8, π for 3) and is turned back into the upright
//! frame; any other angle is taken as written, as are mirrored photos (the schema cannot say
//! "mirrored"). Boxes are clipped to the photo; one that misses it, or has a size that is not a
//! positive finite number, is skipped and the others are kept. Entries of other types are ignored.
//! At most [`MAX_FACES`] faces are kept (`data-model.md` §2: 256 per photo).
//!
//! **Writing** ([`plan`]): the regions are edited where they are, so every byte Dalica does not own
//! stays as it was:
//! - each of Dalica's named faces takes the free readable `Face` entry that overlaps it enough
//!   (IoU ≥ [`MATCH_IOU`]), one with its name first, then the closest; a matched entry keeps every other field byte for
//!   byte and gets Dalica's name and box only where they differ (the box written in the entry's
//!   own unit and rotation, so its `mwg-rs:Rotation` keeps its meaning);
//! - readable `Face` entries with no counterpart are removed; `Face` entries Dalica cannot read
//!   (no usable box) were never imported and are kept, as are entries of every other type;
//! - Dalica's faces with no entry are appended (`mwg-rs:Name`, `mwg-rs:Type`, a normalised
//!   `mwg-rs:Area`), and `mwg-rs:AppliedToDimensions` is added when missing (an existing one is kept:
//!   other entries in pixels depend on it);
//! - a packet without regions gets a new, self-contained `mwg-rs:Regions` (every namespace it uses
//!   declared on it) in Dalica's own description.
//!
//! Inserted elements reuse the prefixes bound where they go and declare the ones that are not.
//! Writing the same faces again changes nothing: names and boxes are compared before anything is
//! written (boxes to within [`AREA_TOLERANCE`]).

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::ops::Range;

use dalica_core::Rect01;

use super::rdf::{At, Field, MWG_RS, RDF, Rdf, ST_AREA, ST_DIM, Value, value_of};
use super::xml::{Doc, is_xml_char};
use crate::fields::{FaceRegion, clean_tag};
use crate::write::NamedFaces;
use crate::{Error, Limits, Result};

/// The most faces read from, or written into, one packet (`data-model.md` §2).
pub const MAX_FACES: usize = 256;

/// How far apart, in normalised units on each edge, two boxes may be and still count as the same
/// (a box read back from the decimals written, or from pixels, rounds).
pub const AREA_TOLERANCE: f64 = 1e-5;

/// The box overlap (intersection over union) at which an entry in the file is one of Dalica's faces.
pub const MATCH_IOU: f64 = 0.5;

/// How close, in radians, a `mwg-rs:Rotation` must be to a quarter or half turn.
const TURN_TOLERANCE: f64 = 0.05;

/// The namespaces a face entry (or a list of them) uses, and `AppliedToDimensions`.
const ENTRY: &[&str] = &[RDF, MWG_RS, ST_AREA];
const DIMS: &[&str] = &[MWG_RS, ST_DIM];

/// The prefixes Dalica prefers for the namespaces it writes here.
const RDF_PREFIX: &str = "rdf";
const MWG_PREFIX: &str = "mwg-rs";
const AREA_PREFIX: &str = "stArea";
const DIM_PREFIX: &str = "stDim";

// --- Reading ------------------------------------------------------------------------------------

/// The face regions of a packet's first top-level `mwg-rs:Regions`, in file order.
pub(super) fn read(rdf: &Rdf, limits: &Limits) -> Vec<FaceRegion> {
    let Some(prop) = rdf.first(MWG_RS, "Regions") else { return Vec::new() };
    let fields = fields(&prop.field.value);
    let dims = dimensions(fields);
    let Some(Value::Array(_, items)) = field(fields, MWG_RS, "RegionList") else { return Vec::new() };
    items
        .iter()
        .filter_map(|i| match entry(&i.value, dims, limits) {
            Entry::Face(f) => Some(f.region),
            _ => None,
        })
        .take(MAX_FACES)
        .collect()
}

/// The fields of a structure (none for a simple value or an array).
fn fields(v: &Value) -> &[Field] {
    match v {
        Value::Struct(f) => f,
        _ => &[],
    }
}

/// The first field `ns`:`local` of a structure.
fn field<'v>(fields: &'v [Field], ns: &str, local: &str) -> Option<&'v Value> {
    fields.iter().find(|f| f.ns == ns && f.local == local).map(|f| &f.value)
}

/// A finite number written as text.
fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `mwg-rs:AppliedToDimensions` in pixels, when both sizes are positive.
fn dimensions(fields: &[Field]) -> Option<(f64, f64)> {
    let d = self::fields(field(fields, MWG_RS, "AppliedToDimensions")?);
    let size = |local: &str| field(d, ST_DIM, local).and_then(Value::text).and_then(number).filter(|v| *v > 0.0);
    size("w").zip(size("h"))
}

/// A region entry as read.
enum Entry {
    Face(Found),
    /// A `Face` entry without a usable box.
    BadFace,
    Other,
}

/// A readable face entry: the upright box, and how the entry writes its box (rotation and unit).
struct Found {
    region: FaceRegion,
    turn: Turn,
    /// The `AppliedToDimensions` a box in pixels was divided by.
    pixels: Option<(f64, f64)>,
}

fn entry(v: &Value, dims: Option<(f64, f64)>, limits: &Limits) -> Entry {
    let Value::Struct(fields) = v else { return Entry::Other };
    let is_face = field(fields, MWG_RS, "Type").and_then(Value::text).is_some_and(|t| t.trim().eq_ignore_ascii_case("Face"));
    if !is_face {
        return Entry::Other;
    }
    face(fields, dims, limits).map_or(Entry::BadFace, Entry::Face)
}

fn face(fields: &[Field], dims: Option<(f64, f64)>, limits: &Limits) -> Option<Found> {
    let Some(Value::Struct(area)) = field(fields, MWG_RS, "Area") else { return None };
    let num = |local: &str| field(area, ST_AREA, local).and_then(Value::text).and_then(number);
    let (cx, cy, w, h) = (num("x")?, num("y")?, num("w")?, num("h")?);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let pixels = match field(area, ST_AREA, "unit").and_then(Value::text).map(str::trim) {
        None => None,
        Some(u) if u.eq_ignore_ascii_case("normalized") => None,
        Some(u) if u.eq_ignore_ascii_case("pixel") => Some(dims?),
        Some(_) => return None,
    };
    let (cx, cy, w, h) = match pixels {
        Some((dw, dh)) => (cx / dw, cy / dh, w / dw, h / dh),
        None => (cx, cy, w, h),
    };
    let turn = Turn::of(field(fields, MWG_RS, "Rotation").and_then(Value::text).and_then(number));
    let (cx, cy, w, h) = turn.upright(cx, cy, w, h);
    let rect = clip(cx, cy, w, h)?;
    let name = field(fields, MWG_RS, "Name").and_then(Value::text).and_then(|n| clean_tag(n, limits.max_string));
    Some(Found { region: FaceRegion { name, rect }, turn, pixels })
}

/// The box with centre (`cx`, `cy`) and size `w` × `h`, clipped to the photo; `None` when nothing of
/// it is left.
fn clip(cx: f64, cy: f64, w: f64, h: f64) -> Option<Rect01> {
    let (x0, x1) = ((cx - w / 2.0).max(0.0), (cx + w / 2.0).min(1.0));
    let (y0, y1) = ((cy - h / 2.0).max(0.0), (cy + h / 2.0).min(1.0));
    Rect01::new(x0, y0, x1, y1).ok()
}

/// How an entry's box is turned from the upright frame (`mwg-rs:Rotation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    None,
    /// −π/2: the box of a photo with EXIF orientation 6, in the sensor's frame.
    Minus90,
    /// +π/2: orientation 8.
    Plus90,
    /// ±π: orientation 3.
    Half,
}

impl Turn {
    fn of(radians: Option<f64>) -> Turn {
        let Some(r) = radians else { return Turn::None };
        // Into −π…π, so 3π/2 is −π/2.
        let r = r - TAU * (r / TAU).round();
        if (r + FRAC_PI_2).abs() < TURN_TOLERANCE {
            Turn::Minus90
        } else if (r - FRAC_PI_2).abs() < TURN_TOLERANCE {
            Turn::Plus90
        } else if (r.abs() - PI).abs() < TURN_TOLERANCE {
            Turn::Half
        } else {
            Turn::None
        }
    }

    /// A box as the entry writes it → the upright box.
    fn upright(self, cx: f64, cy: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
        match self {
            Turn::None => (cx, cy, w, h),
            Turn::Minus90 => (1.0 - cy, cx, h, w),
            Turn::Plus90 => (cy, 1.0 - cx, h, w),
            Turn::Half => (1.0 - cx, 1.0 - cy, w, h),
        }
    }

    /// The inverse of [`Turn::upright`]: an upright box as the entry writes it.
    fn stored(self, cx: f64, cy: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
        match self {
            Turn::None => (cx, cy, w, h),
            Turn::Minus90 => (cy, 1.0 - cx, h, w),
            Turn::Plus90 => (1.0 - cy, cx, h, w),
            Turn::Half => (1.0 - cx, 1.0 - cy, w, h),
        }
    }
}

// --- Comparing ----------------------------------------------------------------------------------

/// The named faces of a write as they are written: each name cleaned ([`clean_tag`], a face
/// whose name cleans to nothing left out), at most [`MAX_FACES`].
pub fn wanted(faces: &NamedFaces, limits: &Limits) -> Vec<(String, Rect01)> {
    faces.faces.iter().filter_map(|f| Some((clean_tag(&f.name, limits.max_string)?, f.rect))).take(MAX_FACES).collect()
}

/// Whether two boxes are the same to within [`AREA_TOLERANCE`].
pub fn close(a: &Rect01, b: &Rect01) -> bool {
    [a.x0() - b.x0(), a.y0() - b.y0(), a.x1() - b.x1(), a.y1() - b.y1()].iter().all(|d| d.abs() <= AREA_TOLERANCE)
}

/// Intersection over union of two boxes.
pub fn iou(a: &Rect01, b: &Rect01) -> f64 {
    let w = (a.x1().min(b.x1()) - a.x0().max(b.x0())).max(0.0);
    let h = (a.y1().min(b.y1()) - a.y0().max(b.y0())).max(0.0);
    let inter = w * h;
    let union = a.width() * a.height() + b.width() * b.height() - inter;
    if union > 0.0 { inter / union } else { 0.0 }
}

/// Whether the faces read back are exactly the named faces written (in any order): as many, and
/// each with its name and its box.
pub fn same_faces(got: &[FaceRegion], want: &[(String, Rect01)]) -> bool {
    if got.len() != want.len() {
        return false;
    }
    let mut used = vec![false; got.len()];
    want.iter().all(|(name, rect)| {
        let hit = got.iter().zip(used.iter()).position(|(g, u)| !u && g.name.as_deref() == Some(name.as_str()) && close(&g.rect, rect));
        hit.and_then(|i| used.get_mut(i)).map(|u| *u = true).is_some()
    })
}

// --- Writing ------------------------------------------------------------------------------------

/// What a write does to a packet's regions.
#[derive(Debug, PartialEq)]
pub(super) enum Plan {
    /// Nothing to change.
    Unchanged,
    /// Edits (source offsets) inside the packet's first `mwg-rs:Regions` element, `element`.
    Edits { element: usize, edits: Vec<(Range<usize>, String)> },
    /// The packet has no regions: a new `mwg-rs:Regions` ([`new_element`]) goes into Dalica's
    /// description.
    New,
}

/// The faces to write and the photo's upright size.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Want {
    pub faces: Vec<(String, Rect01)>,
    pub dims: Option<(u32, u32)>,
}

impl Want {
    pub fn of(faces: &NamedFaces, limits: &Limits) -> Want {
        let dims = (faces.width > 0 && faces.height > 0).then_some((faces.width, faces.height));
        Want { faces: wanted(faces, limits), dims }
    }
}

/// The description element and the `mwg-rs:Regions` element of the packet's first regions.
pub(super) fn find(rdf: &Rdf) -> Option<(usize, At)> {
    rdf.descriptions.iter().find_map(|d| d.props.iter().find(|p| p.field.ns == MWG_RS && p.field.local == "Regions").map(|p| (d.element, p.at)))
}

/// The edits that write `want` into the packet's regions. `contained`: the regions are in Dalica's
/// description, so whatever is inserted must declare its own namespaces (the element stays
/// self-contained, `merge` keeps it as written).
pub(super) fn plan(doc: &Doc<'_>, rdf: &Rdf, want: &Want, contained: bool, limits: &Limits) -> Result<Plan> {
    let Some((_, at)) = find(rdf) else {
        return Ok(if want.faces.is_empty() { Plan::Unchanged } else { Plan::New });
    };
    let At::Element(regions) = at else { return Err(Error::Malformed("XMP: mwg-rs:Regions is not a structure".to_owned())) };
    let stop = contained.then_some(regions);
    let holder = holder(doc, regions);
    let dims_read = dimensions(fields(&value_of(doc, regions)));
    let list = child(doc, holder, MWG_RS, "RegionList");
    let bag = list
        .and_then(|l| doc.element(l)?.children.first().copied())
        .filter(|&b| doc.element(b).is_some_and(|e| ["Bag", "Seq", "Alt"].iter().any(|k| doc.is(&e.name, RDF, k))));

    // The faces in the file, and which of Dalica's each one is.
    let mut found: Vec<(usize, Found)> = Vec::new();
    if let Some(b) = bag.and_then(|b| doc.element(b)) {
        for &li in &b.children {
            if doc.element(li).is_some_and(|e| doc.is(&e.name, RDF, "li"))
                && let Entry::Face(f) = entry(&value_of(doc, li), dims_read, limits)
            {
                found.push((li, f));
            }
        }
    }
    let mut match_of: Vec<Option<usize>> = vec![None; found.len()];
    let mut taken = vec![false; want.faces.len()];
    for (j, (name, rect)) in want.faces.iter().enumerate() {
        // Of the free entries overlapping enough: one already of that name first, then the closest,
        // then the first in the file (so a second write matches as the first did).
        let best = found
            .iter()
            .enumerate()
            .filter(|(i, _)| match_of.get(*i).is_some_and(Option::is_none))
            .map(|(i, (_, f))| (i, (f.region.name.as_deref() == Some(name.as_str()), iou(&f.region.rect, rect))))
            .filter(|(_, (_, o))| *o >= MATCH_IOU)
            .fold(
                None,
                |best: Option<(usize, (bool, f64))>, (i, k)| {
                    if best.is_some_and(|(_, b)| (b.0, b.1) >= (k.0, k.1)) { best } else { Some((i, k)) }
                },
            );
        if let Some((i, _)) = best {
            if let Some(m) = match_of.get_mut(i) {
                *m = Some(j);
            }
            if let Some(t) = taken.get_mut(j) {
                *t = true;
            }
        }
    }

    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for ((li, f), m) in found.iter().zip(&match_of) {
        match m.and_then(|j| want.faces.get(j)) {
            Some((name, rect)) => edits.extend(entry_edits(doc, *li, f, name, rect, stop)),
            None => {
                let span = doc.element(*li).map(|e| doc.space_before(e.span.start).start..e.span.end).ok_or(Error::OutOfRange)?;
                edits.push((span, String::new()));
            }
        }
    }
    let new: Vec<&(String, Rect01)> = want.faces.iter().zip(&taken).filter(|(_, t)| !**t).map(|(f, _)| f).collect();
    let dims = want.dims.filter(|_| !want.faces.is_empty() && child(doc, holder, MWG_RS, "AppliedToDimensions").is_none());
    match (list, bag) {
        (_, Some(bag)) => {
            if !new.is_empty() {
                let ns = Names::at(doc, bag, stop, ENTRY);
                edits.push(append(doc, bag, &|nl| {
                    new.iter().map(|(n, r)| li_text(&ns, &ns.decls, n, r, nl)).collect::<Vec<_>>().join(nl.unwrap_or_default())
                })?);
            }
            if let (Some((w, h)), Some(list)) = (dims, list) {
                let ns = Names::at(doc, holder, stop, DIMS);
                edits.push(before(doc, list, &|_| dims_text(&ns, &ns.decls, w, h))?);
            }
        }
        (Some(list), None) => {
            // A list without an array: its content becomes one.
            if !new.is_empty() {
                let ns = Names::at(doc, list, stop, ENTRY);
                let nl = child_indent(doc, list);
                let text = bag_text(&ns, &ns.decls, &new, nl.as_deref());
                edits.push(set_content(
                    doc,
                    list,
                    &format!("{}{text}{}", nl.as_deref().unwrap_or_default(), own_indent(doc, list).unwrap_or_default()),
                )?);
            }
            if let Some((w, h)) = dims {
                let ns = Names::at(doc, holder, stop, DIMS);
                edits.push(before(doc, list, &|_| dims_text(&ns, &ns.decls, w, h))?);
            }
        }
        (None, _) => {
            let ns = Names::at(doc, holder, stop, if dims.is_some() { &[RDF, MWG_RS, ST_AREA, ST_DIM] } else { ENTRY });
            if dims.is_some() || !new.is_empty() {
                edits.push(append(doc, holder, &|nl| {
                    let mut s = String::new();
                    if let Some((w, h)) = dims {
                        s.push_str(&dims_text(&ns, &ns.decls, w, h));
                    }
                    if !new.is_empty() {
                        if dims.is_some() {
                            s.push_str(nl.unwrap_or_default());
                        }
                        s.push_str(&list_text(&ns, &ns.decls, &new, nl));
                    }
                    s
                })?);
            }
        }
    }
    Ok(if edits.is_empty() { Plan::Unchanged } else { Plan::Edits { element: regions, edits } })
}

/// A new `mwg-rs:Regions` holding `want`, every namespace it uses declared on it. `nl`: the line
/// break and indentation it starts at (`None`: written without white space).
pub(super) fn new_element(want: &Want, nl: Option<&str>) -> String {
    let ns = Names::declared();
    let inner = nl.map(|i| format!("{i} "));
    let faces: Vec<&(String, Rect01)> = want.faces.iter().collect();
    let mut s = format!("<{m}:Regions{} {r}:parseType=\"Resource\">", ns.decls, m = ns.mwg, r = ns.rdf);
    if let Some((w, h)) = want.dims {
        s.push_str(&format!("{}{}", inner.as_deref().unwrap_or_default(), dims_text(&ns, "", w, h)));
    }
    s.push_str(&format!("{}{}", inner.as_deref().unwrap_or_default(), list_text(&ns, "", &faces, inner.as_deref())));
    s.push_str(&format!("{}</{}:Regions>", nl.unwrap_or_default(), ns.mwg));
    s
}

/// The edits that give a matched entry Dalica's name and box, where they differ.
fn entry_edits(doc: &Doc<'_>, li: usize, f: &Found, name: &str, rect: &Rect01, stop: Option<usize>) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    let holder = holder(doc, li);
    if f.region.name.as_deref() != Some(name) {
        let edit = match slot(doc, holder, MWG_RS, "Name") {
            Some(Slot::Attr(range, quote)) => Some((range, escape(name, Some(quote)))),
            Some(Slot::Element(el)) => set_content(doc, el, &escape(name, None)).ok(),
            None => {
                let ns = Names::at(doc, holder, stop, &[MWG_RS]);
                append(doc, holder, &|_| format!("<{m}:Name{}>{}</{m}:Name>", ns.decls, escape(name, None), m = ns.mwg)).ok()
            }
        };
        out.extend(edit);
    }
    if !close(&f.region.rect, rect) {
        let (cx, cy) = ((rect.x0() + rect.x1()) / 2.0, (rect.y0() + rect.y1()) / 2.0);
        let (cx, cy, w, h) = f.turn.stored(cx, cy, rect.width(), rect.height());
        let (cx, cy, w, h) = match f.pixels {
            Some((dw, dh)) => (cx * dw, cy * dh, w * dw, h * dh),
            None => (cx, cy, w, h),
        };
        if let Some(Slot::Element(area)) = slot(doc, holder, MWG_RS, "Area") {
            let area = self::holder(doc, area);
            for (local, v) in [("x", cx), ("y", cy), ("w", w), ("h", h)] {
                let edit = match slot(doc, area, ST_AREA, local) {
                    Some(Slot::Attr(range, _)) => Some((range, number_text(v))),
                    Some(Slot::Element(el)) => set_content(doc, el, &number_text(v)).ok(),
                    None => None,
                };
                out.extend(edit);
            }
        }
    }
    out
}

/// The element whose attributes and children are the fields of the structure `el` holds, as
/// `rdf::value_of` reads it: `el` itself (`rdf:parseType="Resource"` or shorthand), or the
/// `rdf:Description` inside it.
fn holder(doc: &Doc<'_>, el: usize) -> usize {
    if doc.attr(el, RDF, "parseType") == Some("Resource") {
        return el;
    }
    match doc.element(el).and_then(|e| e.children.first().copied()) {
        Some(c) if doc.element(c).is_some_and(|ce| doc.is(&ce.name, RDF, "Description")) => c,
        _ => el,
    }
}

/// The first child element `ns`:`local` of `parent`.
fn child(doc: &Doc<'_>, parent: usize, ns: &str, local: &str) -> Option<usize> {
    doc.element(parent)?.children.iter().copied().find(|&c| doc.element(c).is_some_and(|e| doc.is(&e.name, ns, local)))
}

/// Where a field of a structure is written: the value of an attribute (and its quote), or an element.
enum Slot {
    Attr(Range<usize>, char),
    Element(usize),
}

/// The first field `ns`:`local` of the structure held by `holder`: attributes first, then child
/// elements (the order `rdf` reads them in).
fn slot(doc: &Doc<'_>, holder: usize, ns: &str, local: &str) -> Option<Slot> {
    let el = doc.element(holder)?;
    if let Some(a) = el.attrs.iter().find(|a| !a.is_xmlns && doc.is(&a.name, ns, local)) {
        // The value lies between the quotes after the `=`.
        let text = doc.src.get(a.span.clone())?;
        let eq = text.find('=')?;
        let open = text.get(eq..)?.find(['"', '\''])? + eq;
        let quote = text.get(open..)?.chars().next()?;
        let start = a.span.start + open + 1;
        let end = a.span.end.checked_sub(1)?;
        return (start <= end).then_some(Slot::Attr(start..end, quote));
    }
    child(doc, holder, ns, local).map(Slot::Element)
}

/// The edit that replaces an element's content with `text` (an empty element gets an end tag).
fn set_content(doc: &Doc<'_>, el: usize, text: &str) -> Result<(Range<usize>, String)> {
    let e = doc.element(el).ok_or(Error::OutOfRange)?;
    if e.empty {
        let end = e.span.end;
        let start = end.checked_sub(2).filter(|&s| doc.src.get(s..end) == Some("/>")).ok_or(Error::OutOfRange)?;
        return Ok((start..end, format!(">{text}</{}>", e.raw_name)));
    }
    Ok((e.open_end..e.close_start, text.to_owned()))
}

/// The edit that appends children to `parent`: `make` gets the line break and indentation of a
/// child (or `None`) and returns the children's text, each after the first preceded by it.
fn append(doc: &Doc<'_>, parent: usize, make: &dyn Fn(Option<&str>) -> String) -> Result<(Range<usize>, String)> {
    let e = doc.element(parent).ok_or(Error::OutOfRange)?;
    let nl = child_indent(doc, parent);
    let body = format!("{}{}", nl.as_deref().unwrap_or_default(), make(nl.as_deref()));
    if e.empty {
        return set_content(doc, parent, &format!("{body}{}", own_indent(doc, parent).unwrap_or_default()));
    }
    let at = doc.space_before(e.close_start).start;
    Ok((at..at, body))
}

/// The edit that puts an element just before `sibling`, on a line of its own like it.
fn before(doc: &Doc<'_>, sibling: usize, make: &dyn Fn(Option<&str>) -> String) -> Result<(Range<usize>, String)> {
    let e = doc.element(sibling).ok_or(Error::OutOfRange)?;
    let space = doc.space_before(e.span.start);
    let nl = super::merge::indent_at(doc.src, space.clone());
    Ok((space.start..space.start, format!("{}{}", nl.as_deref().unwrap_or_default(), make(nl.as_deref()))))
}

/// The line break and indentation of an element's own start tag.
fn own_indent(doc: &Doc<'_>, el: usize) -> Option<String> {
    let e = doc.element(el)?;
    super::merge::indent_at(doc.src, doc.space_before(e.span.start))
}

/// The line break and indentation of a child of `parent`: its first child's, else one space more
/// than the parent's own.
fn child_indent(doc: &Doc<'_>, parent: usize) -> Option<String> {
    let e = doc.element(parent)?;
    match e.children.first().and_then(|&c| doc.element(c)) {
        Some(c) => super::merge::indent_at(doc.src, doc.space_before(c.span.start)),
        None => own_indent(doc, parent).map(|i| format!("{i} ")),
    }
}

/// The prefixes inserted elements use and the namespace declarations the outermost of them needs.
struct Names {
    rdf: String,
    mwg: String,
    area: String,
    dim: String,
    decls: String,
}

impl Names {
    /// Every namespace declared under Dalica's prefixes (a self-contained element).
    fn declared() -> Names {
        let decls = [(RDF_PREFIX, RDF), (MWG_PREFIX, MWG_RS), (AREA_PREFIX, ST_AREA), (DIM_PREFIX, ST_DIM)]
            .iter()
            .map(|(p, ns)| format!(" xmlns:{p}=\"{ns}\""))
            .collect();
        Names { rdf: RDF_PREFIX.to_owned(), mwg: MWG_PREFIX.to_owned(), area: AREA_PREFIX.to_owned(), dim: DIM_PREFIX.to_owned(), decls }
    }

    /// The prefixes bound at `el` (looking no further up than `stop`), and declarations for the
    /// namespaces in `need` that have none.
    fn at(doc: &Doc<'_>, el: usize, stop: Option<usize>, need: &[&str]) -> Names {
        let mut bound: BTreeMap<&str, &str> = BTreeMap::new();
        let mut at = Some(el);
        while let Some(i) = at {
            let Some(e) = doc.element(i) else { break };
            for a in e.attrs.iter().filter(|a| a.is_xmlns && !a.name.local.is_empty()) {
                bound.entry(a.name.local).or_insert(a.value.as_str());
            }
            if Some(i) == stop {
                break;
            }
            at = e.parent;
        }
        let ours = [RDF_PREFIX, MWG_PREFIX, AREA_PREFIX, DIM_PREFIX];
        let mut decls = String::new();
        let mut pick = |ns: &str, preferred: &str| -> String {
            if bound.get(preferred) == Some(&ns) {
                return preferred.to_owned();
            }
            if let Some((p, _)) = bound.iter().find(|(p, v)| **v == ns && !ours.contains(p)) {
                return (*p).to_owned();
            }
            if need.contains(&ns) {
                decls.push_str(&format!(" xmlns:{preferred}=\"{ns}\""));
            }
            preferred.to_owned()
        };
        let (rdf, mwg, area, dim) = (pick(RDF, RDF_PREFIX), pick(MWG_RS, MWG_PREFIX), pick(ST_AREA, AREA_PREFIX), pick(ST_DIM, DIM_PREFIX));
        Names { rdf, mwg, area, dim, decls }
    }
}

/// `mwg-rs:AppliedToDimensions` of `w` × `h` pixels.
fn dims_text(ns: &Names, decls: &str, w: u32, h: u32) -> String {
    format!("<{m}:AppliedToDimensions{decls} {d}:w=\"{w}\" {d}:h=\"{h}\" {d}:unit=\"pixel\"/>", m = ns.mwg, d = ns.dim)
}

/// A `mwg-rs:RegionList` holding `faces`. `nl`: the list's own line (`None`: no white space).
fn list_text(ns: &Names, decls: &str, faces: &[&(String, Rect01)], nl: Option<&str>) -> String {
    let inner = nl.map(|i| format!("{i} "));
    format!(
        "<{m}:RegionList{decls}>{}{}{}</{m}:RegionList>",
        inner.as_deref().unwrap_or_default(),
        bag_text(ns, "", faces, inner.as_deref()),
        nl.unwrap_or_default(),
        m = ns.mwg
    )
}

/// An `rdf:Bag` of face entries. `nl`: the bag's own line.
fn bag_text(ns: &Names, decls: &str, faces: &[&(String, Rect01)], nl: Option<&str>) -> String {
    let inner = nl.map(|i| format!("{i} "));
    let items: String =
        faces.iter().map(|(n, r)| format!("{}{}", inner.as_deref().unwrap_or_default(), li_text(ns, "", n, r, inner.as_deref()))).collect();
    format!("<{r}:Bag{decls}>{items}{}</{r}:Bag>", nl.unwrap_or_default(), r = ns.rdf)
}

/// One face entry: name, type and a normalised area. `nl`: the entry's own line; an entry after
/// the first in the same text is preceded by it.
fn li_text(ns: &Names, decls: &str, name: &str, rect: &Rect01, nl: Option<&str>) -> String {
    let inner = nl.map(|i| format!("{i} ")).unwrap_or_default();
    let (cx, cy) = ((rect.x0() + rect.x1()) / 2.0, (rect.y0() + rect.y1()) / 2.0);
    let (r, m, a) = (&ns.rdf, &ns.mwg, &ns.area);
    format!(
        "<{r}:li{decls} {r}:parseType=\"Resource\">{inner}<{m}:Name>{}</{m}:Name>{inner}<{m}:Type>Face</{m}:Type>{inner}<{m}:Area {a}:x=\"{}\" {a}:y=\"{}\" {a}:w=\"{}\" {a}:h=\"{}\" {a}:unit=\"normalized\"/>{}</{r}:li>",
        escape(name, None),
        number_text(cx),
        number_text(cy),
        number_text(rect.width()),
        number_text(rect.height()),
        nl.unwrap_or_default()
    )
}

/// A number with at most seven decimals (a stored box has six; its centre needs one more), without
/// trailing zeros.
fn number_text(v: f64) -> String {
    let s = format!("{v:.7}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".to_owned() } else { s.to_owned() }
}

/// Text escaped for element content, or for an attribute value between `quote`s.
fn escape(s: &str, quote: Option<char>) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            '"' if quote == Some('"') => out.push_str("&quot;"),
            '\'' if quote == Some('\'') => out.push_str("&apos;"),
            '\n' if quote.is_some() => out.push_str("&#xA;"),
            '\t' if quote.is_some() => out.push_str("&#x9;"),
            c if is_xml_char(c) => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect01 {
        Rect01::new(x0, y0, x1, y1).unwrap()
    }

    #[test]
    fn turns_invert() {
        for turn in [Turn::None, Turn::Minus90, Turn::Plus90, Turn::Half] {
            let (cx, cy, w, h) = turn.stored(0.3, 0.4, 0.2, 0.1);
            let back = turn.upright(cx, cy, w, h);
            assert!([back.0 - 0.3, back.1 - 0.4, back.2 - 0.2, back.3 - 0.1].iter().all(|d| d.abs() < 1e-12), "{turn:?}");
        }
        assert_eq!(Turn::of(Some(-FRAC_PI_2 + 0.01)), Turn::Minus90);
        assert_eq!(Turn::of(Some(3.0 * FRAC_PI_2)), Turn::Minus90);
        assert_eq!(Turn::of(Some(-PI + 0.02)), Turn::Half);
        assert_eq!(Turn::of(Some(0.3)), Turn::None);
    }

    #[test]
    fn overlap_and_numbers() {
        let a = rect(0.0, 0.0, 0.5, 0.5);
        assert!((iou(&a, &a) - 1.0).abs() < 1e-12);
        assert!((iou(&a, &rect(0.25, 0.0, 0.75, 0.5)) - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(iou(&a, &rect(0.6, 0.6, 0.7, 0.7)), 0.0);
        assert_eq!(number_text(0.5), "0.5");
        assert_eq!(number_text(0.1234565), "0.1234565");
        assert_eq!(number_text(2.0), "2");
        assert_eq!(number_text(-0.0), "0");
        assert!(clip(-1.0, 0.5, 0.5, 0.2).is_none() && clip(0.0, 0.5, 0.5, 0.2).is_some());
    }

    #[test]
    fn escaping() {
        assert_eq!(escape("a&<>\"'\n\u{1}", None), "a&amp;&lt;&gt;\"'\n");
        assert_eq!(escape("a\"'\n", Some('\'')), "a\"&apos;&#xA;");
    }
}
