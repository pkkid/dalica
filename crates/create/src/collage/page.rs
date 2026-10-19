//! The collage's page (`creating.md` §3): its format, one of the Crop tool's ratios (`editing.md`
//! §6.1: the photo sizes of the locale, the fixed ratios, the user's custom ratios) or Current
//! Display, and its orientation; and the page's size in pixels.
//!
//! A ratio is orientation-free (`4 × 6` and `6 × 4` are the same format): the page's orientation
//! decides which side is long. Current Display keeps the display's size in pixels (taken when the
//! format is chosen), so a recipe reopened on another display lays out and renders the same.

use serde::{Deserialize, Serialize};

use super::DocError;

/// The long side of a created collage in pixels, for every format but Current Display.
pub const LONG_SIDE: u32 = 3000;
/// The largest side of a page in pixels (a display's or a render's).
pub const MAX_PIXELS: u32 = 16_384;
/// The largest side of a ratio (the Crop tool's limit, `crop.ratios.add`).
pub const MAX_RATIO_SIDE: f64 = 10_000.0;
/// The most a page's long side may be over its short side: wider ratios are not page formats.
pub const MAX_RATIO: f64 = 1_000.0;
/// The longest name of a ratio, in bytes.
pub const MAX_NAME: usize = 256;

/// The photo sizes of each locale, as the Crop tool lists them: name, width, height.
pub const US_SIZES: [(&str, f64, f64); 4] = [("4 × 6", 4.0, 6.0), ("5 × 7", 5.0, 7.0), ("8 × 10", 8.0, 10.0), ("8.5 × 11 (Letter)", 8.5, 11.0)];
pub const METRIC_SIZES: [(&str, f64, f64); 4] = [("10 × 15", 10.0, 15.0), ("13 × 18", 13.0, 18.0), ("20 × 25", 20.0, 25.0), ("A4", 210.0, 297.0)];
/// The fixed ratios after the photo sizes.
pub const FIXED: [(&str, f64, f64); 5] = [
    ("Square (1:1)", 1.0, 1.0),
    ("4:3 (Standard screen)", 4.0, 3.0),
    ("16:10 (Widescreen)", 16.0, 10.0),
    ("16:9 (HD video)", 16.0, 9.0),
    ("5:3 (Photo frame)", 5.0, 3.0),
];
/// The default format's name (`FIXED`).
pub const DEFAULT_FORMAT: &str = "16:9 (HD video)";
/// Current Display's name in the list.
pub const CURRENT_DISPLAY: &str = "Current Display";

/// Which photo sizes the list shows (the Crop tool's setting).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PhotoSizes {
    #[default]
    Us,
    Metric,
}

/// Which side of the page is long.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
}

/// A page format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    /// A ratio by name: a photo size, a fixed ratio or a custom one.
    Ratio { name: String, width: f64, height: f64 },
    /// Current Display, with the display's size in pixels.
    Display { width: u32, height: u32 },
}

impl Format {
    /// A ratio format, or `None` when its sides are not usable ([`Format::check`]).
    pub fn ratio(name: &str, width: f64, height: f64) -> Option<Format> {
        let f = Format::Ratio { name: name.to_string(), width, height };
        f.check().is_ok().then_some(f)
    }

    /// Current Display for a display of `width × height` pixels, or `None` when it is not usable.
    pub fn display(width: u32, height: u32) -> Option<Format> {
        let f = Format::Display { width, height };
        f.check().is_ok().then_some(f)
    }

    /// The name shown in the list.
    pub fn name(&self) -> &str {
        match self {
            Format::Ratio { name, .. } => name,
            Format::Display { .. } => CURRENT_DISPLAY,
        }
    }

    /// The long side over the short side (at least 1).
    pub fn long_ratio(&self) -> f64 {
        let (w, h) = match *self {
            Format::Ratio { width, height, .. } => (width, height),
            Format::Display { width, height } => (f64::from(width), f64::from(height)),
        };
        let r = w.max(h) / w.min(h);
        if r.is_finite() && r >= 1.0 { r.min(MAX_RATIO) } else { 1.0 }
    }

