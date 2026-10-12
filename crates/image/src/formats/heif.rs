//! HEIF / HEIC (`formats.md` §3): iPhone and Mac photos, through `heic-rs` behind the non-default
//! `heif` feature, in a panic-guarded wrapper of our own. Without the feature HEIC files are still
//! recognised and listed, with the tile text [`REASON`].
//!
//! Adapted from LightCraft `crates/heif/src/lib.rs` and `crates/heif/src/vui.rs` at revision
//! 76b53778 (MIT OR Apache-2.0, Copyright (c) 2026 ArtCraft Team and the LightCraft contributors;
//! `NOTICE`): the use of heic-rs' container and HEVC stages, the colour description taken from
//! `colr nclx`, else the HEVC VUI (iPhones label their YCbCr full range only there), else BT.601
//! limited range; `imir` axis 0 flipping top and bottom as the files' writers mean it; refusing
//! 4:2:2 and 4:4:4 streams, which heic-rs 0.1.1 decodes wrongly; and the VUI reader. Dalica's own
//! parts: the bounded ISOBMFF walk of `formats/bmff.rs` checks the structure before heic-rs sees
//! it, sizes and buffers go through [`Limits`] and the [`Budget`], and the output is a
//! [`Decoded`] value for the colour stage.
//!
//! Single pictures and grid-tiled photos, monochrome and 4:2:0, 8 to 16 bits (deeper than 8 bits
//! decode to 16-bit samples), an alpha auxiliary image, the container's `clap`/`irot`/`imir`
//! (applied here, in association order, so the decoded pixels are upright), the ICC profile (a
//! grid's may sit on its tiles) and EXIF for the header. Image sequences, overlays, other codecs
//! and 4:2:2/4:4:4 answer [`Error::Unsupported`], never wrong pixels. A file whose brands or
//! primary item say AV1 is AVIF ([`Format::Avif`], never decoded). HEIC thumbnails are HEVC too,
//! so there is no embedded thumbnail for the fast path. Single-threaded: Dalica decodes on its own
//! worker threads.

use std::ops::Range;

use crate::decode::Decoded;
use crate::formats::bmff;
use crate::limits::{Budget, Limits};
use crate::{Error, Format, Header, Result};

/// Whether this build decodes HEIF / HEIC files.
pub const DECODES: bool = cfg!(feature = "heif");

/// The tile text for a HEIF / HEIC file this build does not show.
pub const REASON: &str =
    if cfg!(feature = "heif") { "This kind of HEIC file can't be displayed" } else { "HEIC support isn't included in this build" };

/// Brands that promise HEVC-coded images.
const HEVC_BRANDS: [[u8; 4]; 6] = [*b"heic", *b"heix", *b"heim", *b"heis", *b"hevc", *b"hevx"];

/// Whether the `ftyp` box says AVIF (an AV1 brand and no HEVC one), whatever the major brand.
fn avif_brands(bytes: &[u8]) -> bool {
    let compatible = bmff::compatible_brands(bytes);
    let major = bmff::major_brand(bytes);
    let hevc = compatible.iter().chain(major.as_ref()).any(|b| HEVC_BRANDS.contains(b));
    !hevc && compatible.iter().chain(major.as_ref()).any(|b| b == b"avif" || b == b"avis")
}

pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    if avif_brands(bytes) {
        return Err(Error::Unsupported(Format::Avif));
    }
    #[cfg(feature = "heif")]
    return crate::decode::guard(Format::Heif, || imp::header(bytes, limits));
    #[cfg(not(feature = "heif"))]
    {
        let _ = limits;
        Err(Error::Unsupported(Format::Heif))
    }
}

pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    if avif_brands(bytes) {
        return Err(Error::Unsupported(Format::Avif));
    }
    #[cfg(feature = "heif")]
    return crate::decode::guard(Format::Heif, || imp::decode(bytes, header, budget, limits));
    #[cfg(not(feature = "heif"))]
    {
        let _ = (header, budget, limits);
        Err(Error::Unsupported(Format::Heif))
    }
}

