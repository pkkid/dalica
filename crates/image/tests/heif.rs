//! HEIF / HEIC (`formats.md` §3) and AVIF tiles (§1). With the `heif` feature
//! (`cargo test -p dalica-image --features heif --test heif`): single pictures and grids,
//! rotation, mirror, crop, 8–12 bits, alpha, ICC and EXIF on lossless synthetic files, and the
//! unsupported cases. Without it: the header comes from the container (size, orientation, EXIF)
//! and decoding answers the tile text, with its hint; an unusable container is the tile. Either way,
//! AVIF is a format tile, box loops and oversized boxes are errors, and random bytes never panic.
//!
//! The synthetic-file writer (`testdata` below) is adapted from LightCraft
//! `crates/heif/src/testdata.rs` at revision 76b53778 (MIT OR Apache-2.0, Copyright (c) 2026
//! ArtCraft Team and the LightCraft contributors; `NOTICE`).

mod common;

use dalica_image::{DecodeOptions, Error, Format, Limits, decode, decode_as, formats, read_header, sniff};
use proptest::prelude::*;
use testdata::{Prop, Spec, build};

#[allow(dead_code)]
mod testdata {
    //! Synthetic HEIF files: lossless, so every decoded sample is known.
    //!
    //! Each coded picture is one 32 × 32 coding tree unit stored as PCM samples (H.265 7.3.8.7):
    //! the bitstream carries the YCbCr values verbatim, so the HEVC stage reproduces them exactly
    //! and a test checks everything after it (grids, colour, transforms, metadata) against values
    //! it chose. The few CABAC bins around the PCM data are written with a minimal arithmetic
    //! encoder (9.3.4.4). Larger images are grids of such pictures, as iPhones store photos.
    //! 4:2:0 or monochrome, 8 to 12 bits, a CTU per picture.

    /// Side of every coded picture.
    pub const TILE: u32 = 32;

    /// YCbCr samples at a luma position (chroma is taken at even positions, 4:2:0).
    pub type Sampler<'a> = &'a dyn Fn(u32, u32) -> [u16; 3];