    /// Every side finite, above 0 and within its limit, the ratio at most [`MAX_RATIO`], the name
    /// at most [`MAX_NAME`] bytes.
    pub fn check(&self) -> Result<(), DocError> {
        match *self {
            Format::Ratio { ref name, width, height } => {
                if name.len() > MAX_NAME {
                    return Err(DocError::Range(format!("a page format's name is at most {MAX_NAME} bytes")));
                }
                let side = |v: f64| v.is_finite() && v > 0.0 && v <= MAX_RATIO_SIDE;
                if !(side(width) && side(height)) || width.max(height) / width.min(height) > MAX_RATIO {
                    return Err(DocError::Range(format!(
                        "a page format's sides are above 0, at most {MAX_RATIO_SIDE}, the long one at most {MAX_RATIO} times the short one"
                    )));
                }
            }
            Format::Display { width, height } => {
                let side = |v: u32| (1..=MAX_PIXELS).contains(&v);
                if !(side(width) && side(height)) || f64::from(width.max(height)) / f64::from(width.min(height)) > MAX_RATIO {
                    return Err(DocError::Range(format!("a display's sides are 1 to {MAX_PIXELS} pixels")));
                }
            }
        }
        Ok(())
    }
}

/// The formats the Page format list offers, in order: the locale's photo sizes, the fixed ratios,
/// the custom ratios the caller gives (name, width, height; unusable ones left out), then Current
/// Display for the display's size when it is usable.
pub fn formats(sizes: PhotoSizes, custom: &[(String, f64, f64)], display: Option<(u32, u32)>) -> Vec<Format> {
    let table: &[(&str, f64, f64)] = match sizes {
        PhotoSizes::Us => &US_SIZES,
        PhotoSizes::Metric => &METRIC_SIZES,
    };
    let fixed = table.iter().chain(FIXED.iter()).filter_map(|&(n, w, h)| Format::ratio(n, w, h));
    let custom = custom.iter().filter_map(|(n, w, h)| Format::ratio(n, *w, *h));
    let display = display.and_then(|(w, h)| Format::display(w, h));
    fixed.chain(custom).chain(display).collect()
}

/// The page: a format and an orientation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub format: Format,
    pub orientation: Orientation,
}

impl Default for Page {
    /// 16:9 (HD video), landscape.
    fn default() -> Page {
        Page { format: Format::Ratio { name: DEFAULT_FORMAT.into(), width: 16.0, height: 9.0 }, orientation: Orientation::Landscape }
    }
}

impl Page {
    /// The page's width over its height.
    pub fn ratio(&self) -> f64 {
        let r = self.format.long_ratio();
        match self.orientation {
            Orientation::Landscape => r,
            Orientation::Portrait => 1.0 / r,
        }
    }

    /// The page's size in pixels with its long side `long_side` (clamped to 1 …
    /// [`MAX_PIXELS`]); the short side rounded, at least 1.
    pub fn pixel_size(&self, long_side: u32) -> (u32, u32) {
        let long = long_side.clamp(1, MAX_PIXELS);
        let short = (f64::from(long) / self.format.long_ratio()).round().clamp(1.0, f64::from(long)) as u32;
        match self.orientation {
            Orientation::Landscape => (long, short),
            Orientation::Portrait => (short, long),
        }
    }

    /// The size Create Collage writes: the display's size for Current Display, else
    /// [`LONG_SIDE`] on the long side.
    pub fn output_size(&self) -> (u32, u32) {
        match self.format {
            Format::Display { width, height } if self.format.check().is_ok() => {
                let (long, short) = (width.max(height), width.min(height));
                match self.orientation {
                    Orientation::Landscape => (long, short),
                    Orientation::Portrait => (short, long),
                }
            }
            _ => self.pixel_size(LONG_SIDE),
        }
    }
}
