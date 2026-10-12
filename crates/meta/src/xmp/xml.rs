//! Our own bounded XML reader for XMP packets (`security.md` §4.2): it builds a small
//! namespace-resolved element tree that keeps the byte span of every element and attribute, so the
//! merge writer can cut and splice the packet without touching anything it does not own.
//!
//! It reads the part of XML 1.0 and XML Namespaces that XMP uses: elements, attributes, character
//! data, CDATA sections, comments and processing instructions. Everything that could make a small
//! packet expensive or reach outside it is refused: a `DOCTYPE` (and with it every entity
//! declaration and external reference), any entity reference other than the five predefined ones
//! and character references, more than [`MAX_DEPTH`] levels of elements, and more than
//! [`MAX_NODES`] nodes (elements, attributes, text runs, comments, processing instructions and
//! CDATA sections each count one). Prefixes are resolved through hash maps and repeated attributes
//! are found with a set, so no input makes the work quadratic.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::{Error, Result};

/// The deepest nesting of elements read (the root element is level 1).
pub const MAX_DEPTH: usize = 64;
/// The most nodes read from one packet.
pub const MAX_NODES: usize = 200_000;

/// The namespace the `xml:` prefix is bound to.
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// How long a character or entity reference may be, `&` and `;` included.
const MAX_REFERENCE: usize = 16;

/// A name with its namespace resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name<'a> {
    /// The prefix as written (empty when there is none).
    pub prefix: &'a str,
    pub local: &'a str,
    /// Index into [`Doc::namespaces`]; `None` for unprefixed attributes, namespace declarations and
    /// unprefixed elements outside any default namespace.
    pub ns: Option<usize>,
}

/// One attribute of a start tag.
#[derive(Clone, Debug)]
pub struct Attr<'a> {
    pub name: Name<'a>,
    /// `xmlns` or `xmlns:…`.
    pub is_xmlns: bool,
    /// The value with references replaced and white space normalised.
    pub value: String,
    /// From the white space before the name to just after the closing quote, so cutting it leaves
    /// the tag well formed.
    pub span: Range<usize>,
}

/// One element.
#[derive(Clone, Debug)]
pub struct Element<'a> {
    pub name: Name<'a>,
    /// The name as written in the start tag.
    pub raw_name: &'a str,
    pub attrs: Vec<Attr<'a>>,
    /// The child elements, in document order (indices into [`Doc::elements`]).
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    /// The character data directly inside, references replaced and line ends normalised.
    pub text: String,
    /// Whether that character data holds anything but white space.
    pub has_text: bool,
    /// From `<` of the start tag to just after the end tag (or after `/>`).
    pub span: Range<usize>,
    /// Just after the start tag's `>`.
    pub open_end: usize,
    /// Where the end tag starts (`open_end` for an empty element).
    pub close_start: usize,
    /// Written as `<… />`.
    pub empty: bool,
}

/// A parsed packet.
#[derive(Clone, Debug)]
pub struct Doc<'a> {
    pub src: &'a str,
    /// Every element, in document order (a parent before its children); the root is the first.
    pub elements: Vec<Element<'a>>,
    /// The namespace names in use, each once.
    pub namespaces: Vec<String>,
    /// The `<?xpacket end=…?>` instruction after the root element, if any.
    pub packet_end: Option<Range<usize>>,
}

impl<'a> Doc<'a> {
    pub fn element(&self, i: usize) -> Option<&Element<'a>> {
        self.elements.get(i)
    }

    /// The namespace name a resolved name is in.
    pub fn ns(&self, name: &Name<'_>) -> Option<&str> {
        name.ns.and_then(|i| self.namespaces.get(i)).map(String::as_str)
    }

    /// Whether `name` is `local` in namespace `ns`.
    pub fn is(&self, name: &Name<'_>, ns: &str, local: &str) -> bool {
        name.local == local && self.ns(name) == Some(ns)
    }

    /// The value of the attribute `local` in namespace `ns` of element `i`.
    pub fn attr(&self, i: usize, ns: &str, local: &str) -> Option<&str> {
        let el = self.element(i)?;
        el.attrs.iter().find(|a| !a.is_xmlns && self.is(&a.name, ns, local)).map(|a| a.value.as_str())
    }

    /// The white space run that ends at `at` (nothing but white space between it and `at`).
    pub fn space_before(&self, at: usize) -> Range<usize> {
        let bytes = self.src.as_bytes();
        let mut start = at;
        while start > 0 && bytes.get(start - 1).is_some_and(|&b| is_space(b)) {
            start -= 1;
        }
        start..at
    }
}

