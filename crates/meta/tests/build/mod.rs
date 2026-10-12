//! Builders of synthetic files for the `dalica-meta` tests: EXIF/TIFF structures, IPTC-IIM
//! streams, Photoshop resources, XMP packets, and the JPEG, PNG, WebP and TIFF containers around
//! them. Everything is built from the public specifications in the test; no real photo is used.

#![allow(dead_code)]

/// TIFF field types.
pub const BYTE: u16 = 1;
pub const ASCII: u16 = 2;
pub const SHORT: u16 = 3;
pub const LONG: u16 = 4;
pub const RATIONAL: u16 = 5;
pub const UNDEFINED: u16 = 7;
pub const SRATIONAL: u16 = 10;

/// One IFD entry to build.
#[derive(Clone, Debug)]
pub enum F {
    /// A value, encoded in the structure's byte order (inline when it fits in four bytes).
    Value { tag: u16, kind: u16, count: u32, words: Vec<Word> },
    /// An entry written exactly as given: its count and its value field (an offset or inline
    /// bytes), with nothing placed in the data area. For hostile structures.
    Raw { tag: u16, kind: u16, count: u32, field: u32 },
    /// A LONG holding the offset of these bytes, placed in the data area (strip offsets).
    OffsetTo { tag: u16, bytes: Vec<u8> },
}

/// One value unit, encoded by the builder in the right byte order.
#[derive(Clone, Copy, Debug)]
pub enum Word {
    U8(u8),
    U16(u16),
    U32(u32),
}

impl F {
    pub fn tag(&self) -> u16 {
        match self {
            F::Value { tag, .. } | F::Raw { tag, .. } | F::OffsetTo { tag, .. } => *tag,
        }
    }
}

pub fn ascii(tag: u16, s: &str) -> F {
    let mut b: Vec<Word> = s.bytes().map(Word::U8).collect();
    b.push(Word::U8(0));
    F::Value { tag, kind: ASCII, count: b.len() as u32, words: b }
}

pub fn ascii_bytes(tag: u16, s: &[u8]) -> F {
    let mut b: Vec<Word> = s.iter().copied().map(Word::U8).collect();
    b.push(Word::U8(0));
    F::Value { tag, kind: ASCII, count: b.len() as u32, words: b }
}

pub fn short(tag: u16, v: u16) -> F {
    F::Value { tag, kind: SHORT, count: 1, words: vec![Word::U16(v)] }
}

pub fn long(tag: u16, v: u32) -> F {
    F::Value { tag, kind: LONG, count: 1, words: vec![Word::U32(v)] }
}

pub fn rationals(tag: u16, v: &[(u32, u32)]) -> F {
    F::Value { tag, kind: RATIONAL, count: v.len() as u32, words: v.iter().flat_map(|&(n, d)| [Word::U32(n), Word::U32(d)]).collect() }
}

pub fn srational(tag: u16, n: i32, d: i32) -> F {
    F::Value { tag, kind: SRATIONAL, count: 1, words: vec![Word::U32(n as u32), Word::U32(d as u32)] }
}

pub fn undefined(tag: u16, bytes: &[u8]) -> F {
    F::Value { tag, kind: UNDEFINED, count: bytes.len() as u32, words: bytes.iter().copied().map(Word::U8).collect() }
}

pub fn bytes(tag: u16, bytes: &[u8]) -> F {
    F::Value { tag, kind: BYTE, count: bytes.len() as u32, words: bytes.iter().copied().map(Word::U8).collect() }
}

pub fn raw(tag: u16, kind: u16, count: u32, field: u32) -> F {
    F::Raw { tag, kind, count, field }
}

/// A TIFF structure: IFD0 with optional EXIF, GPS and Interoperability IFDs and IFD1. Pointer
/// entries to the sub-IFDs are added when they are not empty (unless IFD0 already has one).
#[derive(Clone, Debug, Default)]
pub struct Tiff {
    pub big_endian: bool,
    pub ifd0: Vec<F>,
    pub exif: Vec<F>,
    pub gps: Vec<F>,
    pub interop: Vec<F>,
    pub ifd1: Vec<F>,
}

const EXIF_IFD: u16 = 0x8769;
const GPS_IFD: u16 = 0x8825;
const INTEROP_IFD: u16 = 0xa005;

impl Tiff {
    pub fn new() -> Tiff {
        Tiff::default()
    }