    /// A property of the primary image.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Prop {
        /// `colr` of type `prof`.
        Icc(Vec<u8>),
        /// `colr` of type `nclx`: primaries, transfer, matrix, full range.
        Nclx(u16, u16, u16, bool),
        /// `irot`: quarter turns counter-clockwise.
        Irot(u8),
        /// `imir` with this axis.
        Imir(u8),
        /// `clap`: width, height, horizontal and vertical offset (whole pixels).
        Clap(u32, u32, i32, i32),
    }

    /// What [`build`] writes.
    pub struct Spec<'a> {
        /// Displayed (grid output) size, before transforms.
        pub width: u32,
        pub height: u32,
        pub bit_depth: u8,
        pub sample: Sampler<'a>,
        /// Properties of the primary image, in association order.
        pub props: Vec<Prop>,
        /// EXIF as a TIFF structure (written with HEIF's 4-byte offset header).
        pub exif: Option<Vec<u8>>,
        /// An alpha auxiliary image (monochrome, full range).
        pub alpha: Option<&'a dyn Fn(u32, u32) -> u16>,
    }

    impl<'a> Spec<'a> {
        pub fn new(width: u32, height: u32, sample: Sampler<'a>) -> Spec<'a> {
            Spec { width, height, bit_depth: 8, sample, props: Vec::new(), exif: None, alpha: None }
        }
    }

    #[derive(Default)]
    struct Bits {
        bytes: Vec<u8>,
        n: usize,
    }

    impl Bits {
        fn put(&mut self, v: u32, len: u32) {
            for i in (0..len).rev() {
                if self.n.is_multiple_of(8) {
                    self.bytes.push(0);
                }
                if (v >> i) & 1 == 1
                    && let Some(b) = self.bytes.last_mut()
                {
                    *b |= 0x80 >> (self.n % 8);
                }
                self.n += 1;
            }
        }

        fn ue(&mut self, v: u32) {
            let x = u64::from(v) + 1;
            let len = 64 - x.leading_zeros();
            self.put(0, len - 1);
            self.put((x & 0xffff_ffff) as u32, len);
        }

        fn align_zero(&mut self) {
            while !self.n.is_multiple_of(8) {
                self.put(0, 1);
            }
        }

        fn trailing(&mut self) {
            self.put(1, 1);
            self.align_zero();
        }
    }

    /// The CABAC arithmetic encoder of H.265 9.3.4.4, for the bins these pictures need.
    struct Cabac {
        low: u32,
        range: u32,
        first: bool,
        outstanding: u32,
    }

    impl Cabac {
        fn new() -> Cabac {
            Cabac { low: 0, range: 510, first: true, outstanding: 0 }
        }

        fn put_bit(&mut self, out: &mut Bits, b: u32) {
            if self.first {
                self.first = false;
            } else {
                out.put(b, 1);
            }
            while self.outstanding > 0 {
                out.put(1 - b, 1);
                self.outstanding -= 1;
            }
        }

        fn renorm(&mut self, out: &mut Bits) {
            while self.range < 256 {
                if self.low < 256 {
                    self.put_bit(out, 0);
                } else if self.low >= 512 {
                    self.low -= 512;
                    self.put_bit(out, 1);
                } else {
                    self.low -= 256;
                    self.outstanding += 1;
                }
                self.range <<= 1;
                self.low <<= 1;
            }
        }

        /// `part_mode` = PART_2Nx2N: the most probable symbol of a fresh context.
        fn part_mode_2nx2n(&mut self, out: &mut Bits) {
            let lps = [128, 176, 208, 240][((self.range >> 6) & 3) as usize];
            self.range -= lps;
            self.renorm(out);
        }

        /// A terminating bin equal to 1 (`pcm_flag`, `end_of_slice_segment_flag`), with the flush.
        fn terminate_one(&mut self, out: &mut Bits) {
            self.range -= 2;
            self.low += self.range;
            self.range = 2;
            self.renorm(out);
            self.put_bit(out, (self.low >> 9) & 1);
            out.put(((self.low >> 7) & 3) | 1, 2);
        }
    }

    fn nal(kind: u8, rbsp: &[u8]) -> Vec<u8> {
        let mut out = vec![kind << 1, 1];
        let mut zeros = 0;
        for &b in rbsp {
            if zeros >= 2 && b <= 3 {
                out.push(3);
                zeros = 0;
            }
            zeros = if b == 0 { zeros + 1 } else { 0 };
            out.push(b);
        }
        out
    }

    fn profile_idc(bit_depth: u8, mono: bool) -> u32 {
        match (bit_depth, mono) {
            (_, true) => 4, // range extensions
            (8, false) => 1,
            (9 | 10, false) => 2,
            _ => 4,
        }
    }

    fn ptl(w: &mut Bits, idc: u32) {
        w.put(0, 2);
        w.put(0, 1);
        w.put(idc, 5);
        w.put(1 << (31 - idc), 32); // compatible with itself
        w.put(0b1001, 4); // progressive, frame only
        w.put(0, 22);
        w.put(0, 21);
        w.put(0, 1);
        w.put(93, 8); // level 3.1
    }

    fn parameter_sets(bit_depth: u8, mono: bool) -> [Vec<u8>; 3] {
        let idc = profile_idc(bit_depth, mono);
        let bd = u32::from(bit_depth);
        let mut v = Bits::default();
        v.put(0, 4);
        v.put(3, 2);
        v.put(0, 6);
        v.put(0, 3);
        v.put(1, 1);
        v.put(0xffff, 16);
        ptl(&mut v, idc);
        v.put(0, 1); // vps_sub_layer_ordering_info_present_flag
        v.ue(0);
        v.ue(0);
        v.ue(0);
        v.put(0, 6); // vps_max_layer_id
        v.ue(0); // vps_num_layer_sets_minus1
        v.put(0, 1); // vps_timing_info_present_flag
        v.put(0, 1); // vps_extension_flag
        v.trailing();

        let mut s = Bits::default();
        s.put(0, 4);
        s.put(0, 3);
        s.put(1, 1);
        ptl(&mut s, idc);
        s.ue(0);
        s.ue(if mono { 0 } else { 1 });
        s.ue(TILE);
        s.ue(TILE);
        s.put(0, 1); // conformance_window_flag
        s.ue(bd - 8);
        s.ue(bd - 8);
        s.ue(4);
        s.put(1, 1);
        s.ue(0);
        s.ue(0);
        s.ue(0);
        s.ue(2); // log2_min_luma_coding_block_size_minus3: 32
        s.ue(0); // log2_diff_max_min_luma_coding_block_size: CTB 32
        s.ue(0); // log2_min_luma_transform_block_size_minus2: 4
        s.ue(3); // log2_diff_max_min_luma_transform_block_size: 32
        s.ue(0);
        s.ue(0);
        s.put(0, 1); // scaling_list_enabled_flag
        s.put(0, 1); // amp_enabled_flag
        s.put(0, 1); // sample_adaptive_offset_enabled_flag
        s.put(1, 1); // pcm_enabled_flag
        s.put(bd - 1, 4); // pcm_sample_bit_depth_luma_minus1
        s.put(bd - 1, 4); // pcm_sample_bit_depth_chroma_minus1
        s.ue(2); // log2_min_pcm_luma_coding_block_size_minus3: 32
        s.ue(0); // log2_diff_max_min_pcm_luma_coding_block_size
        s.put(1, 1); // pcm_loop_filter_disabled_flag
        s.ue(0); // num_short_term_ref_pic_sets
        s.put(0, 1); // long_term_ref_pics_present_flag
        s.put(0, 1); // sps_temporal_mvp_enabled_flag
        s.put(0, 1); // strong_intra_smoothing_enabled_flag
        s.put(0, 1); // vui_parameters_present_flag
        s.put(0, 1); // sps_extension_present_flag
        s.trailing();

        let mut p = Bits::default();
        p.ue(0);
        p.ue(0);
        p.put(0, 6); // dependent slices, output flag, 3 extra header bits, sign hiding
        p.put(0, 1); // cabac_init_present_flag
        p.ue(0);
        p.ue(0);
        p.ue(0); // init_qp_minus26 (se(0) is the same bits)
        p.put(0, 3); // constrained intra, transform skip, cu_qp_delta
        p.ue(0);
        p.ue(0);
        p.put(0, 1);
        p.put(0, 2);
        p.put(0, 1); // transquant_bypass_enabled_flag
        p.put(0, 1); // tiles_enabled_flag
        p.put(0, 1); // entropy_coding_sync_enabled_flag
        p.put(0, 1); // pps_loop_filter_across_slices_enabled_flag
        p.put(1, 1); // deblocking_filter_control_present_flag
        p.put(0, 1); // deblocking_filter_override_enabled_flag
        p.put(1, 1); // pps_deblocking_filter_disabled_flag
        p.put(0, 1);
        p.put(0, 1);
        p.ue(0);
        p.put(0, 1);
        p.put(0, 1);
        p.trailing();
        [nal(32, &v.bytes), nal(33, &s.bytes), nal(34, &p.bytes)]
    }

    /// One coded 32 × 32 picture: the `hvcC` payload and the item data (length-prefixed NAL).
    struct Coded {
        hvcc: Vec<u8>,
        data: Vec<u8>,
    }

    fn picture(bit_depth: u8, mono: bool, sample: &dyn Fn(u32, u32) -> [u16; 3]) -> Coded {
        let bd = u32::from(bit_depth);
        let mut w = Bits::default();
        w.put(1, 1); // first_slice_segment_in_pic_flag
        w.put(0, 1); // no_output_of_prior_pics_flag
        w.ue(0);
        w.ue(2); // I slice
        w.ue(0); // slice_qp_delta, se(0)
        w.trailing(); // byte_alignment()
        let mut c = Cabac::new();
        c.part_mode_2nx2n(&mut w);
        c.terminate_one(&mut w); // pcm_flag
        w.align_zero(); // pcm_alignment_zero_bit
        for y in 0..TILE {
            for x in 0..TILE {
                w.put(u32::from(sample(x, y)[0]), bd);
            }
        }
        if !mono {
            for k in 1..3 {
                for y in 0..TILE / 2 {
                    for x in 0..TILE / 2 {
                        w.put(u32::from(sample(2 * x, 2 * y)[k]), bd);
                    }
                }
            }
        }
        let mut c = Cabac::new();
        c.terminate_one(&mut w); // end_of_slice_segment_flag
        w.align_zero();
        let slice = nal(19, &w.bytes);

        let sets = parameter_sets(bit_depth, mono);
        let mut h = vec![1u8, profile_idc(bit_depth, mono) as u8];
        h.extend_from_slice(&(1u32 << (31 - profile_idc(bit_depth, mono))).to_be_bytes());
        h.extend_from_slice(&[
            0x90,
            0,
            0,
            0,
            0,
            0,
            93,
            0xf0,
            0,
            0xfc,
            0xfc | u8::from(!mono),
            0xf8 | (bit_depth - 8),
            0xf8 | (bit_depth - 8),
            0,
            0,
            0x0f,
            3,
        ]);
        for (kind, set) in [32u8, 33, 34].into_iter().zip(&sets) {
            h.push(0x80 | kind);
            h.extend_from_slice(&1u16.to_be_bytes());
            h.extend_from_slice(&(set.len() as u16).to_be_bytes());
            h.extend_from_slice(set);
        }
        let mut data = (slice.len() as u32).to_be_bytes().to_vec();
        data.extend_from_slice(&slice);
        Coded { hvcc: h, data }
    }

    pub fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(payload);
        v
    }

    fn full_box(kind: &[u8; 4], version: u8, payload: &[u8]) -> Vec<u8> {
        let mut p = vec![version, 0, 0, 0];
        p.extend_from_slice(payload);
        bx(kind, &p)
    }

    struct Item {
        id: u16,
        kind: &'static [u8; 4],
        data: Vec<u8>,
        /// Indices (1-based) into the property list, with the essential flag.
        props: Vec<(u8, bool)>,
        /// Not an image of its own (a grid tile).
        hidden: bool,
    }

    #[derive(Default)]
    struct File {
        items: Vec<Item>,
        props: Vec<Vec<u8>>,
        refs: Vec<(&'static [u8; 4], u16, Vec<u16>)>,
    }

    impl File {
        fn prop(&mut self, b: Vec<u8>) -> u8 {
            self.props.push(b);
            self.props.len() as u8
        }

        fn add(&mut self, kind: &'static [u8; 4], data: Vec<u8>, props: Vec<(u8, bool)>) -> u16 {
            let id = self.items.len() as u16 + 1;
            self.items.push(Item { id, kind, data, props, hidden: false });
            id
        }

        /// A coded image of `w` × `h` (a grid when larger than a tile); returns its item id.
        fn image(&mut self, w: u32, h: u32, bit_depth: u8, mono: bool, sample: &dyn Fn(u32, u32) -> [u16; 3]) -> u16 {
            let (cols, rows) = (w.div_ceil(TILE), h.div_ceil(TILE));
            let ispe = |w: u32, h: u32| {
                let mut p = w.to_be_bytes().to_vec();
                p.extend_from_slice(&h.to_be_bytes());
                full_box(b"ispe", 0, &p)
            };
            let mut hvcc_prop = None;
            let tile_ispe = self.prop(ispe(TILE, TILE));
            let mut tiles = Vec::new();
            for r in 0..rows {
                for c in 0..cols {
                    let coded = picture(bit_depth, mono, &|x, y| sample((c * TILE + x).min(w - 1), (r * TILE + y).min(h - 1)));
                    let hv = *hvcc_prop.get_or_insert_with(|| self.prop(bx(b"hvcC", &coded.hvcc)));
                    tiles.push(self.add(b"hvc1", coded.data, vec![(hv, true), (tile_ispe, false)]));
                }
            }
            if let ([only], true) = (tiles.as_slice(), w == TILE && h == TILE) {
                return *only;
            }
            for it in self.items.iter_mut().filter(|i| tiles.contains(&i.id)) {
                it.hidden = true;
            }
            let mut g = vec![0u8, 0, (rows - 1) as u8, (cols - 1) as u8];
            g.extend_from_slice(&(w as u16).to_be_bytes());
            g.extend_from_slice(&(h as u16).to_be_bytes());
            let grid_ispe = self.prop(ispe(w, h));
            let id = self.add(b"grid", g, vec![(grid_ispe, false)]);
            self.refs.push((b"dimg", id, tiles));
            id
        }

        fn write(self, primary: u16) -> Vec<u8> {
            let ftyp = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
            let mut hdlr = vec![0u8; 4];
            hdlr.extend_from_slice(b"pict");
            hdlr.extend_from_slice(&[0; 13]);
            let hdlr = full_box(b"hdlr", 0, &hdlr);
            let pitm = full_box(b"pitm", 0, &primary.to_be_bytes());
            let mut iinf = (self.items.len() as u16).to_be_bytes().to_vec();
            for it in &self.items {
                let mut e = vec![2, 0, 0, u8::from(it.hidden)]; // version 2, flags
                e.extend_from_slice(&it.id.to_be_bytes());
                e.extend_from_slice(&[0, 0]);
                e.extend_from_slice(it.kind);
                e.push(0);
                iinf.extend_from_slice(&bx(b"infe", &e));
            }
            let iinf = full_box(b"iinf", 0, &iinf);
            let mut iref = Vec::new();
            for (kind, from, to) in &self.refs {
                let mut r = from.to_be_bytes().to_vec();
                r.extend_from_slice(&(to.len() as u16).to_be_bytes());
                for t in to {
                    r.extend_from_slice(&t.to_be_bytes());
                }
                iref.extend_from_slice(&bx(kind, &r));
            }
            let iref = full_box(b"iref", 0, &iref);
            let ipco = bx(b"ipco", &self.props.concat());
            let mut ipma = (self.items.iter().filter(|i| !i.props.is_empty()).count() as u32).to_be_bytes().to_vec();
            for it in self.items.iter().filter(|i| !i.props.is_empty()) {
                ipma.extend_from_slice(&it.id.to_be_bytes());
                ipma.push(it.props.len() as u8);
                ipma.extend(it.props.iter().map(|&(i, essential)| i | if essential { 0x80 } else { 0 }));
            }
            let iprp = bx(b"iprp", &[ipco, full_box(b"ipma", 0, &ipma)].concat());
            let iloc = |base: u32| {
                let mut l = vec![0x44, 0x00];
                l.extend_from_slice(&(self.items.len() as u16).to_be_bytes());
                let mut offset = base;
                for it in &self.items {
                    l.extend_from_slice(&it.id.to_be_bytes());
                    l.extend_from_slice(&[0, 0, 0, 0, 0, 1]); // construction method 0, data ref 0, one extent
                    l.extend_from_slice(&offset.to_be_bytes());
                    l.extend_from_slice(&(it.data.len() as u32).to_be_bytes());
                    offset += it.data.len() as u32;
                }
                full_box(b"iloc", 1, &l)
            };
            let meta = |base| full_box(b"meta", 0, &[hdlr.clone(), pitm.clone(), iloc(base), iinf.clone(), iref.clone(), iprp.clone()].concat());
            let start = (ftyp.len() + meta(0).len() + 8) as u32;
            let mdat = bx(b"mdat", &self.items.iter().flat_map(|i| i.data.iter().copied()).collect::<Vec<u8>>());
            [ftyp, meta(start), mdat].concat()
        }
    }

    /// Writes the HEIF file `spec` describes.
    pub fn build(spec: &Spec<'_>) -> Vec<u8> {
        let mut f = File::default();
        let primary = f.image(spec.width, spec.height, spec.bit_depth, false, spec.sample);
        let mut assoc = Vec::new();
        for p in &spec.props {
            let b = match p {
                Prop::Icc(icc) => bx(b"colr", &[b"prof".as_slice(), icc].concat()),
                Prop::Nclx(pr, tc, mc, full) => {
                    let mut c = b"nclx".to_vec();
                    for v in [pr, tc, mc] {
                        c.extend_from_slice(&v.to_be_bytes());
                    }
                    c.push(if *full { 0x80 } else { 0 });
                    bx(b"colr", &c)
                }
                Prop::Irot(a) => bx(b"irot", &[a & 3]),
                Prop::Imir(a) => bx(b"imir", &[a & 1]),
                Prop::Clap(w, h, ho, vo) => {
                    let mut c = Vec::new();
                    for v in [*w as i32, 1, *h as i32, 1, *ho, 1, *vo, 1] {
                        c.extend_from_slice(&v.to_be_bytes());
                    }
                    bx(b"clap", &c)
                }
            };
            assoc.push((f.prop(b), true));
        }
        if let Some(it) = f.items.iter_mut().find(|i| i.id == primary) {
            it.props.extend(assoc.iter().copied());
        }
        if let Some(exif) = &spec.exif {
            let id = f.add(b"Exif", [&[0u8, 0, 0, 0][..], exif].concat(), Vec::new());
            f.refs.push((b"cdsc", id, vec![primary]));
        }
        if let Some(alpha) = spec.alpha {
            let id = f.image(spec.width, spec.height, spec.bit_depth, true, &|x, y| [alpha(x, y), 0, 0]);
            let auxc = f.prop(full_box(b"auxC", 0, b"urn:mpeg:hevc:2015:auxid:1\0"));
            if let Some(it) = f.items.iter_mut().find(|i| i.id == id) {
                it.props.push((auxc, true));
            }
            f.refs.push((b"auxl", id, vec![primary]));
        }
        f.write(primary)
    }
}

