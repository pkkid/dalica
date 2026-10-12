//! The colour stage: one conversion from a [`Decoded`] image to 8-bit sRGB RGBA (`formats.md` §4).
//!
//! - **Tagged** (an embedded ICC profile, v2 or v4, and [`DecodeOptions::color_managed`] on): the
//!   profile is checked against the limits of `security.md` §4.2 (≤ `max_icc` bytes, ≤
//!   `max_icc_tags` tags, the tag table inside the data, a colour space matching the pixels),
//!   parsed and applied with `moxcms` (relative colorimetric) to sRGB.
//! - **Unusable profile** (damaged, oversized, the wrong colour space, or `moxcms` refusing or
//!   panicking): logged, and the pixels are shown as if untagged ([`ColorPath::Fallback`]).
//! - **Untagged**, or colour management off: grey and RGB are taken as sRGB; CMYK is converted
//!   naively (R = (1 − C)(1 − K), …).
//!
//! 16-bit samples go through `moxcms`' 16-bit transform and are rounded to 8 bits once at the
//! end; float samples go through its float transform. Alpha is carried over, never converted.
//! Work is done in bands of rows so temporary buffers stay small; every buffer is charged to the
//! decode's [`Budget`].

use std::panic::{AssertUnwindSafe, catch_unwind};

use moxcms::{ColorProfile, DataColorSpace, Layout, ParsingOptions, RenderingIntent, TransformExecutor, TransformOptions};

use crate::decode::{ColorModel, DecodeOptions, Decoded, Samples};
use crate::limits::{Budget, Limits};
use crate::{Result, Rgba8};

/// Pixels converted per band.
const BAND_PIXELS: usize = 1 << 14;
/// Bytes of an ICC header plus the tag count (ICC.1 §7.2–7.3).
const ICC_HEADER: usize = 132;

/// Which way the colour stage went (for tests and diagnostics).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorPath {
    /// No profile: grey and RGB taken as sRGB.
    Untagged,
    /// A profile was present but colour management is off.
    Ignored,
    /// The embedded profile was applied.
    Profile,
    /// The embedded profile could not be used (the reason); the pixels were taken as untagged.
    Fallback(String),
}

/// The output of the colour stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Converted {
    /// 8-bit sRGB RGBA, stored orientation.
    pub image: Rgba8,
    pub path: ColorPath,
}

/// Converts `d` to 8-bit sRGB RGBA (still in its stored orientation). Untagged 8-bit RGBA samples
/// are the output as they are, without a copy.
pub fn to_srgb8(d: Decoded, options: &DecodeOptions, budget: &mut Budget, limits: &Limits) -> Result<Converted> {
    d.check()?;
    let (width, height) = (d.width, d.height);
    let path = match (&d.icc, options.color_managed) {
        (None, _) => ColorPath::Untagged,
        (Some(_), false) => ColorPath::Ignored,
        (Some(icc), true) => match with_profile(&d, icc, budget, limits) {
            Ok(data) => return Ok(Converted { image: Rgba8 { width, height, data }, path: ColorPath::Profile }),
            Err(reason) => {
                log::warn!("colour profile not used, shown as sRGB: {reason}");
                ColorPath::Fallback(reason)
            }
        },
    };
    let data = untagged(d, budget)?;
    Ok(Converted { image: Rgba8 { width, height, data }, path })
}