    pub fn build(&self) -> Vec<u8> {
        let mut ifd0 = self.ifd0.clone();
        let mut exif = self.exif.clone();
        // Placeholders; patched once the offsets are known.
        if !self.interop.is_empty() && !exif.iter().any(|f| f.tag() == INTEROP_IFD) {
            exif.push(long(INTEROP_IFD, 0));
        }
        if !exif.is_empty() && !ifd0.iter().any(|f| f.tag() == EXIF_IFD) {
            ifd0.push(long(EXIF_IFD, 0));
        }
        if !self.gps.is_empty() && !ifd0.iter().any(|f| f.tag() == GPS_IFD) {
            ifd0.push(long(GPS_IFD, 0));
        }
        let table = |n: usize| 2 + 12 * n + 4;
        let at0 = 8usize;
        let at_exif = at0 + table(ifd0.len());
        let at_gps = at_exif + if exif.is_empty() { 0 } else { table(exif.len()) };
        let at_interop = at_gps + if self.gps.is_empty() { 0 } else { table(self.gps.len()) };
        let at_ifd1 = at_interop + if self.interop.is_empty() { 0 } else { table(self.interop.len()) };
        let data_at = at_ifd1 + if self.ifd1.is_empty() { 0 } else { table(self.ifd1.len()) };
        let patch = |fields: &mut Vec<F>, tag: u16, to: usize| {
            for f in fields.iter_mut() {
                if let F::Value { tag: t, words, .. } = f
                    && *t == tag
                    && matches!(words.as_slice(), [Word::U32(0)])
                {
                    *words = vec![Word::U32(to as u32)];
                }
            }
        };
        patch(&mut ifd0, EXIF_IFD, at_exif);
        patch(&mut ifd0, GPS_IFD, at_gps);
        patch(&mut exif, INTEROP_IFD, at_interop);

        let mut head = if self.big_endian { b"MM\0*".to_vec() } else { b"II*\0".to_vec() };
        head.extend(self.u32(at0 as u32));
        let mut tables = Vec::new();
        let mut data = Vec::new();
        let ifds: [(&[F], u32); 5] =
            [(&ifd0, if self.ifd1.is_empty() { 0 } else { at_ifd1 as u32 }), (&exif, 0), (&self.gps, 0), (&self.interop, 0), (&self.ifd1, 0)];
        for (fields, next) in ifds {
            if fields.is_empty() {
                continue;
            }
            let mut fields = fields.to_vec();
            fields.sort_by_key(F::tag);
            tables.extend(self.u16(fields.len() as u16));
            for f in &fields {
                match f {
                    F::Value { tag, kind, count, words } => {
                        let encoded: Vec<u8> = words.iter().flat_map(|w| self.word(*w)).collect();
                        tables.extend(self.u16(*tag));
                        tables.extend(self.u16(*kind));
                        tables.extend(self.u32(*count));
                        if encoded.len() <= 4 {
                            let mut inline = encoded.clone();
                            inline.resize(4, 0);
                            tables.extend(inline);
                        } else {
                            if data.len() % 2 == 1 {
                                data.push(0);
                            }
                            tables.extend(self.u32((data_at + data.len()) as u32));
                            data.extend(encoded);
                        }
                    }
                    F::Raw { tag, kind, count, field } => {
                        tables.extend(self.u16(*tag));
                        tables.extend(self.u16(*kind));
                        tables.extend(self.u32(*count));
                        tables.extend(self.u32(*field));
                    }
                    F::OffsetTo { tag, bytes } => {
                        tables.extend(self.u16(*tag));
                        tables.extend(self.u16(LONG));
                        tables.extend(self.u32(1));
                        if data.len() % 2 == 1 {
                            data.push(0);
                        }
                        tables.extend(self.u32((data_at + data.len()) as u32));
                        data.extend(bytes);
                    }
                }
            }
            tables.extend(self.u32(next));
        }
        [head, tables, data].concat()
    }

    fn word(&self, w: Word) -> Vec<u8> {
        match w {
            Word::U8(b) => vec![b],
            Word::U16(v) => self.u16(v).to_vec(),
            Word::U32(v) => self.u32(v).to_vec(),
        }
    }

    fn u16(&self, v: u16) -> [u8; 2] {
        if self.big_endian { v.to_be_bytes() } else { v.to_le_bytes() }
    }

    fn u32(&self, v: u32) -> [u8; 4] {
        if self.big_endian { v.to_be_bytes() } else { v.to_le_bytes() }
    }
}