/// HEIC files carry no thumbnail Dalica decodes without the HEVC decoder (`formats.md` §1).
pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    header(bytes, limits).map(|_| None)
}

#[cfg(feature = "heif")]
mod imp {
    use heic_rs::context::Context;
    use heic_rs::hevc::{ChromaFormat, Frame};
    use heic_rs::props::colr::{MatrixCoefficients, Nclx, Range as SignalRange};
    use heic_rs::props::simple::Mirror;
    use heic_rs::props::{ItemProps, Transform};
    use heic_rs::{Image, PixelLayout, transform};

    use super::vui;
    use crate::decode::{ColorModel, Decoded, Samples};
    use crate::formats::bmff::{BoxRef, Boxes};
    use crate::limits::{Budget, Limits, Work};
    use crate::{Error, Format, Header, Orientation, Result};

    const UNSUPPORTED: Error = Error::Unsupported(Format::Heif);

    /// Bytes per pixel charged for each decoded picture's planes (16-bit samples, 4:2:0 or grey).
    const PLANE_BYTES: u64 = 3;

    fn err(e: heic_rs::Error) -> Error {
        use heic_rs::Error as H;
        match e {
            H::Truncated(_) => Error::Truncated,
            // No `meta`: an image sequence. No `hvcC`: an overlay, an identity derivation or
            // another codec.
            H::Unsupported(_) | H::MissingBox("meta" | "hvcC") => UNSUPPORTED,
            H::CyclicDerivation(_) => Error::Loop,
            H::PixelLimit { .. } => Error::TooLarge { width: 0, height: 0 },
            H::BoxTooLarge { .. } => Error::TooMany("bytes in one box"),
            H::Io(kind) => Error::Io(kind.to_string()),
            e => Error::Malformed(format!("HEIF: {e}")),
        }
    }

    /// Our own bounded walk of the structure before heic-rs reads it: the top level up to `meta`
    /// and the containers inside `meta`. A file with `moov` and no `meta` is an image sequence.
    fn structure(bytes: &[u8], limits: &Limits) -> Result<()> {
        let mut work = Work::new(limits.max_segments, "ISOBMFF boxes");
        let mut top = Boxes::top(bytes);
        let mut sequence = false;
        while let Some(b) = top.next(&mut work)? {
            match &b.kind {
                b"meta" => return walk(bytes, &b, 4, &mut work),
                b"moov" => sequence = true,
                _ => {}
            }
        }
        Err(if sequence { UNSUPPORTED } else { Error::Malformed("a HEIF file without meta".into()) })
    }

    /// Walks `parent`'s children, descending into the container boxes HEIF puts in `meta`.
    fn walk(bytes: &[u8], parent: &BoxRef, skip: usize, work: &mut Work) -> Result<()> {
        let mut boxes = Boxes::children(bytes, parent, skip)?;
        while let Some(b) = boxes.next(work)? {
            let skip = match &b.kind {
                b"iprp" | b"ipco" | b"dinf" => 0,
                b"iref" => 4,
                b"dref" => 8,
                // A full box, then an entry count of 16 bits (version 0) or 32.
                b"iinf" => 4 + if bytes.get(b.body.start) == Some(&0) { 2 } else { 4 },
                _ => continue,
            };
            walk(bytes, &b, skip, work)?;
        }
        Ok(())
    }

    /// Refuses a primary (or tile) item that is not HEVC: AV1 is AVIF; anything else (overlays,
    /// JPEG items, identity derivations) is unsupported.
    fn check_item(ctx: &Context<'_>, id: u32, grid_allowed: bool) -> Result<()> {
        let item = ctx.meta.item(id).ok_or_else(|| Error::Malformed(format!("HEIF: item {id} is missing")))?;
        match &item.item_type {
            b"hvc1" => Ok(()),
            b"grid" if grid_allowed => Ok(()),
            b"av01" => Err(Error::Unsupported(Format::Avif)),
            _ => Err(UNSUPPORTED),
        }
    }