/// Grey samples with a position pattern (full-range luma, neutral chroma), so a pixel's value
/// says where it came from.
fn luma(x: u32, y: u32) -> u16 {
    ((x * 7 + y * 3) % 200 + 20) as u16
}

fn grey(x: u32, y: u32) -> [u16; 3] {
    [luma(x, y), 128, 128]
}

/// Full-range BT.601, as the grey tests read it back exactly.
const FULL: Prop = Prop::Nclx(1, 13, 6, true);

fn heic(width: u32, height: u32, props: Vec<Prop>) -> Vec<u8> {
    build(&Spec { props, ..Spec::new(width, height, &grey) })
}

/// A file of the brands given, then `rest`.
fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
    let mut p = major.to_vec();
    p.extend([0, 0, 0, 0]);
    for c in compatible {
        p.extend(*c);
    }
    testdata::bx(b"ftyp", &p)
}

#[test]
fn avif_is_a_format_tile_whatever_its_major_brand() {
    let l = Limits::default();
    let avif = [ftyp(b"avif", &[b"mif1", b"avif"]), testdata::bx(b"meta", &[0; 16])].concat();
    assert_eq!(sniff(&avif), Some(Format::Avif));
    assert!(!Format::Avif.supported());
    for r in [read_header(&avif, &l).map(|_| ()), decode(&avif, &l).map(|_| ())] {
        let e = r.unwrap_err();
        assert_eq!((e.clone(), e.reason()), (Error::Unsupported(Format::Avif), "AVIF files can't be displayed".into()));
    }
    assert_eq!(formats::embedded_thumbnail(Format::Avif, &avif, &l), Ok(None));
    // `mif1` major with an AV1 brand: sniffed as HEIF, refused as AVIF.
    let mif1 = [ftyp(b"mif1", &[b"mif1", b"avif", b"miaf"]), testdata::bx(b"meta", &[0; 16])].concat();
    assert_eq!(sniff(&mif1), Some(Format::Heif));
    assert_eq!(read_header(&mif1, &l), Err(Error::Unsupported(Format::Avif)));
    assert_eq!(decode(&mif1, &l), Err(Error::Unsupported(Format::Avif)));
}