/// An IPTC-IIM stream of `(record, number, data)` datasets.
pub fn iim(datasets: &[(u8, u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(record, number, data) in datasets {
        out.extend([0x1c, record, number]);
        if data.len() < 0x8000 {
            out.extend((data.len() as u16).to_be_bytes());
        } else {
            out.extend(0x8004u16.to_be_bytes());
            out.extend((data.len() as u32).to_be_bytes());
        }
        out.extend(data);
    }
    out
}

/// Photoshop image resources (`8BIM`, empty names) of `(id, data)`.
pub fn irb(resources: &[(u16, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(id, data) in resources {
        out.extend(b"8BIM");
        out.extend(id.to_be_bytes());
        out.extend([0, 0]);
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(data);
        if data.len() % 2 == 1 {
            out.push(0);
        }
    }
    out
}

/// An XMP packet around `body` (the content is opaque to these tests until M6.9).
pub fn xmp_packet(body: &str) -> Vec<u8> {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">{body}</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    )
    .into_bytes()
}

/// The image data every built container carries, so tests can see that it survives.
pub const PIXELS: &[u8] = b"\x12\x34 pretend entropy-coded scan data \x56\x78";

// JPEG.

pub const APP1: u8 = 0xe1;
pub const APP2: u8 = 0xe2;
pub const APP13: u8 = 0xed;
pub const COM: u8 = 0xfe;

/// SOI, an APP0, the segments, a frame header, a scan header, the scan data and EOI.
pub fn jpeg(segments: &[(u8, Vec<u8>)]) -> Vec<u8> {
    jpeg_with_scan(segments, PIXELS)
}

pub fn jpeg_with_scan(segments: &[(u8, Vec<u8>)], scan: &[u8]) -> Vec<u8> {
    let mut out = vec![0xff, 0xd8];
    let mut all = vec![(0xe0, b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0".to_vec())];
    all.extend(segments.iter().cloned());
    all.push((0xc0, vec![8, 0, 1, 0, 1, 1, 1, 0x11, 0]));
    for (marker, payload) in all {
        out.extend([0xff, marker]);
        out.extend(((payload.len() + 2) as u16).to_be_bytes());
        out.extend(payload);
    }
    out.extend([0xff, 0xda, 0, 8, 1, 1, 0, 0, 0x3f, 0]);
    out.extend(scan);
    out.extend([0xff, 0xd9]);
    out
}

pub fn app1_exif(tiff: &[u8]) -> (u8, Vec<u8>) {
    (APP1, [&b"Exif\0\0"[..], tiff].concat())
}

pub fn app1_xmp(packet: &[u8]) -> (u8, Vec<u8>) {
    (APP1, [&b"http://ns.adobe.com/xap/1.0/\0"[..], packet].concat())
}

pub fn app1_extended(guid: &str, full: u32, offset: u32, data: &[u8]) -> (u8, Vec<u8>) {
    (APP1, [&b"http://ns.adobe.com/xmp/extension/\0"[..], guid.as_bytes(), &full.to_be_bytes(), &offset.to_be_bytes(), data].concat())
}

pub fn app13(irb: &[u8]) -> (u8, Vec<u8>) {
    (APP13, [&b"Photoshop 3.0\0"[..], irb].concat())
}

pub fn app2_icc(profile: &[u8]) -> (u8, Vec<u8>) {
    (APP2, [&b"ICC_PROFILE\0\x01\x01"[..], profile].concat())
}

// PNG.

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

pub fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(data);
    out.extend(crc32(&[&kind[..], data].concat()).to_be_bytes());
    out
}

/// The signature, IHDR, the chunks, an IDAT holding [`PIXELS`] and IEND.
pub fn png(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
    for (kind, data) in chunks {
        out.extend(png_chunk(kind, data));
    }
    out.extend(png_chunk(b"IDAT", PIXELS));
    out.extend(png_chunk(b"IEND", &[]));
    out
}

pub fn text(keyword: &str, text: &[u8]) -> Vec<u8> {
    [keyword.as_bytes(), &[0], text].concat()
}

pub fn ztxt(keyword: &str, text: &[u8]) -> Vec<u8> {
    [keyword.as_bytes(), &[0, 0], &fdeflate::compress_to_vec(text)].concat()
}

pub fn itxt(keyword: &str, text: &[u8], compressed: bool) -> Vec<u8> {
    let body = if compressed { fdeflate::compress_to_vec(text) } else { text.to_vec() };
    [keyword.as_bytes(), &[0, u8::from(compressed), 0], b"en\0", b"\0", &body].concat()
}

/// The text of a "Raw profile type" chunk: name line, length, hex digits (36 bytes a line).
pub fn raw_profile(name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("\n{name}\n{:8}\n", bytes.len());
    for line in bytes.chunks(36) {
        for b in line {
            out.push_str(&format!("{b:02x}"));
        }
        out.push('\n');
    }
    out.into_bytes()
}

// WebP.

fn riff_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    out
}

/// An extended-format WebP: `VP8X` (flags for EXIF and XMP when present), the chunks, and a
/// `VP8L` image chunk holding [`PIXELS`].
pub fn webp(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut flags = 0u8;
    for (kind, _) in chunks {
        flags |= match *kind {
            b"EXIF" => 0x08,
            b"XMP " => 0x04,
            b"ICCP" => 0x20,
            _ => 0,
        };
    }
    let mut body = b"WEBP".to_vec();
    body.extend(riff_chunk(b"VP8X", &[flags, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
    for (kind, data) in chunks {
        body.extend(riff_chunk(kind, data));
    }
    body.extend(riff_chunk(b"VP8L", PIXELS));
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// A simple-format WebP: one image chunk and nothing else.
pub fn webp_simple() -> Vec<u8> {
    let mut body = b"WEBP".to_vec();
    body.extend(riff_chunk(b"VP8L", PIXELS));
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

// TIFF files.

/// A one-strip TIFF file: `tiff`'s IFDs plus the image tags of a strip holding [`PIXELS`].
pub fn tiff_file(mut tiff: Tiff) -> Vec<u8> {
    tiff.ifd0.extend([
        long(0x0100, PIXELS.len() as u32),
        long(0x0101, 1),
        short(0x0102, 8),
        short(0x0103, 1),
        short(0x0106, 1),
        F::OffsetTo { tag: 0x0111, bytes: PIXELS.to_vec() },
        short(0x0115, 1),
        long(0x0116, 1),
        long(0x0117, PIXELS.len() as u32),
    ]);
    tiff.build()
}