/// Checks and parses an embedded profile for pixels of `model`; `Err` says why it is unusable.
pub fn load_profile(icc: &[u8], model: ColorModel, limits: &Limits) -> std::result::Result<ColorProfile, String> {
    if icc.len() > limits.max_icc {
        return Err(format!("a profile of {} bytes is over the {} byte limit", icc.len(), limits.max_icc));
    }
    let be = |at: usize| icc.get(at..at + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_be_bytes);
    let (Some(declared), Some(tags)) = (be(0), be(128)) else { return Err(format!("a profile of {} bytes is too short", icc.len())) };
    if declared as usize > icc.len() {
        return Err(format!("the profile says {declared} bytes but has {}", icc.len()));
    }
    if tags > limits.max_icc_tags {
        return Err(format!("{tags} tags is over the limit of {}", limits.max_icc_tags));
    }
    if ICC_HEADER + 12 * tags as usize > icc.len() {
        return Err("the tag table runs past the profile".into());
    }
    let options = ParsingOptions { max_profile_size: limits.max_icc.saturating_add(1), ..ParsingOptions::default() };
    let profile = catch_unwind(|| ColorProfile::new_from_slice_with_options(icc, options))
        .map_err(|_| "the profile parser panicked".to_string())?
        .map_err(|e| format!("the profile does not parse: {e:?}"))?;
    let fits = matches!(
        (profile.color_space, model),
        (DataColorSpace::Gray, ColorModel::Gray) | (DataColorSpace::Rgb, ColorModel::Rgb) | (DataColorSpace::Cmyk, ColorModel::Cmyk)
    );
    if !fits {
        return Err(format!("a {:?} profile on {model:?} pixels", profile.color_space));
    }
    Ok(profile)
}

/// Applies the embedded profile, or says why it cannot be used.
fn with_profile(d: &Decoded, icc: &[u8], budget: &mut Budget, limits: &Limits) -> std::result::Result<Vec<u8>, String> {
    let profile = load_profile(icc, d.model, limits)?;
    let srgb = ColorProfile::new_srgb();
    let options =
        TransformOptions { rendering_intent: RenderingIntent::RelativeColorimetric, allow_use_cicp_transfer: false, ..TransformOptions::default() };
    let (ch, cc) = (d.channels(), d.model.channels());
    let mut out = budget.vec(d.width as usize * d.height as usize * 4, 255u8).map_err(|e| e.to_string())?;
    let mut run = || -> std::result::Result<(), String> {
        let err = |e: moxcms::CmsError| format!("the profile cannot be applied: {e:?}");
        match (&d.samples, d.model) {
            // 8-bit grey and RGB, with or without alpha: straight into RGBA, alpha carried over.
            (Samples::U8(s), ColorModel::Gray | ColorModel::Rgb) => {
                let layout = match (d.model, d.alpha) {
                    (ColorModel::Gray, false) => Layout::Gray,
                    (ColorModel::Gray, true) => Layout::GrayAlpha,
                    (_, false) => Layout::Rgb,
                    (_, true) => Layout::Rgba,
                };
                let t = profile.create_transform_8bit(layout, &srgb, Layout::Rgba, options).map_err(err)?;
                for (band, dst) in s.chunks(BAND_PIXELS * ch).zip(out.chunks_mut(BAND_PIXELS * 4)) {
                    t.transform(band, dst).map_err(|e| format!("the colour transform failed: {e:?}"))?;
                }
                Ok(())
            }
            // CMYK and deeper samples: the colour channels in bands, alpha copied separately.
            (Samples::U8(s), ColorModel::Cmyk) => {
                let t = profile.create_transform_8bit(Layout::Rgba, &srgb, Layout::Rgb, options).map_err(err)?;
                bands(&*t, s, ch, cc, |v| v, |v| v, budget, &mut out)
            }
            (Samples::U16(s), _) => {
                let t = profile.create_transform_16bit(source_layout(d.model), &srgb, Layout::Rgb, options).map_err(err)?;
                bands(&*t, s, ch, cc, |v| v, to8_u16, budget, &mut out)
            }
            (Samples::F32(s), _) => {
                let t = profile.create_transform_f32(source_layout(d.model), &srgb, Layout::Rgb, options).map_err(err)?;
                bands(&*t, s, ch, cc, |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }, to8_f32, budget, &mut out)
            }
        }
    };
    catch_unwind(AssertUnwindSafe(&mut run)).map_err(|_| "the colour transform panicked".to_string())??;
    if d.alpha && !matches!((&d.samples, d.model), (Samples::U8(_), ColorModel::Gray | ColorModel::Rgb)) {
        copy_alpha(d, &mut out);
    }
    Ok(out)
}