/// A TIFF-structured EXIF block: taken 2026-06-04 19:42:10, orientation 6, for a 64 × 32 photo.
fn dated_exif() -> Vec<u8> {
    let exif = dalica_scenes::Exif {
        orientation: 6,
        make: Some("Dalica".into()),
        date_time_original: "2026-06-04T19:42:10".parse().ok(),
        ..Default::default()
    };
    dalica_scenes::exif::tiff(&exif, 64, 32, None).unwrap()
}

/// Without the feature a HEIC is listed like any photo: its header comes from the container
/// (the primary item's `ispe` size, `irot`/`imir` as the orientation, the `Exif` item), while
/// decoding answers the tile text, with the hint under it.
#[cfg(not(feature = "heif"))]
#[test]
fn without_the_feature_heic_reads_its_container_and_shows_the_tile() {
    let l = Limits::default();
    let exif = dated_exif();
    let file = build(&Spec { exif: Some(exif.clone()), props: vec![FULL, Prop::Irot(3)], ..Spec::new(64, 32, &grey) });
    assert_eq!(sniff(&file), Some(Format::Heif));
    assert!(!Format::Heif.supported());
    let h = read_header(&file, &l).unwrap();
    // Stored 64 × 32, turned three quarters anticlockwise to show: EXIF orientation 6.
    assert_eq!((h.format, h.width, h.height, h.orientation.get(), h.display_size()), (Format::Heif, 64, 32, 6, (32, 64)));
    let e = h.exif.as_ref().unwrap();
    assert_eq!((e.date_time_original, e.make.as_deref(), h.thumbnail.clone()), ("2026-06-04T19:42:10".parse().ok(), Some("Dalica"), None));
    for r in [
        decode(&file, &l).map(|_| ()),
        dalica_image::decode_thumbnail(&file, 128, &l).map(|_| ()),
        formats::embedded_thumbnail(Format::Heif, &file, &l).map(|_| ()),
    ] {
        let e = r.unwrap_err();
        assert_eq!((e.clone(), e.reason()), (Error::Unsupported(Format::Heif), "HEIC support isn't included in this build".into()));
    }
    assert_eq!(formats::hint(Format::Heif), Some(formats::heif::HINT));
    assert_eq!(formats::hint_for(formats::heif::REASON), Some(formats::heif::HINT));
    assert!(Format::ALL.into_iter().filter(|&f| f != Format::Heif).all(|f| formats::hint(f).is_none()));
    // The container's transforms, in association order; the EXIF orientation is not one of them.
    for (props, orientation) in [(vec![], 1), (vec![Prop::Irot(1)], 8), (vec![Prop::Imir(0)], 4), (vec![Prop::Irot(1), Prop::Imir(1)], 7)] {
        let h = read_header(&build(&Spec { exif: Some(exif.clone()), props, ..Spec::new(64, 32, &grey) }), &l).unwrap();
        assert_eq!(h.orientation.get(), orientation);
    }
    // Without an `Exif` item: the size alone.
    let bare = read_header(&heic(64, 64, vec![FULL]), &l).unwrap();
    assert_eq!((bare.width, bare.height, bare.exif), (64, 64, None));
}

