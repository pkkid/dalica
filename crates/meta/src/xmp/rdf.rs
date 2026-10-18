//! The RDF inside an XMP packet (ISO 16684-1 §7): the top-level `rdf:Description` elements under
//! `rdf:RDF` and their properties in attribute form (`xmp:Rating="5"`) and element form, with
//! simple values, `rdf:resource` URIs, arrays (`rdf:Bag`, `rdf:Seq`, `rdf:Alt` with `xml:lang`)
//! and structures (`rdf:parseType="Resource"`, a nested `rdf:Description`, or shorthand
//! attributes). Properties are matched by namespace name, never by prefix, so a packet that binds
//! the Dublin Core namespace to `foo:` still has a `dc:description`.

use super::xml::{Doc, Name, XML_NS};

pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub const X: &str = "adobe:ns:meta/";
pub const DC: &str = "http://purl.org/dc/elements/1.1/";
pub const XMP: &str = "http://ns.adobe.com/xap/1.0/";
pub const EXIF: &str = "http://ns.adobe.com/exif/1.0/";
pub const PHOTOSHOP: &str = "http://ns.adobe.com/photoshop/1.0/";
/// The Metadata Working Group region schema (`xmp-interop.md` §6).
pub const MWG_RS: &str = "http://www.metadataworkinggroup.com/schemas/regions/";
/// The area structure of a region (`stArea:x`, `y`, `w`, `h`, `unit`).
pub const ST_AREA: &str = "http://ns.adobe.com/xmp/sType/Area#";
/// The dimensions structure (`stDim:w`, `h`, `unit`) of `mwg-rs:AppliedToDimensions`.
pub const ST_DIM: &str = "http://ns.adobe.com/xap/1.0/sType/Dimensions#";

/// The usual prefix of well-known namespaces, for listing labels (a packet may bind any prefix).
const PREFIXES: &[(&str, &str)] = &[
    (DC, "dc"),
    (XMP, "xmp"),
    (EXIF, "exif"),
    (PHOTOSHOP, "photoshop"),
    (RDF, "rdf"),
    ("http://cipa.jp/exif/1.0/", "exifEX"),
    ("http://ns.adobe.com/tiff/1.0/", "tiff"),
    ("http://ns.adobe.com/exif/1.0/aux/", "aux"),
    ("http://ns.adobe.com/xap/1.0/mm/", "xmpMM"),
    ("http://ns.adobe.com/xap/1.0/rights/", "xmpRights"),
    ("http://ns.adobe.com/xmp/note/", "xmpNote"),
    ("http://ns.adobe.com/lightroom/1.0/", "lr"),
    ("http://ns.adobe.com/camera-raw-settings/1.0/", "crs"),
    ("http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/", "Iptc4xmpCore"),
    ("http://iptc.org/std/Iptc4xmpExt/2008-02-29/", "Iptc4xmpExt"),
    (MWG_RS, "mwg-rs"),
    (ST_AREA, "stArea"),
    (ST_DIM, "stDim"),
    (XML_NS, "xml"),
];

/// The kind of an RDF array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayKind {
    Bag,
    Seq,
    Alt,
}

/// A property value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A simple value, or an `rdf:resource` URI.
    Text(String),
    Array(ArrayKind, Vec<Item>),
    Struct(Vec<Field>),
}

impl Value {
    /// The text of a simple value, or of the `rdf:value` of a qualified one.
    pub fn text(&self) -> Option<&str> {
        match self {
            Value::Text(t) => Some(t),
            Value::Struct(fields) => fields.iter().find(|f| f.ns == RDF && f.local == "value").and_then(|f| f.value.text()),
            Value::Array(..) => None,
        }
    }
}

/// One item of an array.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Its `xml:lang`, if any.
    pub lang: Option<String>,
    pub value: Value,
}

/// One field of a structure (or a top-level property).
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub ns: String,
    pub prefix: String,
    pub local: String,
    pub value: Value,
}

/// Where a top-level property is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    /// The attribute with this index on the description's start tag.
    Attr(usize),
    /// This element (a child of the description).
    Element(usize),
}

/// A property of a top-level description.
#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub field: Field,
    pub at: At,
}

/// A top-level `rdf:Description`.
#[derive(Clone, Debug, PartialEq)]
pub struct Description {
    pub element: usize,
    pub props: Vec<Prop>,
}

/// The RDF of a packet.
#[derive(Clone, Debug)]
pub struct Rdf {
    /// The `rdf:RDF` element, if there is one.
    pub rdf: Option<usize>,
    pub descriptions: Vec<Description>,
}

