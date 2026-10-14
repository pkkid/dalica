//! What the Export dialog asks (`creating.md` §2): the size, the quality, numbers, the watermark
//! and whether to remove the location.

use dalica_meta::WriteOptions;

use crate::output::{Purpose, Quality, Spec};

/// The smallest long side the size slider offers.
pub const MIN_SIDE: u32 = 100;
/// The largest long side the size slider offers.
pub const MAX_SIDE: u32 = 4000;
/// The long side an export uses unless asked otherwise.
pub const DEFAULT_SIDE: u32 = 1600;
/// Where the size slider stops.
pub const SIDE_STOPS: [u32; 8] = [480, 640, 800, 1024, 1280, 1600, 2048, 3840];

/// The size of the exported pictures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// The rendered size (after any crop).
    Original,
    /// The longer side fitted into this many pixels, [`MIN_SIDE`]–[`MAX_SIDE`]; never enlarged.
    LongSide(u32),
}

/// The JPEG quality presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityPreset {
    /// The source's estimated quality and chroma subsampling, else 90 ("Keeps the original's
    /// quality").
    Automatic,
    /// 85 ("Balances quality and size").
    Normal,
    /// 95 ("Keeps fine detail; large files").
    Maximum,
    /// 60 ("Small files with some quality loss").
    Minimum,
    /// A quality of its own, 1–100.
    Custom(u8),
}

impl QualityPreset {
    pub const NORMAL: u8 = 85;
    pub const MAXIMUM: u8 = 95;
    pub const MINIMUM: u8 = 60;

    /// The quality the output pipeline writes.
    pub fn quality(self) -> Quality {
        match self {
            QualityPreset::Automatic => Quality::Match,
            QualityPreset::Normal => Quality::Fixed(Self::NORMAL),
            QualityPreset::Maximum => Quality::Fixed(Self::MAXIMUM),
            QualityPreset::Minimum => Quality::Fixed(Self::MINIMUM),
            QualityPreset::Custom(q) => Quality::Fixed(q),
        }
    }
}

/// Everything the Export dialog sets, apart from the location and the folder's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportOptions {
    pub size: Size,
    pub quality: QualityPreset,
    /// Put `001-`, `002-`, … in front of the names, in tray order.
    pub numbers: bool,
    /// The watermark's text; `None` or blank draws none.
    pub watermark: Option<String>,
    /// Remove GPS coordinates from the exported files (place names stay).
    pub remove_location: bool,
}

impl Default for ExportOptions {
    /// The dialog's defaults: resize to [`DEFAULT_SIDE`], Automatic quality, no numbers, no
    /// watermark, the location kept.
    fn default() -> ExportOptions {
        ExportOptions {
            size: Size::LongSide(DEFAULT_SIDE),
            quality: QualityPreset::Automatic,
            numbers: false,
            watermark: None,
            remove_location: false,
        }
    }
}

impl ExportOptions {
    /// Why these options cannot be used, if they cannot: a long side outside
    /// [`MIN_SIDE`]–[`MAX_SIDE`] or a custom quality outside 1–100.
    pub fn check(&self) -> Result<(), String> {
        if let Size::LongSide(side) = self.size
            && !(MIN_SIDE..=MAX_SIDE).contains(&side)
        {
            return Err(format!("the size must be {MIN_SIDE}–{MAX_SIDE} pixels, not {side}"));
        }
        if let QualityPreset::Custom(q) = self.quality
            && !(1..=100).contains(&q)
        {
            return Err(format!("the quality must be 1–100, not {q}"));
        }
        Ok(())
    }

    /// The output pipeline's request for every photo of the export: the export format (JPEG, PNG
    /// for transparency), the size and quality, the watermark, no EXIF thumbnail, and every owned
    /// field written (stars as ratings too: an export carries all of them).
    pub fn spec(&self) -> Result<Spec, String> {
        self.check()?;
        Ok(Spec {
            quality: self.quality.quality(),
            long_side: match self.size {
                Size::Original => None,
                Size::LongSide(side) => Some(side),
            },
            watermark: self.watermark.clone().filter(|t| !t.trim().is_empty()),
            thumbnail: false,
            remove_location: self.remove_location,
            write: WriteOptions { stars_as_ratings: true, ..WriteOptions::default() },
            ..Spec::new(Purpose::Export)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_map_to_their_qualities() {
        assert_eq!(QualityPreset::Automatic.quality(), Quality::Match);
        assert_eq!(QualityPreset::Normal.quality(), Quality::Fixed(85));
        assert_eq!(QualityPreset::Maximum.quality(), Quality::Fixed(95));
        assert_eq!(QualityPreset::Minimum.quality(), Quality::Fixed(60));
        assert_eq!(QualityPreset::Custom(42).quality(), Quality::Fixed(42));
    }

    #[test]
    fn out_of_range_options_are_refused() {
        assert!(ExportOptions::default().check().is_ok());
        for side in [0, 99, 4001, u32::MAX] {
            assert!(ExportOptions { size: Size::LongSide(side), ..ExportOptions::default() }.spec().is_err(), "{side}");
        }
        for side in [MIN_SIDE, MAX_SIDE] {
            assert!(ExportOptions { size: Size::LongSide(side), ..ExportOptions::default() }.check().is_ok());
        }
        for q in [0, 101, 255] {
            assert!(ExportOptions { quality: QualityPreset::Custom(q), ..ExportOptions::default() }.check().is_err(), "{q}");
        }
    }

    #[test]
    fn the_spec_follows_the_options() {
        let o = ExportOptions { size: Size::Original, watermark: Some("  ".into()), remove_location: true, ..ExportOptions::default() };
        let s = o.spec().unwrap();
        assert_eq!(s.long_side, None);
        assert_eq!(s.watermark, None, "blank text draws no watermark");
        assert!(s.remove_location && !s.thumbnail && s.write.stars_as_ratings);
        let s = ExportOptions { watermark: Some("© Ana".into()), ..ExportOptions::default() }.spec().unwrap();
        assert_eq!((s.long_side, s.watermark.as_deref()), (Some(DEFAULT_SIDE), Some("© Ana")));
    }
}