/// A container the reader cannot use answers the tile text, never "Damaged file": no `meta` or
/// an empty one, no `ispe`, a damaged box, an `Exif` item past the end of a file that is all
/// there. Bytes that stop inside a box are a partial read: read on.
#[cfg(not(feature = "heif"))]
#[test]
fn without_the_feature_an_unusable_container_is_the_tile() {
    let l = Limits::default();
    let tile = Err(Error::Unsupported(Format::Heif));
    let single = build(&Spec { exif: Some(dated_exif()), props: vec![FULL], ..Spec::new(testdata::TILE, testdata::TILE, &grey) });
    let relabel = |from: &[u8; 4], to: &[u8; 4]| {
        let mut b = single.clone();
        let at = b.windows(4).position(|w| w == from).unwrap();
        b[at..at + 4].copy_from_slice(to);
        b
    };
    let bare = [ftyp(b"heic", &[b"mif1", b"heic"]), testdata::bx(b"meta", &[0; 4])].concat();
    let mut stuck = single.clone();
    let ipco = stuck.windows(4).position(|w| w == b"ipco").unwrap() + 4;
    stuck[ipco..ipco + 4].copy_from_slice(&3u32.to_be_bytes());
    // The `mdat` box made to end before the EXIF item's extent does (the file is all there).
    let mdat = single.windows(4).position(|w| w == b"mdat").unwrap() - 4;
    let short = [&single[..mdat], &testdata::bx(b"mdat", &single[mdat + 8..single.len() - 8])].concat();
    for (case, bytes) in [
        ("no meta", [ftyp(b"heic", &[b"mif1", b"heic"]), testdata::bx(b"free", b"")].concat()),
        ("empty meta", bare),
        ("no ispe", relabel(b"ispe", b"ispf")),
        ("no pitm", relabel(b"pitm", b"pitn")),
        ("a box that never advances", stuck),
        ("EXIF past the end", short.clone()),
    ] {
        assert_eq!(read_header(&bytes, &l).map(|_| ()), tile, "{case}");
        assert_eq!(decode(&bytes, &l).map(|_| ()), tile, "{case}");
    }
    // Cut inside `mdat`, before the end of the EXIF item: read on, and the whole file has it.
    assert_eq!(read_header(&single[..single.len() - 8], &l), Err(Error::Truncated));
    assert!(read_header(&single, &l).unwrap().exif.is_some());
}