/// Whether a packet holds nothing but white space and NUL bytes (an empty block some writers leave).
pub fn is_blank(bytes: &[u8]) -> bool {
    let rest = bytes.strip_prefix("\u{feff}".as_bytes()).unwrap_or(bytes);
    rest.iter().all(|&b| is_space(b) || b == 0)
}

/// XML white space.
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// Whether `c` may appear in an XML 1.0 document.
pub fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..)
}

/// Parses a packet under [`MAX_DEPTH`] and [`MAX_NODES`].
pub fn parse(src: &str) -> Result<Doc<'_>> {
    let mut p = Parser::new(src);
    p.run()?;
    p.finish()
}

fn malformed(what: &str) -> Error {
    Error::Malformed(format!("XMP: {what}"))
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    nodes: usize,
    elements: Vec<Element<'a>>,
    /// The open elements, innermost last.
    stack: Vec<usize>,
    namespaces: Vec<String>,
    ns_index: HashMap<String, usize>,
    /// Prefix → its bindings, innermost last (`""` is the default namespace; `None` undeclares it).
    bindings: HashMap<&'a str, Vec<Option<usize>>>,
    /// The prefixes each open element declared, to unbind when it closes.
    declared: Vec<Vec<&'a str>>,
    root_closed: bool,
    packet_end: Option<Range<usize>>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Parser<'a> {
        let mut p = Parser {
            src,
            pos: 0,
            nodes: 0,
            elements: Vec::new(),
            stack: Vec::new(),
            namespaces: Vec::new(),
            ns_index: HashMap::new(),
            bindings: HashMap::new(),
            declared: Vec::new(),
            root_closed: false,
            packet_end: None,
        };
        let xml = p.intern(XML_NS);
        p.bindings.insert("xml", vec![Some(xml)]);
        p
    }

    fn bytes(&self) -> &'a [u8] {
        self.src.as_bytes()
    }

    fn rest(&self) -> &'a [u8] {
        self.bytes().get(self.pos..).unwrap_or_default()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes().get(self.pos).copied()
    }

    fn slice(&self, r: Range<usize>) -> Result<&'a str> {
        self.src.get(r).ok_or_else(|| malformed("a name or value is cut inside a character"))
    }

    fn count(&mut self, n: usize) -> Result<()> {
        self.nodes = self.nodes.saturating_add(n);
        if self.nodes > MAX_NODES { Err(Error::TooMany("XML nodes")) } else { Ok(()) }
    }

    fn intern(&mut self, uri: &str) -> usize {
        if let Some(&i) = self.ns_index.get(uri) {
            return i;
        }
        let i = self.namespaces.len();
        self.namespaces.push(uri.to_owned());
        self.ns_index.insert(uri.to_owned(), i);
        i
    }

    /// The position just after the next `needle` at or after `from`.
    fn find(&self, from: usize, needle: &[u8]) -> Result<usize> {
        let hay = self.bytes().get(from..).unwrap_or_default();
        let at = hay.windows(needle.len()).position(|w| w == needle).ok_or(Error::Truncated)?;
        Ok(from + at + needle.len())
    }

    fn run(&mut self) -> Result<()> {
        // A byte-order mark before anything else is not text.
        if self.rest().starts_with("\u{feff}".as_bytes()) {
            self.pos += 3;
        }
        while self.pos < self.bytes().len() {
            let rest = self.rest();
            if rest.starts_with(b"<?") {
                self.instruction()?;
            } else if rest.starts_with(b"<!--") {
                let end = self.find(self.pos + 4, b"-->")?;
                self.count(1)?;
                self.pos = end;
            } else if rest.starts_with(b"<![CDATA[") {
                self.cdata()?;
            } else if rest.starts_with(b"<!") {
                return Err(malformed("a DOCTYPE or markup declaration (refused)"));
            } else if rest.starts_with(b"</") {
                self.end_tag()?;
            } else if rest.starts_with(b"<") {
                self.start_tag()?;
            } else {
                self.text()?;
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<Doc<'a>> {
        if !self.stack.is_empty() {
            return Err(Error::Truncated);
        }
        if self.elements.is_empty() {
            return Err(malformed("no element"));
        }
        Ok(Doc { src: self.src, elements: self.elements, namespaces: self.namespaces, packet_end: self.packet_end })
    }

    fn instruction(&mut self) -> Result<()> {
        let start = self.pos;
        self.pos += 2;
        let target = self.name_range()?;
        let end = self.find(self.pos, b"?>")?;
        self.count(1)?;
        let target = self.slice(target)?;
        if target == "xpacket" && self.stack.is_empty() && self.root_closed {
            let body = self.src.get(self.pos..end.saturating_sub(2)).unwrap_or_default();
            if body.trim_start().starts_with("end") {
                self.packet_end = Some(start..end);
            }
        }
        self.pos = end;
        Ok(())
    }

    fn cdata(&mut self) -> Result<()> {
        let from = self.pos + 9;
        let end = self.find(from, b"]]>")?;
        let Some(&top) = self.stack.last() else { return Err(malformed("a CDATA section outside the root element")) };
        self.count(1)?;
        let raw = self.slice(from..end - 3)?;
        let text = normalise_lines(raw);
        self.add_text(top, &text);
        self.pos = end;
        Ok(())
    }

    fn add_text(&mut self, el: usize, text: &str) {
        if let Some(e) = self.elements.get_mut(el) {
            e.has_text |= text.bytes().any(|b| !is_space(b));
            e.text.push_str(text);
        }
    }

    fn text(&mut self) -> Result<()> {
        let start = self.pos;
        let len = self.rest().iter().position(|&b| b == b'<').unwrap_or(self.rest().len());
        self.pos = start + len;
        let raw = self.slice(start..self.pos)?;
        match self.stack.last().copied() {
            None => {
                // Outside the root: only white space, NUL padding some writers leave, or a stray BOM.
                if !raw.chars().all(|c| c.is_ascii_whitespace() || c == '\0' || c == '\u{feff}') {
                    return Err(malformed("text outside the root element"));
                }
                Ok(())
            }
            Some(top) => {
                self.count(1)?;
                let text = decode(raw, false)?;
                self.add_text(top, &text);
                Ok(())
            }
        }
    }

    /// A name: bytes up to white space or a delimiter.
    fn name_range(&mut self) -> Result<Range<usize>> {
        let start = self.pos;
        let len = self
            .rest()
            .iter()
            .position(|&b| is_space(b) || matches!(b, b'/' | b'>' | b'=' | b'<' | b'"' | b'\'' | b'?' | b'&'))
            .unwrap_or(self.rest().len());
        if len == 0 {
            return Err(if self.pos >= self.bytes().len() { Error::Truncated } else { malformed("a missing name") });
        }
        self.pos += len;
        Ok(start..self.pos)
    }

    fn skip_space(&mut self) -> bool {
        let start = self.pos;
        while self.peek().is_some_and(is_space) {
            self.pos += 1;
        }
        self.pos > start
    }

    fn expect(&mut self, b: u8) -> Result<()> {
        match self.peek() {
            Some(x) if x == b => {
                self.pos += 1;
                Ok(())
            }
            Some(_) => Err(malformed("an unexpected character in a tag")),
            None => Err(Error::Truncated),
        }
    }

    fn lookup(&self, prefix: &str) -> Option<Option<usize>> {
        self.bindings.get(prefix).and_then(|v| v.last().copied())
    }

    fn start_tag(&mut self) -> Result<()> {
        let start = self.pos;
        self.pos += 1;
        let raw_name = self.name_range()?;
        let raw_name = self.slice(raw_name)?;
        let mut raw_attrs: Vec<(&'a str, String, Range<usize>)> = Vec::new();
        let empty = loop {
            let space_start = self.pos;
            let spaced = self.skip_space();
            match self.peek() {
                Some(b'>') => {
                    self.pos += 1;
                    break false;
                }
                Some(b'/') => {
                    self.pos += 1;
                    self.expect(b'>')?;
                    break true;
                }
                Some(_) if spaced => {
                    let name = self.name_range()?;
                    let name = self.slice(name)?;
                    self.skip_space();
                    self.expect(b'=')?;
                    self.skip_space();
                    let quote = match self.peek() {
                        Some(q @ (b'"' | b'\'')) => q,
                        Some(_) => return Err(malformed("an attribute value without quotes")),
                        None => return Err(Error::Truncated),
                    };
                    self.pos += 1;
                    let len = self.rest().iter().position(|&b| b == quote).ok_or(Error::Truncated)?;
                    let raw = self.slice(self.pos..self.pos + len)?;
                    if raw.contains('<') {
                        return Err(malformed("a '<' inside an attribute value"));
                    }
                    self.pos += len + 1;
                    raw_attrs.push((name, decode(raw, true)?, space_start..self.pos));
                }
                Some(_) => return Err(malformed("attributes not separated by white space")),
                None => return Err(Error::Truncated),
            }
        };
        if self.stack.is_empty() && (self.root_closed || !self.elements.is_empty()) {
            return Err(malformed("a second root element"));
        }
        if self.stack.len() >= MAX_DEPTH {
            return Err(Error::TooMany("XML nesting levels"));
        }
        self.count(raw_attrs.len().saturating_add(1))?;

        // Namespace declarations first: they apply to the element's own name and attributes.
        let mut seen: HashSet<&str> = HashSet::with_capacity(raw_attrs.len());
        let mut declared: Vec<&'a str> = Vec::new();
        for (name, value, _) in &raw_attrs {
            if !seen.insert(name) {
                return Err(malformed("a repeated attribute"));
            }
            let prefix = if *name == "xmlns" {
                ""
            } else if let Some(p) = name.strip_prefix("xmlns:") {
                if p.is_empty() || p == "xmlns" || p.contains(':') || value.is_empty() {
                    return Err(malformed("a bad namespace declaration"));
                }
                p
            } else {
                continue;
            };
            let binding = (!value.is_empty()).then(|| self.intern(value));
            self.bindings.entry(prefix).or_default().push(binding);
            declared.push(prefix);
        }
        let (prefix, local) = split_name(raw_name)?;
        let ns = match self.lookup(prefix) {
            Some(ns) => ns,
            None if prefix.is_empty() => None,
            None => return Err(malformed("an undeclared namespace prefix")),
        };
        let mut attrs = Vec::with_capacity(raw_attrs.len());
        let mut expanded: HashSet<(usize, &str)> = HashSet::new();
        for (raw, value, span) in raw_attrs {
            let is_xmlns = raw == "xmlns" || raw.starts_with("xmlns:");
            let name = if is_xmlns {
                Name { prefix: "xmlns", local: raw.strip_prefix("xmlns:").unwrap_or_default(), ns: None }
            } else {
                let (prefix, local) = split_name(raw)?;
                let ns = if prefix.is_empty() {
                    None
                } else {
                    match self.lookup(prefix) {
                        Some(Some(ns)) => Some(ns),
                        _ => return Err(malformed("an undeclared namespace prefix")),
                    }
                };
                if let Some(ns) = ns
                    && !expanded.insert((ns, local))
                {
                    return Err(malformed("a repeated attribute"));
                }
                Name { prefix, local, ns }
            };
            attrs.push(Attr { name, is_xmlns, value, span });
        }

        let index = self.elements.len();
        let parent = self.stack.last().copied();
        if let Some(p) = parent.and_then(|p| self.elements.get_mut(p)) {
            p.children.push(index);
        }
        let end = self.pos;
        self.elements.push(Element {
            name: Name { prefix, local, ns },
            raw_name,
            attrs,
            children: Vec::new(),
            parent,
            text: String::new(),
            has_text: false,
            span: start..end,
            open_end: end,
            close_start: end,
            empty,
        });
        if empty {
            self.unbind(&declared);
            if self.stack.is_empty() {
                self.root_closed = true;
            }
        } else {
            self.stack.push(index);
            self.declared.push(declared);
        }
        Ok(())
    }

    fn unbind(&mut self, prefixes: &[&'a str]) {
        for p in prefixes {
            if let Some(v) = self.bindings.get_mut(p) {
                v.pop();
            }
        }
    }

    fn end_tag(&mut self) -> Result<()> {
        let start = self.pos;
        self.pos += 2;
        let name = self.name_range()?;
        let name = self.slice(name)?;
        self.skip_space();
        self.expect(b'>')?;
        let top = self.stack.pop().ok_or_else(|| malformed("an end tag without its start tag"))?;
        let declared = self.declared.pop().unwrap_or_default();
        self.unbind(&declared);
        let end = self.pos;
        let el = self.elements.get_mut(top).ok_or_else(|| malformed("an end tag without its start tag"))?;
        if el.raw_name != name {
            return Err(malformed("an end tag that does not match its start tag"));
        }
        el.span.end = end;
        el.close_start = start;
        if self.stack.is_empty() {
            self.root_closed = true;
        }
        Ok(())
    }
}

/// `prefix:local` or `local`; at most one colon, neither part empty.
fn split_name(raw: &str) -> Result<(&str, &str)> {
    match raw.split_once(':') {
        None => Ok(("", raw)),
        Some((p, l)) if !p.is_empty() && !l.is_empty() && !l.contains(':') => Ok((p, l)),
        Some(_) => Err(malformed("a name with a bad prefix")),
    }
}

/// Line ends normalised to `\n` (XML 1.0 §2.11).
fn normalise_lines(raw: &str) -> String {
    if !raw.contains('\r') {
        return raw.to_owned();
    }
    raw.replace("\r\n", "\n").replace('\r', "\n")
}

/// Character data or an attribute value with references replaced, line ends normalised and, in an
/// attribute, literal tabs and line ends turned into spaces (XML 1.0 §3.3.3). Only the five
/// predefined entities and character references to characters XML allows are accepted.
fn decode(raw: &str, attribute: bool) -> Result<String> {
    let simple = !raw.contains(['&', '\r']) && !(attribute && raw.contains(['\t', '\n']));
    if simple {
        return Ok(raw.to_owned());
    }
    let raw = normalise_lines(raw);
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw.as_str();
    while let Some(at) = rest.find(['&', '\t', '\n']) {
        let (before, tail) = rest.split_at(at);
        out.push_str(before);
        let Some(c) = tail.chars().next() else { break };
        if c != '&' {
            out.push(if attribute { ' ' } else { c });
            rest = tail.get(1..).unwrap_or_default();
            continue;
        }
        let window = tail.get(..tail.len().min(MAX_REFERENCE)).unwrap_or(tail);
        let semi = window.find(';').ok_or_else(|| malformed("an unterminated reference"))?;
        let name = tail.get(1..semi).unwrap_or_default();
        let ch = match name {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "apos" => '\'',
            "quot" => '"',
            _ => {
                let code = if let Some(hex) = name.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(dec) = name.strip_prefix('#') {
                    dec.parse::<u32>().ok()
                } else {
                    return Err(malformed("an entity reference (refused)"));
                };
                code.and_then(char::from_u32).filter(|&c| is_xml_char(c)).ok_or_else(|| malformed("a bad character reference"))?
            }
        };
        out.push(ch);
        rest = tail.get(semi + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_references_and_line_ends() {
        assert_eq!(decode("a &lt;b&gt; &amp; &#65;&#x42;", false).unwrap(), "a <b> & AB");
        assert_eq!(decode("a\r\nb\rc", false).unwrap(), "a\nb\nc");
        assert_eq!(decode("a\tb\r\nc&#10;", true).unwrap(), "a b c\n");
        for bad in ["&foo;", "&#0;", "&#xFFFE;", "&amp", "&#xD800;", "&#99999999999;"] {
            assert!(decode(bad, false).is_err(), "{bad}");
        }
    }

    #[test]
    fn spans_and_namespaces() {
        let src = "<a xmlns:p=\"u\" p:x='1'><p:b q=\"2\"/>t<![CDATA[<c>]]></a>";
        let d = parse(src).unwrap();
        assert_eq!(d.elements.len(), 2);
        let a = &d.elements[0];
        assert_eq!((a.span.clone(), a.text.as_str(), a.has_text), (0..src.len(), "t<c>", true));
        assert_eq!(&src[a.attrs[1].span.clone()], " p:x='1'");
        assert_eq!(&src[a.close_start..], "</a>");
        let b = &d.elements[1];
        assert!(d.is(&b.name, "u", "b") && b.empty && b.parent == Some(0));
        assert_eq!(&src[b.span.clone()], "<p:b q=\"2\"/>");
        assert_eq!(d.attr(0, "u", "x"), Some("1"));
    }

    #[test]
    fn refuses_what_xmp_never_needs() {
        for bad in [
            "<!DOCTYPE a [<!ENTITY e \"x\">]><a>&e;</a>",
            "<a>&e;</a>",
            "<a><b></a></b>",
            "<a></a><b/>",
            "<a x='1' x='2'/>",
            "<a xmlns:p='u' xmlns:q='u' p:x='1' q:x='2'/>",
            "<p:a/>",
            "<a p:x='1'/>",
            "<a x='1'y='2'/>",
            "<a x=1/>",
            "text<a/>",
            "<a:b:c/>",
            "<a xmlns:p=''/>",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        for truncated in ["<a", "<a>", "<a x='1", "<!-- x", "<a><![CDATA[x</a>", "<?xpacket x", ""] {
            assert!(parse(truncated).is_err(), "{truncated}");
        }
    }

    #[test]
    fn default_namespaces_and_undeclaring() {
        let d = parse("<a xmlns='u'><b xmlns=''><c/></b></a>").unwrap();
        assert_eq!(d.ns(&d.elements[0].name), Some("u"));
        assert_eq!(d.ns(&d.elements[2].name), None);
    }
}