    fn open<'a>(bytes: &'a [u8], limits: &Limits) -> Result<Context<'a>> {
        structure(bytes, limits)?;
        let ctx = Context::open(bytes).map_err(err)?;
        check_item(&ctx, ctx.meta.primary, true)?;
        Ok(ctx)
    }

    pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
        let ctx = open(bytes, limits)?;
        let info = heic_rs::probe(bytes).map_err(err)?;
        if matches!(info.chroma, ChromaFormat::Yuv422 | ChromaFormat::Yuv444) {
            return Err(UNSUPPORTED);
        }
        let bare = Header {
            format: Format::Heif,
            width: info.width,
            height: info.height,
            orientation: Orientation::UPRIGHT,
            exif: None,
            exif_error: None,
            thumbnail: None,
        };
        // The EXIF orientation only repeats the container's transforms, which the decode
        // applies: the header stays upright. EXIF thumbnails are not offered (no file offset).
        // An EXIF item past the end of the bytes means a partial read: read on. A damaged one
        // only loses the metadata.
        let exif = match ctx.exif(ctx.meta.primary) {
            Ok(exif) => exif,
            Err(heic_rs::Error::Truncated(_)) => return Err(Error::Truncated),
            Err(_) => None,
        };
        let mut h = bare.with_exif(exif.map(|e| (e, 0)), limits);
        h.orientation = Orientation::UPRIGHT;
        h.thumbnail = None;
        Ok(h)
    }

    /// The coded items of `id`: the item itself, or a grid's tiles in `dimg` order.
    fn coded_items(ctx: &Context<'_>, id: u32) -> Result<(Option<heic_rs::Grid>, Vec<u32>)> {
        Ok(match ctx.grid(id).map_err(err)? {
            Some((g, tiles)) => (Some(g), tiles),
            None => (None, vec![id]),
        })
    }

    /// Checks every coded picture of `items` before decoding any: HEVC, 4:2:0 or grey, and the
    /// pictures' total size within the limits. Returns the pixels to charge.
    fn check_pictures(ctx: &Context<'_>, items: &[u32], limits: &Limits) -> Result<u64> {
        let mut total = 0u64;
        for &t in items {
            check_item(ctx, t, false)?;
            let p = ctx.props(t).map_err(err)?;
            let hvcc = p.hvcc.as_ref().ok_or(UNSUPPORTED)?;
            if hvcc.chroma_format >= 2 {
                return Err(UNSUPPORTED);
            }
            let info = heic_rs::hevc::probe(&hvcc.parameter_sets()).map_err(err)?;
            total = total.saturating_add(u64::from(info.width) * u64::from(info.height));
            // Tiles may overhang the output a little; never more than twice the pixel limit.
            if total > limits.max_pixels.saturating_mul(2) {
                return Err(Error::TooLarge { width: info.width, height: info.height });
            }
        }
        Ok(total)
    }

    fn picture(ctx: &Context<'_>, id: u32, limit: u64) -> Result<Frame> {
        let p = ctx.props(id).map_err(err)?;
        let hvcc = p.hvcc.as_ref().ok_or(UNSUPPORTED)?;
        let data = ctx.item_data(id).map_err(err)?;
        let nals = hvcc.split_nals(&data).map_err(err)?;
        let params = hvcc.parameter_sets();
        if params.is_empty() {
            return Err(Error::Malformed("HEIF: hvcC carries no parameter sets".into()));
        }
        let frame = heic_rs::hevc::decode_still(&params, &nals).map_err(err)?;
        frame.validate().map_err(err)?;
        heic_rs::image::check_pixels(frame.width, frame.height, limit).map_err(err)?;
        Ok(frame)
    }

    /// Decodes an item (a picture or a grid, composed) and returns its first coded picture's
    /// properties, where the HEVC configuration and the VUI live.
    fn item<'a>(ctx: &Context<'a>, id: u32, limit: u64) -> Result<(Frame, ItemProps<'a>)> {
        let (grid, items) = coded_items(ctx, id)?;
        let first = *items.first().ok_or_else(|| Error::Malformed("HEIF: a grid without tiles".into()))?;
        let first_props = ctx.props(first).map_err(err)?;
        let frame = match grid {
            Some(g) => {
                let tiles = items.iter().map(|&t| picture(ctx, t, limit)).collect::<Result<Vec<_>>>()?;
                heic_rs::grid::compose(&g, &tiles, limit).map_err(err)?
            }
            None => picture(ctx, id, limit)?,
        };
        Ok((frame, first_props))
    }

    /// The colour description: `nclx` on the item (or its first tile), else the VUI of the first
    /// coded picture, else the H.265 defaults (limited range); an unspecified matrix is BT.601.
    fn colour(p: &ItemProps<'_>, first: &ItemProps<'_>) -> Nclx {
        if let Some(n) = p.nclx.or(first.nclx) {
            return n;
        }
        let s = first.hvcc.as_ref().and_then(|c| vui::signal(&c.parameter_sets())).unwrap_or_default();
        let code = if s.matrix == 2 { 6 } else { u16::from(s.matrix) };
        Nclx {
            primaries: s.primaries.into(),
            transfer: s.transfer.into(),
            matrix: MatrixCoefficients::from_code(code),
            matrix_code: code,
            range: if s.full_range { SignalRange::Full } else { SignalRange::Limited },
        }
    }

    /// Applies the container's transforms in association order. `imir` axis 0 flips top and
    /// bottom, as the writers of these files mean it (heic-rs 0.1.1 reads it the other way).
    fn transformed(mut img: Image, transforms: &[Transform]) -> Result<Image> {
        for t in transforms {
            img = match *t {
                Transform::Crop(c) => transform::crop(img, c).map_err(err)?,
                Transform::Rotate(r) => transform::rotate(img, r),
                Transform::Mirror(Mirror::LeftRight) => transform::mirror(img, Mirror::TopBottom),
                Transform::Mirror(Mirror::TopBottom) => transform::mirror(img, Mirror::LeftRight),
            };
        }
        Ok(img)
    }

    pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
        let ctx = open(bytes, limits)?;
        let id = ctx.meta.primary;
        let p = ctx.props(id).map_err(err)?;
        let limit = limits.max_pixels;
        let (_, items) = coded_items(&ctx, id)?;
        let mut pixels = check_pictures(&ctx, &items, limits)?;
        let alpha_id = ctx.alpha_item(id).map_err(err)?;
        if let Some(a) = alpha_id {
            pixels = pixels.saturating_add(check_pictures(&ctx, &coded_items(&ctx, a)?.1, limits)?);
        }
        // What heic-rs allocates itself: the pictures' planes (and a grid's canvas), the RGB
        // image and a rotated copy; given back once they are dropped.
        // Charged at 16-bit RGBA, the deepest layout.
        let (w, h) = (u64::from(header.width), u64::from(header.height));
        let scratch = pixels.saturating_mul(PLANE_BYTES).saturating_add(w.saturating_mul(h).saturating_mul(PLANE_BYTES + 2 * 8));
        budget.charge(scratch)?;
        let result = decode_pixels(&ctx, id, &p, alpha_id, limit, budget);
        budget.refund(scratch);
        result
    }

    fn decode_pixels(ctx: &Context<'_>, id: u32, p: &ItemProps<'_>, alpha_id: Option<u32>, limit: u64, budget: &mut Budget) -> Result<Decoded> {
        let (frame, first) = item(ctx, id, limit)?;
        let alpha = match alpha_id {
            Some(a) => {
                let (alpha, _) = item(ctx, a, limit)?;
                if (alpha.width, alpha.height) != (frame.width, frame.height) {
                    return Err(UNSUPPORTED);
                }
                Some(alpha)
            }
            None => None,
        };
        let deep = frame.bit_depth > 8;
        let layout = match (deep, alpha.is_some()) {
            (false, false) => PixelLayout::Rgb8,
            (false, true) => PixelLayout::Rgba8,
            (true, false) => PixelLayout::Rgb16,
            (true, true) => PixelLayout::Rgba16,
        };
        let nclx = colour(p, &first);
        let icc = p.icc.or(first.icc).map(<[u8]>::to_vec);
        let img = heic_rs::color::convert(&frame, alpha.as_ref(), nclx, layout, limit, Some(1)).map_err(err)?;
        drop((frame, alpha));
        let img = transformed(img, &p.transforms)?;
        let samples = if deep {
            let mut out = budget.vec(img.data.len() / 2, 0u16)?;
            for (o, &[a, b]) in out.iter_mut().zip(img.data.as_chunks::<2>().0) {
                *o = u16::from_ne_bytes([a, b]);
            }
            Samples::U16(out)
        } else {
            budget.charge(img.data.len() as u64)?;
            Samples::U8(img.data)
        };
        Ok(Decoded {
            width: img.width,
            height: img.height,
            model: ColorModel::Rgb,
            alpha: layout.has_alpha(),
            samples,
            icc,
            orientation: Orientation::UPRIGHT,
        })
    }
}