/// Planted by fuzzing in the sibling app: a malformed box that made heic-rs 0.1.1 slice out of
/// range. Whatever stage stops it, it is a damaged file.
const HEIC_RS_BOX_PANIC: [u8; 72] = [
    0x00, 0x00, 0x00, 0x24, 0x66, 0x74, 0x79, 0x70, 0x68, 0x65, 0x69, 0x63, 0x00, 0x00, 0x00, 0x00, 0x6d, 0x69, 0x66, 0x31, 0x4d, 0x69, 0x50, 0x72,
    0x6d, 0x69, 0x61, 0x66, 0x4d, 0x69, 0x48, 0x42, 0x72, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x6d, 0x65, 0x74, 0x61, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x10, 0x75, 0x75, 0x69, 0x64, 0x00, 0x00, 0x00, 0x00, 0x03, 0x08, 0x08, 0x08, 0x00, 0x03, 0x01, 0x03, 0x70, 0x00, 0xa8, 0x00,
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(common::cases(256)))]

    /// Truncation and corruption anywhere in a real file never panic, with or without the feature.
    #[test]
    fn corrupted_files_never_panic(cut in any::<prop::sample::Index>(), flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8)) {
        let mut bytes = build(&Spec { exif: Some(dated_exif()), props: vec![FULL, Prop::Irot(1)], ..Spec::new(64, 32, &grey) });
        for (at, v) in flips {
            let i = at.index(bytes.len());
            bytes[i] ^= v;
        }
        bytes.truncate(cut.index(bytes.len() + 1));
        let l = Limits::default();
        let _ = read_header(&bytes, &l);
        let _ = decode(&bytes, &l);
        let _ = decode_as(Format::Heif, &bytes, &DecodeOptions::default(), &l);
        let _ = formats::embedded_thumbnail(Format::Heif, &bytes, &l);
    }
}

#[test]
fn the_planted_heic_rs_panic_is_a_damaged_file() {
    for r in [read_header(&HEIC_RS_BOX_PANIC, &Limits::default()).map(|_| ()), decode(&HEIC_RS_BOX_PANIC, &Limits::default()).map(|_| ())] {
        let e = r.unwrap_err();
        assert!(e.reason() == "Damaged file" || e == Error::Unsupported(Format::Heif), "{e:?}");
    }
}

#[cfg(feature = "heif")]
mod with_the_feature {
    use dalica_image::formats::heif;
    use dalica_image::{Budget, ColorModel, DecodeOptions, Error, Format, Limits, Rgba8, Samples, decode, decode_with, formats, read_header, sniff};
    use dalica_scenes::icc::{Space, Version, rgb};

    use super::testdata::{Prop, Spec, TILE, build, bx};
    use super::{FULL, grey, heic, luma};

    fn l() -> Limits {
        Limits::default()
    }

    /// The grey value at `(x, y)` of a decoded image.
    fn at(img: &Rgba8, x: u32, y: u32) -> u8 {
        img.data[((y * img.width + x) * 4) as usize]
    }