/// The `moxcms` layout of the colour channels alone (CMYK as four channels).
fn source_layout(model: ColorModel) -> Layout {
    match model {
        ColorModel::Gray => Layout::Gray,
        ColorModel::Rgb => Layout::Rgb,
        ColorModel::Cmyk => Layout::Rgba,
    }
}

/// Runs `t` over `src` in bands: the colour channels of each pixel (alpha left out, `clean`ed)
/// into a small buffer, through the transform, and into `out`'s RGB bytes.
#[allow(clippy::too_many_arguments)]
fn bands<T: Copy + Default>(
    t: &(dyn TransformExecutor<T> + Send + Sync),
    src: &[T],
    ch: usize,
    cc: usize,
    clean: impl Fn(T) -> T,
    to8: impl Fn(T) -> u8,
    budget: &mut Budget,
    out: &mut [u8],
) -> std::result::Result<(), String> {
    let mut colour = budget.vec(BAND_PIXELS * cc, T::default()).map_err(|e| e.to_string())?;
    let mut rgb = budget.vec(BAND_PIXELS * 3, T::default()).map_err(|e| e.to_string())?;
    for (band, dst) in src.chunks(BAND_PIXELS * ch).zip(out.chunks_mut(BAND_PIXELS * 4)) {
        let n = band.len() / ch;
        let (colour, rgb) = (colour.get_mut(..n * cc).unwrap_or_default(), rgb.get_mut(..n * 3).unwrap_or_default());
        for (c, px) in colour.chunks_exact_mut(cc).zip(band.chunks_exact(ch)) {
            for (c, &v) in c.iter_mut().zip(px) {
                *c = clean(v);
            }
        }
        t.transform(colour, rgb).map_err(|e| format!("the colour transform failed: {e:?}"))?;
        for (o, &[r, g, b]) in dst.as_chunks_mut::<4>().0.iter_mut().zip(rgb.as_chunks::<3>().0) {
            [o[0], o[1], o[2]] = [to8(r), to8(g), to8(b)];
        }
    }
    Ok(())
}

/// Grey and RGB as sRGB, CMYK naively. 8-bit RGBA comes back as it is.
fn untagged(d: Decoded, budget: &mut Budget) -> Result<Vec<u8>> {
    let d = match d {
        Decoded { samples: Samples::U8(s), model: ColorModel::Rgb, alpha: true, .. } => return Ok(s),
        d => d,
    };
    let (model, alpha, ch) = (d.model, d.alpha, d.channels());
    let pixels = d.width as usize * d.height as usize;
    let mut out = budget.vec(pixels * 4, 255u8)?;
    match &d.samples {
        Samples::U8(s) if model == ColorModel::Rgb => {
            for (o, &[r, g, b]) in out.as_chunks_mut::<4>().0.iter_mut().zip(s.as_chunks::<3>().0) {
                [o[0], o[1], o[2]] = [r, g, b];
            }
        }
        Samples::U8(s) => plain(s, model, ch, |v| v, &mut out),
        Samples::U16(s) => plain(s, model, ch, to8_u16, &mut out),
        Samples::F32(s) => plain(s, model, ch, to8_f32, &mut out),
    }
    if alpha {
        copy_alpha(&d, &mut out);
    }
    Ok(out)
}

fn plain<T: Copy>(src: &[T], model: ColorModel, ch: usize, to8: impl Fn(T) -> u8, out: &mut [u8]) {
    for (px, o) in src.chunks_exact(ch).zip(out.as_chunks_mut::<4>().0) {
        let rgb = match (model, px) {
            (ColorModel::Gray, &[v, ..]) => [to8(v); 3],
            (ColorModel::Rgb, &[r, g, b, ..]) => [to8(r), to8(g), to8(b)],
            (ColorModel::Cmyk, &[c, m, y, k, ..]) => naive_cmyk([to8(c), to8(m), to8(y), to8(k)]),
            _ => [0; 3],
        };
        [o[0], o[1], o[2]] = rgb;
    }
}

/// CMYK ink amounts (0 = none) to RGB without a profile: R = (1 − C)(1 − K), and so on.
pub fn naive_cmyk([c, m, y, k]: [u8; 4]) -> [u8; 3] {
    let inv = |v: u8| u32::from(255 - v);
    let mix = |v: u8| ((inv(v) * inv(k) + 127) / 255) as u8;
    [mix(c), mix(m), mix(y)]
}