/// The colour description in an HEVC sequence parameter set's VUI (Rec. ITU-T H.265, 7.3.2.2 and
/// Annex E): the fallback when an item carries no `colr` box of type `nclx`. Only the syntax up
/// to `matrix_coeffs` is read; anything this reader can't follow is `None` and the caller falls
/// back to the H.265 defaults. It never fails a decode.
#[cfg(feature = "heif")]
mod vui {
    /// What the VUI's `video_signal_type` says (H.265 E.3.1). Absent fields keep the defaults the
    /// standard infers: limited range, every code point 2 ("unspecified").
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Signal {
        pub full_range: bool,
        pub primaries: u8,
        pub transfer: u8,
        pub matrix: u8,
    }

    impl Default for Signal {
        fn default() -> Self {
            Signal { full_range: false, primaries: 2, transfer: 2, matrix: 2 }
        }
    }

    /// NAL unit type of a sequence parameter set.
    pub const SPS_NUT: u8 = 33;

    /// The signal description from the SPS among `parameter_sets` (NAL units with their two-byte
    /// header). `None`: no SPS, or one this reader can't follow.
    pub fn signal(parameter_sets: &[&[u8]]) -> Option<Signal> {
        let sps = parameter_sets.iter().find(|n| n.first().is_some_and(|b| (b >> 1) & 0x3f == SPS_NUT))?;
        parse_sps(&unescape(sps.get(2..)?))
    }