    /// Every pixel of `img` equals the source luma at `source(x, y)`, within one level.
    fn check(case: &str, img: &Rgba8, source: impl Fn(u32, u32) -> (u32, u32)) {
        for y in 0..img.height {
            for x in 0..img.width {
                let (sx, sy) = source(x, y);
                let want = i32::from(luma(sx, sy));
                let got = i32::from(at(img, x, y));
                assert!((got - want).abs() <= 1, "{case}: ({x}, {y}) is {got}, source ({sx}, {sy}) is {want}");
                let i = ((y * img.width + x) * 4) as usize;
                assert_eq!(
                    (img.data[i + 1], img.data[i + 2], img.data[i + 3]),
                    (img.data[i], img.data[i], 255),
                    "{case}: grey and opaque at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn single_pictures_and_grids_decode_exactly() {
        assert!(Format::Heif.supported());
        for (w, h) in [(TILE, TILE), (64, 64), (96, 64), (80, 48), (40, 72)] {
            let file = heic(w, h, vec![FULL]);
            assert_eq!(sniff(&file), Some(Format::Heif));
            let hd = read_header(&file, &l()).unwrap();
            assert_eq!((hd.format, hd.width, hd.height, hd.orientation.get()), (Format::Heif, w, h, 1), "{w} × {h}");
            let img = decode(&file, &l()).unwrap();
            assert_eq!((img.width, img.height), (w, h));
            check(&format!("grid {w} × {h}"), &img, |x, y| (x, y));
            assert_eq!(formats::embedded_thumbnail(Format::Heif, &file, &l()), Ok(None));
        }
    }

    /// `irot` turns counter-clockwise in quarter turns; the header gives the turned size.
    #[test]
    fn rotation() {
        let (w, h) = (64, 32);
        for turns in 0..4u8 {
            let file = heic(w, h, vec![FULL, Prop::Irot(turns)]);
            let hd = read_header(&file, &l()).unwrap();
            let size = if turns % 2 == 1 { (h, w) } else { (w, h) };
            assert_eq!((hd.width, hd.height), size, "irot {turns}");
            let img = decode(&file, &l()).unwrap();
            assert_eq!((img.width, img.height), size);
            check(&format!("irot {turns}"), &img, |x, y| match turns {
                0 => (x, y),
                1 => (w - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (y, h - 1 - x),
            });
        }
    }

    /// `imir` axis 0 flips top and bottom, axis 1 left and right; transforms apply in
    /// association order (rotate, then mirror).
    #[test]
    fn mirror_and_order() {
        let (w, h) = (64, 32);
        let img = decode(&heic(w, h, vec![FULL, Prop::Imir(0)]), &l()).unwrap();
        check("imir 0", &img, |x, y| (x, h - 1 - y));
        let img = decode(&heic(w, h, vec![FULL, Prop::Imir(1)]), &l()).unwrap();
        check("imir 1", &img, |x, y| (w - 1 - x, y));
        // A quarter turn counter-clockwise (32 × 64), then left and right exchanged.
        let img = decode(&heic(w, h, vec![FULL, Prop::Irot(1), Prop::Imir(1)]), &l()).unwrap();
        assert_eq!((img.width, img.height), (h, w));
        check("irot 1 then imir 1", &img, |x, y| (w - 1 - y, h - 1 - x));
    }

    #[test]
    fn clean_aperture_crops() {
        let file = heic(64, 64, vec![FULL, Prop::Clap(32, 16, 4, -6)]);
        let hd = read_header(&file, &l()).unwrap();
        assert_eq!((hd.width, hd.height), (32, 16));
        let img = decode(&file, &l()).unwrap();
        // The centre moves by the offsets: left = (64 - 32) / 2 + 4, top = (64 - 16) / 2 - 6.
        check("clap", &img, |x, y| (x + 20, y + 18));
    }

    #[test]
    fn deeper_samples_decode_to_16_bits() {
        for depth in [10u8, 12] {
            let shift = depth - 8;
            let deep = move |x: u32, y: u32| {
                let [yy, cb, cr] = grey(x, y);
                [yy << shift, cb << shift, cr << shift]
            };
            let file = build(&Spec { bit_depth: depth, props: vec![FULL], ..Spec::new(64, 32, &deep) });
            let hd = read_header(&file, &l()).unwrap();
            let mut budget = Budget::new(&l(), hd.width, hd.height);
            let d = formats::decoder(Format::Heif)(&file, &hd, &mut budget, &l()).unwrap();
            assert!(matches!(d.samples, Samples::U16(_)), "{depth}-bit");
            let img = decode(&file, &l()).unwrap();
            check(&format!("{depth}-bit"), &img, |x, y| (x, y));
        }
    }

    #[test]
    fn alpha_is_carried() {
        let alpha = |x: u32, _y: u32| (x * 4) as u16;
        let file = build(&Spec { alpha: Some(&alpha), props: vec![FULL], ..Spec::new(64, 32, &grey) });
        let img = decode(&file, &l()).unwrap();
        for x in 0..64 {
            assert_eq!(img.data[(x * 4 + 3) as usize], (x * 4) as u8, "alpha at {x}");
        }
        assert_eq!(at(&img, 5, 3), luma(5, 3) as u8);
    }

    /// The ICC profile reaches the colour stage; with colour management off it is ignored.
    #[test]
    fn the_profile_is_applied() {
        let red = |_x: u32, _y: u32| -> [u16; 3] {
            // Saturated red in full-range BT.601 YCbCr.
            [76, 85, 255]
        };
        let p3 = rgb(Space::DisplayP3, Version::V4);
        let file = build(&Spec { props: vec![FULL, Prop::Icc(p3.clone())], ..Spec::new(32, 32, &red) });
        let hd = read_header(&file, &l()).unwrap();
        let mut budget = Budget::new(&l(), 32, 32);
        let d = formats::decoder(Format::Heif)(&file, &hd, &mut budget, &l()).unwrap();
        assert_eq!((d.model, d.icc.as_deref()), (ColorModel::Rgb, Some(p3.as_slice())));
        let managed = decode(&file, &l()).unwrap();
        let plain = decode_with(&file, &DecodeOptions { color_managed: false }, &l()).unwrap();
        assert_ne!(managed.data[..3], plain.data[..3], "the P3 profile changes saturated red");
        assert!(plain.data[0] >= 250 && plain.data[1] <= 4, "{:?}", &plain.data[..4]);
    }

    /// Without `colr nclx` the colour comes from the VUI, else limited range: luma 16 is black and
    /// 235 white.
    #[test]
    fn without_nclx_samples_are_limited_range() {
        let ramp = |x: u32, _y: u32| -> [u16; 3] { [if x < 16 { 16 } else { 235 }, 128, 128] };
        let img = decode(&build(&Spec::new(32, 32, &ramp)), &l()).unwrap();
        assert_eq!((at(&img, 0, 0), at(&img, 31, 31)), (0, 255));
    }

    #[test]
    fn exif_is_read_but_the_header_stays_upright() {
        let exif = dalica_scenes::exif::tiff(
            &dalica_scenes::Exif {
                orientation: 6,
                make: Some("Dalica".into()),
                date_time_original: "2026-06-04T19:42:10".parse().ok(),
                ..Default::default()
            },
            64,
            32,
            None,
        )
        .unwrap();
        let file = build(&Spec { exif: Some(exif.clone()), props: vec![FULL, Prop::Irot(3)], ..Spec::new(64, 32, &grey) });
        let hd = read_header(&file, &l()).unwrap();
        let e = hd.exif.as_ref().unwrap();
        assert_eq!((e.orientation, e.make.as_deref()), (Some(6), Some("Dalica")));
        assert_eq!((hd.orientation.get(), hd.width, hd.height, hd.thumbnail.clone()), (1, 32, 64, None));
        let img = decode(&file, &l()).unwrap();
        assert_eq!((img.width, img.height), (32, 64));
        // A single picture whose EXIF item lies past the bytes read so far: read on, rather than
        // a header without its dates.
        let single = build(&Spec { exif: Some(exif.clone()), props: vec![FULL], ..Spec::new(TILE, TILE, &grey) });
        let cut = single.len() - exif.len() / 2;
        assert_eq!(read_header(&single[..cut], &l()), Err(Error::Truncated));
        assert!(read_header(&single, &l()).unwrap().exif.is_some());
    }

    /// Image sequences, overlays, AV1 items and 4:4:4 streams answer the HEIF tile text.
    #[test]
    fn unsupported_variants_answer_unsupported() {
        let unsupported = Error::Unsupported(Format::Heif);
        assert_eq!(unsupported.reason(), heif::REASON);
        assert_eq!(heif::REASON, "This kind of HEIC file can't be displayed");
        // A sequence: `moov`, no `meta`.
        let seq = [bx(b"ftyp", b"msf1\0\0\0\0msf1hevc"), bx(b"moov", &bx(b"mvhd", &[0; 100])), bx(b"mdat", b"")].concat();
        assert_eq!(sniff(&seq), Some(Format::Heif));
        assert_eq!(read_header(&seq, &l()), Err(unsupported.clone()));
        assert_eq!(decode(&seq, &l()), Err(unsupported.clone()));
        // The grid item relabelled as an overlay, then as AV1.
        let grid = heic(64, 64, vec![FULL]);
        let at = grid.windows(4).position(|w| w == b"grid").unwrap();
        let mut overlay = grid.clone();
        overlay[at..at + 4].copy_from_slice(b"iovl");
        assert_eq!(read_header(&overlay, &l()), Err(unsupported.clone()));
        let mut av1 = grid.clone();
        av1[at..at + 4].copy_from_slice(b"av01");
        assert_eq!(read_header(&av1, &l()), Err(Error::Unsupported(Format::Avif)));
        // 4:4:4 (the hvcC chroma format byte), which heic-rs 0.1.1 would decode wrongly.
        let single = heic(TILE, TILE, vec![FULL]);
        let hvcc = single.windows(4).position(|w| w == b"hvcC").unwrap() + 4;
        let mut yuv444 = single.clone();
        yuv444[hvcc + 16] = 0xff;
        assert_eq!(read_header(&yuv444, &l()), Err(unsupported.clone()));
        assert_eq!(decode(&yuv444, &l()), Err(unsupported));
    }

    /// Our walk refuses broken structure before heic-rs sees it.
    #[test]
    fn box_loops_and_oversized_boxes_are_errors() {
        let file = heic(64, 64, vec![FULL]);
        let meta = file.windows(4).position(|w| w == b"meta").unwrap() - 4;
        // meta claiming more than the file: truncated (a partial read reads on).
        let mut over = file.clone();
        over[meta..meta + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(read_header(&over, &l()), Err(Error::Truncated));
        // A property box whose size never advances.
        let ipco = file.windows(4).position(|w| w == b"ipco").unwrap() + 4;
        let mut stuck = file.clone();
        stuck[ipco..ipco + 4].copy_from_slice(&3u32.to_be_bytes());
        assert_eq!(read_header(&stuck, &l()).unwrap_err().reason(), "Damaged file");
        // A child larger than its parent.
        let mut big = file.clone();
        big[ipco..ipco + 4].copy_from_slice(&0x0010_0000u32.to_be_bytes());
        assert!(read_header(&big, &l()).is_err());
        // No meta at all.
        let bare = [bx(b"ftyp", b"heic\0\0\0\0mif1heic"), bx(b"free", b"")].concat();
        assert!(matches!(read_header(&bare, &l()), Err(Error::Malformed(_))));
    }

    #[test]
    fn declared_sizes_are_checked_before_decoding() {
        let file = heic(96, 64, vec![FULL]);
        let tight = Limits { max_pixels: 96 * 64 - 1, ..Limits::default() };
        assert!(matches!(decode(&file, &tight), Err(Error::TooLarge { .. })));
        let small_budget = Limits { max_alloc: 1000, ..Limits::default() };
        assert!(matches!(decode(&file, &small_budget), Err(Error::TooLarge { .. })));
        assert!(decode(&file, &l()).is_ok());
    }

    /// Every prefix: `Ok` with the full size, or `Err`; never a panic.
    #[test]
    fn truncation_at_every_offset_never_panics() {
        let file = heic(64, 32, vec![FULL, Prop::Irot(1)]);
        for len in 0..file.len() {
            let cut = &file[..len];
            if let Ok(h) = read_header(cut, &l()) {
                assert_eq!((h.width, h.height), (32, 64), "prefix {len}");
            }
            let _ = decode(cut, &l());
        }
    }
}