impl Rdf {
    pub fn new(doc: &Doc<'_>) -> Rdf {
        let rdf = doc.elements.iter().position(|e| doc.is(&e.name, RDF, "RDF"));
        let mut descriptions = Vec::new();
        if let Some(el) = rdf.and_then(|i| doc.element(i)) {
            for &child in &el.children {
                if doc.element(child).is_some_and(|c| doc.is(&c.name, RDF, "Description")) {
                    descriptions.push(Description { element: child, props: props_of(doc, child) });
                }
            }
        }
        Rdf { rdf, descriptions }
    }

    /// The first top-level property `ns`:`local` in any description.
    pub fn first(&self, ns: &str, local: &str) -> Option<&Prop> {
        self.descriptions.iter().flat_map(|d| d.props.iter()).find(|p| p.field.ns == ns && p.field.local == local)
    }
}

fn field(doc: &Doc<'_>, name: &Name<'_>, value: Value) -> Field {
    Field { ns: doc.ns(name).unwrap_or_default().to_owned(), prefix: name.prefix.to_owned(), local: name.local.to_owned(), value }
}

/// Whether an attribute is RDF syntax or an XML attribute rather than a property.
fn is_syntax(doc: &Doc<'_>, name: &Name<'_>) -> bool {
    matches!(doc.ns(name), Some(RDF) | Some(XML_NS) | None)
}

/// The properties of a description element: its attributes, then its child elements.
fn props_of(doc: &Doc<'_>, el: usize) -> Vec<Prop> {
    let Some(e) = doc.element(el) else { return Vec::new() };
    let mut out = Vec::new();
    for (i, a) in e.attrs.iter().enumerate() {
        if !a.is_xmlns && !is_syntax(doc, &a.name) {
            out.push(Prop { field: field(doc, &a.name, Value::Text(a.value.clone())), at: At::Attr(i) });
        }
    }
    for &c in &e.children {
        if let Some(ce) = doc.element(c) {
            out.push(Prop { field: field(doc, &ce.name, value_of(doc, c)), at: At::Element(c) });
        }
    }
    out
}

/// The fields of a structure written as a description or a `parseType="Resource"` element.
fn fields_of(doc: &Doc<'_>, el: usize) -> Vec<Field> {
    props_of(doc, el).into_iter().map(|p| p.field).collect()
}

/// The value of a property element (or an array item). Recursion follows the element tree, whose
/// depth the tokenizer bounds.
pub fn value_of(doc: &Doc<'_>, el: usize) -> Value {
    let Some(e) = doc.element(el) else { return Value::Text(String::new()) };
    if let Some(uri) = doc.attr(el, RDF, "resource") {
        return Value::Text(uri.to_owned());
    }
    if doc.attr(el, RDF, "parseType") == Some("Resource") {
        return Value::Struct(fields_of(doc, el));
    }
    let shorthand: Vec<Field> =
        e.attrs.iter().filter(|a| !a.is_xmlns && !is_syntax(doc, &a.name)).map(|a| field(doc, &a.name, Value::Text(a.value.clone()))).collect();
    let Some(&first) = e.children.first() else {
        return if shorthand.is_empty() { Value::Text(e.text.clone()) } else { Value::Struct(shorthand) };
    };
    let Some(child) = doc.element(first) else { return Value::Text(e.text.clone()) };
    let kind = [("Bag", ArrayKind::Bag), ("Seq", ArrayKind::Seq), ("Alt", ArrayKind::Alt)]
        .into_iter()
        .find_map(|(name, kind)| doc.is(&child.name, RDF, name).then_some(kind));
    if let Some(kind) = kind {
        let items = child
            .children
            .iter()
            .filter(|&&li| doc.element(li).is_some_and(|l| doc.is(&l.name, RDF, "li")))
            .map(|&li| Item { lang: doc.attr(li, XML_NS, "lang").map(str::to_owned), value: value_of(doc, li) })
            .collect();
        return Value::Array(kind, items);
    }
    if doc.is(&child.name, RDF, "Description") {
        return Value::Struct(fields_of(doc, first));
    }
    // Child property elements without `parseType` (not valid RDF, but seen): a structure of the
    // shorthand attributes and those children.
    Value::Struct(fields_of(doc, el))
}

/// The label of a property: the usual prefix of its namespace, else the one written.
pub fn label(f: &Field) -> String {
    let prefix = PREFIXES.iter().find(|(ns, _)| *ns == f.ns).map_or(f.prefix.as_str(), |(_, p)| p);
    if prefix.is_empty() { f.local.clone() } else { format!("{prefix}:{}", f.local) }
}