fn copy_alpha(d: &Decoded, out: &mut [u8]) {
    let (ch, at) = (d.channels(), d.model.channels());
    let put = |out: &mut [u8], alphas: &mut dyn Iterator<Item = u8>| {
        for (o, a) in out.as_chunks_mut::<4>().0.iter_mut().zip(alphas) {
            o[3] = a;
        }
    };
    match &d.samples {
        Samples::U8(s) => put(out, &mut s.chunks_exact(ch).filter_map(|p| p.get(at).copied())),
        Samples::U16(s) => put(out, &mut s.chunks_exact(ch).filter_map(|p| p.get(at).copied().map(to8_u16))),
        Samples::F32(s) => put(out, &mut s.chunks_exact(ch).filter_map(|p| p.get(at).copied().map(to8_f32))),
    }
}

/// 16 bits to 8, rounded.
pub fn to8_u16(v: u16) -> u8 {
    ((u32::from(v) * 255 + 32_767) / 65_535) as u8
}

/// A nominal 0–1 float to 8 bits: clamped, rounded, non-finite as 0.
pub fn to8_f32(v: f32) -> u8 {
    if v.is_finite() { (v.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Orientation;

    fn decoded(model: ColorModel, alpha: bool, samples: Samples, icc: Option<Vec<u8>>) -> Decoded {
        let ch = model.channels() + usize::from(alpha);
        Decoded { width: (samples.len() / ch) as u32, height: 1, model, alpha, samples, icc, orientation: Orientation::UPRIGHT }
    }

    fn convert(d: &Decoded, managed: bool) -> Converted {
        let limits = Limits::default();
        let mut budget = Budget::new(&limits, d.width, d.height);
        to_srgb8(d.clone(), &DecodeOptions { color_managed: managed }, &mut budget, &limits).unwrap()
    }

    #[test]
    fn untagged_samples_are_srgb_at_every_depth() {
        let g = decoded(ColorModel::Gray, true, Samples::U16(vec![0, 65535, 32768, 257, 65535, 0]), None);
        let c = convert(&g, true);
        assert_eq!((c.image.data, c.path), (vec![0, 0, 0, 255, 128, 128, 128, 1, 255, 255, 255, 0], ColorPath::Untagged));
        let f = decoded(ColorModel::Rgb, false, Samples::F32(vec![0.0, 0.5, 1.5, f32::NAN, -1.0, f32::INFINITY]), None);
        assert_eq!(convert(&f, true).image.data, vec![0, 128, 255, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn cmyk_without_a_profile_is_converted_naively() {
        assert_eq!(naive_cmyk([0, 0, 0, 0]), [255, 255, 255]);
        assert_eq!(naive_cmyk([255, 0, 0, 0]), [0, 255, 255]);
        assert_eq!(naive_cmyk([0, 0, 0, 255]), [0, 0, 0]);
        assert_eq!(naive_cmyk([128, 0, 64, 128]), [63, 127, 95]);
        let d = decoded(ColorModel::Cmyk, false, Samples::U8(vec![0, 255, 255, 0]), None);
        assert_eq!(convert(&d, true).image.data, vec![255, 0, 0, 255]);
    }

    #[test]
    fn an_srgb_profile_changes_nothing_that_matters() {
        let icc = ColorProfile::new_srgb().encode().unwrap();
        let px: Vec<u8> = (0..=255).flat_map(|v| [v, 255 - v, v / 2]).collect();
        let d = decoded(ColorModel::Rgb, false, Samples::U8(px.clone()), Some(icc));
        let c = convert(&d, true);
        assert_eq!(c.path, ColorPath::Profile);
        for (o, p) in c.image.data.chunks(4).zip(px.chunks(3)) {
            for i in 0..3 {
                assert!(o[i].abs_diff(p[i]) <= 1, "{o:?} vs {p:?}");
            }
        }
        assert_eq!(convert(&d, false).path, ColorPath::Ignored);
    }

    /// Alpha passes through every profile path unchanged, and a grey profile keeps grey neutral.
    #[test]
    fn alpha_and_grey_survive_the_profile_paths() {
        let srgb = ColorProfile::new_srgb().encode().unwrap();
        let rgba = decoded(ColorModel::Rgb, true, Samples::U8(vec![200, 100, 50, 7, 10, 20, 30, 250]), Some(srgb.clone()));
        let c = convert(&rgba, true);
        assert_eq!(c.path, ColorPath::Profile);
        assert_eq!((c.image.data[3], c.image.data[7]), (7, 250));
        let rgba16 = decoded(ColorModel::Rgb, true, Samples::U16(vec![0, 0, 0, 257 * 9, 65535, 65535, 65535, 0]), Some(srgb.clone()));
        let c = convert(&rgba16, true);
        assert_eq!((c.path, c.image.data), (ColorPath::Profile, vec![0, 0, 0, 9, 255, 255, 255, 0]));
        let grey = ColorProfile::new_gray_with_gamma(2.2).encode().unwrap();
        for alpha in [false, true] {
            let px = if alpha { vec![0u8, 3, 128, 99, 255, 255] } else { vec![0, 128, 255] };
            let c = convert(&decoded(ColorModel::Gray, alpha, Samples::U8(px), Some(grey.clone())), true);
            assert_eq!(c.path, ColorPath::Profile, "alpha {alpha}");
            let d = &c.image.data;
            assert_eq!((d[0], d[1], d[2], d[8], d[9], d[10]), (0, 0, 0, 255, 255, 255), "alpha {alpha}");
            assert!(d[4] == d[5] && d[5] == d[6] && d[4].abs_diff(128) < 8, "{d:?}");
            assert_eq!([d[3], d[7], d[11]], if alpha { [3, 99, 255] } else { [255; 3] });
        }
    }

    /// Untagged 8-bit RGBA is the output itself: no second buffer is charged.
    #[test]
    fn untagged_rgba_is_not_copied() {
        let limits = Limits { max_alloc: 8, ..Limits::default() };
        let mut budget = Budget::new(&limits, 2, 1);
        let d = decoded(ColorModel::Rgb, true, Samples::U8(vec![1, 2, 3, 4, 5, 6, 7, 8]), None);
        let c = to_srgb8(d, &DecodeOptions::default(), &mut budget, &limits).unwrap();
        assert_eq!((c.image.data, budget.left()), (vec![1, 2, 3, 4, 5, 6, 7, 8], 8));
    }

    #[test]
    fn unusable_profiles_fall_back_to_srgb() {
        let px = vec![10u8, 20, 30];
        let srgb = ColorProfile::new_srgb().encode().unwrap();
        let mut too_many_tags = srgb.clone();
        too_many_tags[128..132].copy_from_slice(&1001u32.to_be_bytes());
        let mut short = srgb.clone();
        short.truncate(100);
        let mut lying = srgb.clone();
        lying[0..4].copy_from_slice(&(srgb.len() as u32 + 1).to_be_bytes());
        let grey = ColorProfile::new_gray_with_gamma(2.2).encode().unwrap();
        for (case, icc) in [("tags", too_many_tags), ("short", short), ("lying size", lying), ("grey on RGB", grey), ("garbage", vec![0x55; 4000])] {
            let c = convert(&decoded(ColorModel::Rgb, false, Samples::U8(px.clone()), Some(icc)), true);
            assert!(matches!(c.path, ColorPath::Fallback(_)), "{case}: {:?}", c.path);
            assert_eq!(c.image.data, vec![10, 20, 30, 255], "{case}");
        }
        // Exactly at the size limit is allowed; one byte over is refused before parsing.
        let limits = Limits { max_icc: srgb.len(), ..Limits::default() };
        assert!(load_profile(&srgb, ColorModel::Rgb, &limits).is_ok());
        assert!(load_profile(&srgb, ColorModel::Rgb, &Limits { max_icc: srgb.len() - 1, ..limits }).is_err());
    }
}