    /// Removes emulation prevention bytes (`00 00 03` → `00 00`).
    fn unescape(nal: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(nal.len());
        let mut zeros = 0usize;
        for &b in nal {
            if zeros >= 2 && b == 3 {
                zeros = 0;
                continue;
            }
            zeros = if b == 0 { zeros + 1 } else { 0 };
            out.push(b);
        }
        out
    }

    struct Bits<'a> {
        data: &'a [u8],
        pos: usize,
    }

    impl Bits<'_> {
        fn u(&mut self, n: u32) -> Option<u32> {
            let mut v = 0u32;
            for _ in 0..n {
                let byte = *self.data.get(self.pos / 8)?;
                v = (v << 1) | u32::from((byte >> (7 - self.pos % 8)) & 1);
                self.pos += 1;
            }
            Some(v)
        }

        fn flag(&mut self) -> Option<bool> {
            self.u(1).map(|b| b == 1)
        }

        fn skip(&mut self, n: usize) -> Option<()> {
            self.pos = self.pos.checked_add(n)?;
            (self.pos <= self.data.len().saturating_mul(8)).then_some(())
        }

        /// `ue(v)`; values past 32 bits are refused (no valid field here needs them).
        fn ue(&mut self) -> Option<u32> {
            let mut zeros = 0u32;
            while !self.flag()? {
                zeros += 1;
                if zeros > 31 {
                    return None;
                }
            }
            let rest = self.u(zeros)?;
            ((1u64 << zeros) - 1 + u64::from(rest)).try_into().ok()
        }
    }

    /// `profile_tier_level(1, max_sub_layers_minus1)` (7.3.3): only skipped.
    fn profile_tier_level(r: &mut Bits, max_sub_layers_minus1: u32) -> Option<()> {
        r.skip(96)?; // general profile space .. general_level_idc
        let mut present = [(false, false); 8];
        for p in present.iter_mut().take(max_sub_layers_minus1 as usize) {
            *p = (r.flag()?, r.flag()?);
        }
        if max_sub_layers_minus1 > 0 {
            r.skip(2 * (8 - max_sub_layers_minus1 as usize))?; // reserved_zero_2bits
        }
        for &(profile, level) in present.iter().take(max_sub_layers_minus1 as usize) {
            if profile {
                r.skip(88)?;
            }
            if level {
                r.skip(8)?;
            }
        }
        Some(())
    }

    /// `scaling_list_data()` (7.3.4): only skipped.
    fn scaling_list_data(r: &mut Bits) -> Option<()> {
        for size_id in 0..4u32 {
            let step = if size_id == 3 { 3 } else { 1 };
            for _ in (0..6).step_by(step) {
                if !r.flag()? {
                    r.ue()?; // scaling_list_pred_matrix_id_delta
                } else {
                    let coefs = 64.min(1 << (4 + (size_id << 1)));
                    if size_id > 1 {
                        r.ue()?; // scaling_list_dc_coef_minus8 (se(v): same length)
                    }
                    for _ in 0..coefs {
                        r.ue()?; // scaling_list_delta_coef (se(v))
                    }
                }
            }
        }
        Some(())
    }

    /// `st_ref_pic_set(idx)` (7.3.7) as it appears in an SPS; returns `NumDeltaPocs[idx]`.
    fn st_ref_pic_set(r: &mut Bits, idx: usize, num_delta_pocs: &[u32]) -> Option<u32> {
        if idx != 0 && r.flag()? {
            // inter_ref_pic_set_prediction_flag: predicted from the previous set.
            r.skip(1)?; // delta_rps_sign
            r.ue()?; // abs_delta_rps_minus1
            let reference = *num_delta_pocs.get(idx - 1)?;
            let mut count = 0u32;
            for _ in 0..=reference {
                let used = r.flag()?;
                let kept = used || r.flag()?; // use_delta_flag when not used_by_curr_pic_flag
                count += u32::from(kept);
            }
            return Some(count);
        }
        let negative = r.ue()?;
        let positive = r.ue()?;
        let total = negative.checked_add(positive)?;
        if total > 32 {
            return None; // at most 16 + 16 for any level
        }
        for _ in 0..total {
            r.ue()?; // delta_poc_s*_minus1
            r.skip(1)?; // used_by_curr_pic_s*_flag
        }
        Some(total)
    }

    fn parse_sps(rbsp: &[u8]) -> Option<Signal> {
        let r = &mut Bits { data: rbsp, pos: 0 };
        r.skip(4)?; // sps_video_parameter_set_id
        let max_sub_layers_minus1 = r.u(3)?;
        if max_sub_layers_minus1 > 6 {
            return None;
        }
        r.skip(1)?; // sps_temporal_id_nesting_flag
        profile_tier_level(r, max_sub_layers_minus1)?;
        r.ue()?; // sps_seq_parameter_set_id
        if r.ue()? == 3 {
            r.skip(1)?; // separate_colour_plane_flag
        }
        r.ue()?; // pic_width_in_luma_samples
        r.ue()?; // pic_height_in_luma_samples
        if r.flag()? {
            for _ in 0..4 {
                r.ue()?; // conf_win_*_offset
            }
        }
        r.ue()?; // bit_depth_luma_minus8
        r.ue()?; // bit_depth_chroma_minus8
        let log2_max_poc_lsb = r.ue()?.checked_add(4)?;
        if log2_max_poc_lsb > 16 {
            return None;
        }
        let first = if r.flag()? { 0 } else { max_sub_layers_minus1 };
        for _ in first..=max_sub_layers_minus1 {
            r.ue()?; // sps_max_dec_pic_buffering_minus1
            r.ue()?; // sps_max_num_reorder_pics
            r.ue()?; // sps_max_latency_increase_plus1
        }
        for _ in 0..6 {
            r.ue()?; // coding/transform block sizes, transform hierarchy depths
        }
        if r.flag()? && r.flag()? {
            // scaling_list_enabled_flag, sps_scaling_list_data_present_flag
            scaling_list_data(r)?;
        }
        r.skip(2)?; // amp_enabled_flag, sample_adaptive_offset_enabled_flag
        if r.flag()? {
            // pcm_enabled_flag
            r.skip(8)?; // pcm sample bit depths
            r.ue()?;
            r.ue()?;
            r.skip(1)?; // pcm_loop_filter_disabled_flag
        }
        let sets = r.ue()? as usize;
        if sets > 64 {
            return None;
        }
        let mut num_delta_pocs = Vec::with_capacity(sets);
        for idx in 0..sets {
            let n = st_ref_pic_set(r, idx, &num_delta_pocs)?;
            num_delta_pocs.push(n);
        }
        if r.flag()? {
            // long_term_ref_pics_present_flag
            let n = r.ue()?;
            if n > 32 {
                return None;
            }
            for _ in 0..n {
                r.skip(log2_max_poc_lsb as usize + 1)?; // lt_ref_pic_poc_lsb_sps, used_by_curr_pic_lt_sps_flag
            }
        }
        r.skip(2)?; // sps_temporal_mvp_enabled_flag, strong_intra_smoothing_enabled_flag
        let mut signal = Signal::default();
        if !r.flag()? {
            return Some(signal); // no VUI
        }
        if r.flag()? && r.u(8)? == 255 {
            // aspect_ratio_info_present_flag, aspect_ratio_idc == EXTENDED_SAR
            r.skip(32)?;
        }
        if r.flag()? {
            r.skip(1)?; // overscan_appropriate_flag
        }
        if r.flag()? {
            // video_signal_type_present_flag
            r.skip(3)?; // video_format
            signal.full_range = r.flag()?;
            if r.flag()? {
                signal.primaries = r.u(8)? as u8;
                signal.transfer = r.u(8)? as u8;
                signal.matrix = r.u(8)? as u8;
            }
        }
        Some(signal)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A bit writer for building parameter sets.
        #[derive(Default)]
        struct Writer {
            bits: Vec<bool>,
        }

        impl Writer {
            fn u(&mut self, v: u32, n: u32) {
                for i in (0..n).rev() {
                    self.bits.push(i < 32 && (v >> i) & 1 == 1);
                }
            }
            fn ue(&mut self, v: u32) {
                let x = u64::from(v) + 1;
                let len = 64 - x.leading_zeros();
                self.u(0, len - 1);
                for i in (0..len).rev() {
                    self.bits.push((x >> i) & 1 == 1);
                }
            }
            fn bytes(mut self) -> Vec<u8> {
                self.bits.push(true); // rbsp_stop_one_bit
                while !self.bits.len().is_multiple_of(8) {
                    self.bits.push(false);
                }
                self.bits.chunks(8).map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | u8::from(b))).collect()
            }
        }

        /// An SPS with every optional part this reader skips, then `vui`.
        fn sps(vui: impl FnOnce(&mut Writer)) -> Vec<u8> {
            let mut w = Writer::default();
            w.u(0, 4);
            w.u(1, 3); // one sub-layer above the base
            w.u(1, 1);
            w.u(0, 88);
            w.u(93, 8);
            w.u(0b11, 2); // sub-layer profile and level present
            w.u(0, 2 * 7);
            w.u(0, 88);
            w.u(0, 8);
            w.ue(0);
            w.ue(1); // 4:2:0
            w.ue(4032);
            w.ue(3024);
            w.u(1, 1); // conformance window
            for _ in 0..4 {
                w.ue(3);
            }
            w.ue(0);
            w.ue(0);
            w.ue(4);
            w.u(1, 1);
            for _ in 0..2 {
                w.ue(1);
                w.ue(0);
                w.ue(0);
            }
            for v in [0, 3, 0, 3, 0, 0] {
                w.ue(v);
            }
            w.u(1, 1); // scaling lists, with data
            w.u(1, 1);
            for size_id in 0..4u32 {
                for _ in (0..6).step_by(if size_id == 3 { 3 } else { 1 }) {
                    w.u(1, 1);
                    if size_id > 1 {
                        w.ue(0);
                    }
                    for _ in 0..64.min(1 << (4 + (size_id << 1))) {
                        w.ue(0);
                    }
                }
            }
            w.u(0, 2);
            w.u(1, 1); // pcm
            w.u(0x77, 8);
            w.ue(0);
            w.ue(1);
            w.u(1, 1);
            // Two short-term sets, the second predicted from the first.
            w.ue(2);
            w.ue(1);
            w.ue(0);
            w.ue(0);
            w.u(1, 1);
            w.u(1, 1);
            w.u(0, 1);
            w.ue(0);
            w.u(0, 1);
            w.u(1, 1);
            w.u(1, 1);
            w.u(1, 1); // long-term pictures
            w.ue(1);
            w.u(0, 8 + 1);
            w.u(0, 2);
            vui(&mut w);
            let mut nal = vec![SPS_NUT << 1, 1];
            let mut zeros = 0;
            for b in w.bytes() {
                if zeros >= 2 && b <= 3 {
                    nal.push(3);
                    zeros = 0;
                }
                zeros = if b == 0 { zeros + 1 } else { 0 };
                nal.push(b);
            }
            nal
        }

        #[test]
        fn reads_full_range_and_matrix_from_the_vui() {
            let nal = sps(|w| {
                w.u(1, 1); // vui present
                w.u(1, 1);
                w.u(255, 8);
                w.u(0x0001_0001, 32);
                w.u(1, 1);
                w.u(0, 1);
                w.u(1, 1); // video_signal_type_present_flag
                w.u(5, 3);
                w.u(1, 1); // full range
                w.u(1, 1);
                w.u(12, 8);
                w.u(13, 8);
                w.u(6, 8);
            });
            let s = signal(&[&[0x40, 1, 0xc], &nal, &[0x44, 1]]);
            assert_eq!(s, Some(Signal { full_range: true, primaries: 12, transfer: 13, matrix: 6 }));
        }

        #[test]
        fn absent_fields_take_the_defaults_and_damage_is_none() {
            assert_eq!(signal(&[&sps(|w| w.u(0, 1))]), Some(Signal::default()));
            assert_eq!(signal(&[]), None);
            assert_eq!(signal(&[&[0x40, 1, 2, 3]]), None);
            assert_eq!(signal(&[&[SPS_NUT << 1, 1, 0xff, 0xff, 0xff]]), None);
            let nal = sps(|w| w.u(0, 1));
            for cut in 2..nal.len() {
                let _ = signal(&[&nal[..cut]]);
            }
        }
    }
}
