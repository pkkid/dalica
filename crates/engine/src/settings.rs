//! `settings.json` in the library directory (`data-model.md` §4, `architecture.md` §4): preferences
//! as a JSON object. Keys this version does not know are kept verbatim; a damaged file is kept aside
//! as `.corrupt-<time>` and defaults are used.
//!
//! M1 reads and writes `"keymap"`: the user's shortcut overrides, as `{command, context, key}`
//! entries (`key: null` = removed). Entries for commands this version does not know, and entries it
//! cannot read, are kept verbatim and ignored (`menus-and-shortcuts.md` §2.0).
//!
//! M3 adds three objects, each merged key by key (keys this version does not write are kept):
//!
//! - `"window"`: `{x, y, width, height, maximized}`, the window's outer position and inner size in
//!   points as last seen while it was neither maximised nor full screen ([`WindowState`]);
//! - `"view"`: `{treeWidth, pane, paneLarge}`, the view state restored at the next start
//!   ([`ViewState`], `ui-layout.md` §2); M4 adds the library view's options to the same object,
//!   `{thumbSize, treeMode, folderSort, folderSortReverse, caption, smallPictures,
//!   hiddenPictures}` ([`ViewOptions`]);
//! - `"interface"`: `{systemTitleBar}`, Options → General → Interface → Use system title bar
//!   (`design-system.md` §11.1).
//!
//! M4 adds `"scrolling"`: `{thumbExponent, thumbTopSpeed}`, the light table's speed-control thumb
//! ([`ThumbSpeed`], `ui-layout.md` §5.6), read only, so the curve can be tuned by hand in the file
//! without a rebuild.
//!
//! M5 adds `"fileTypes"`: `{png, gif, bmp, tiff, webp, heif, avif, jxl, psd, raw, video}`, Options →
//! File Types (`data-model.md` §7): each `false` leaves that type out of every scan ([`FileTypes`];
//! JPEG is always on, and a value that is not a boolean counts as on).
//!
//! M5 also adds `"view"` → `colorManagement` (View → Use Color Management, on unless stored as
//! `false`) and `"cache"`: `{thumbnailBytes}`, the thumbnail cache's size (2 GiB by default; a value
//! that is not a whole number of bytes in `dalica_thumbs::upkeep::MIN_BUDGET ..= MAX_BUDGET` is the
//! default).
//!
//! M6 adds two objects, merged key by key like the others (M6.5):
//!
//! - `"files"`: Options → General → Files, the metadata writes (`menus-and-shortcuts.md` §5,
//!   `xmp-interop.md` §1–§2; [`MetadataWrites`]): `{writeCaptionsTagsPlaces, writeStarsAsRatings,
//!   updateModificationTime, writeRawXmpSidecars, writeDates}`, on, off, off, off, on by default.
//!   Their checkboxes arrive with the Options dialog (M10).
//! - `"tags"`: the Tags pane's quick tags and recently used tags (`organizing.md` §5.2;
//!   [`QuickTags`]): `{quick, useRecent, recent}`, `quick` ten slots of tag text or `null`,
//!   `useRecent` "Use the top two buttons for recently used tags" (on by default), `recent` the
//!   recently used tags, most recent first (at most [`MAX_RECENT_TAGS`]).
//!
//! M7 adds `"edit"`, merged key by key the same way (M7.3; [`EditSettings`]): `{cropRatios,
//! lastCropRatio, photoSizes, singleClickLeaves}`, the Crop panel's custom ratios (`{name, width,
//! height}` each, at most [`MAX_CROP_RATIOS`]) and the last ratio used (`editing.md` §6.1), the
//! photo sizes it lists (`"us"` or `"metric"`; unset, US when the system's region is the US), and
//! "Single-click to leave the edit view" (off; `editing.md` §1).
//!
//! M8 adds two more, merged key by key the same way (M8.3; `data-model.md` §6, `creating.md` §2):
//!
//! - `"save"`: `{explain, lastFolder, lastFormat}` ([`SaveSettings`]): the first-time Save
//!   explanation (on), the folder and format Save As last used (none; `"jpeg"` or `"png"`, JPEG).
//! - `"export"`: `{location, resize, size, quality, customQuality, numbers, watermark,
//!   watermarkText, removeLocation}` ([`ExportSettings`]): the export location (unset:
//!   `<Pictures>/Dalica/Exports`, [`default_export_location`]), "Resize to" 1600 (on; sizes in
//!   [`EXPORT_SIZE_RANGE`]), quality Automatic (`"automatic"`, `"normal"`, `"maximum"`,
//!   `"minimum"` or `"custom"`) with the Custom value 75, numbers off, the watermark off with empty
//!   text, and Options → Export → "Remove location from exported photos" off. The projects
//!   directory the tree lists in Projects is always `<Pictures>/Dalica`, whatever location is
//!   chosen ([`Session::projects_dir`](crate::Session::projects_dir), M9.28).
//!
//! M9 adds `"organize"`, merged key by key the same way (M9.3; `organizing.md` §4 and §9,
//! `menus-and-shortcuts.md` §5; [`OrganizeSettings`]): `{askBeforeMoving,
//! removeFromAlbumsWithoutAsking, deleteWithoutAsking, recentDays, lastMoveFolder}`: the first-time
//! move question (on; its "Don't ask again" turns it off), Options → General → Files → "Remove from
//! albums without asking" and "Delete from disk without asking" (off, off), the days Recently
//! Updated covers (30, in [`RECENT_DAYS_RANGE`]) and the folder Move Folder last moved into (none).
//!
//! The command modules read them through [`Session::settings`](crate::Session::settings) and change
//! them through [`Session::change_settings`](crate::Session::change_settings) with the typed
//! accessors below. A value of the wrong type is the default and is reported once, when the library
//! opens ([`Settings::problems`], in `Session::notices`), by its key only: tag text never reaches a
//! notice or a log (`security.md` §4.4).
//!
//! Stored numbers are hostile like any input: a value that is not a finite number in its range is
//! dropped (the default is used), never trusted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dalica_core::{DateTime, Timestamp};
use dalica_scan::{FileType, FileTypes};
use serde_json::{Map, Value, json};

use crate::keymap::{Key, Override};
use crate::registry::Context;

/// The settings file's name in the library directory.
pub const FILE: &str = "settings.json";
/// The largest settings file read.
const MAX_BYTES: u64 = 64 << 20;

/// The window's default and smallest inner size in points (`ui-layout.md` §2), and the largest
/// side a stored size may have.
pub const WINDOW_DEFAULT: [f32; 2] = [1400.0, 900.0];
pub const WINDOW_MIN: [f32; 2] = [900.0, 600.0];
pub const WINDOW_MAX_SIDE: f32 = 16384.0;
/// The farthest from the origin a stored window position may be, in points.
pub const POSITION_LIMIT: f32 = 100_000.0;
/// The widest a stored library-tree width may be, in points (the layout clamps it to the window).
pub const TREE_WIDTH_LIMIT: f32 = 16384.0;
/// The longest stored side-pane name read.
const MAX_NAME: usize = 64;

/// The window as last seen (`"window"` in `settings.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowState {
    /// The outer top-left corner in points; `None` where the platform does not tell (Wayland).
    pub position: Option<[f32; 2]>,
    /// The inner size in points while not maximised, within `WINDOW_MIN ..= WINDOW_MAX_SIDE`.
    pub size: [f32; 2],
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        WindowState { position: None, size: WINDOW_DEFAULT, maximized: false }
    }
}

impl WindowState {
    /// This state with every number in its range: a size that is not finite is the default, one
    /// out of range is clamped; a position that is not finite or farther than `POSITION_LIMIT` is
    /// dropped.
    pub fn sanitized(self) -> WindowState {
        let side = |v: f32, min: f32, default: f32| if v.is_finite() { v.clamp(min, WINDOW_MAX_SIDE) } else { default };
        let size = [side(self.size[0], WINDOW_MIN[0], WINDOW_DEFAULT[0]), side(self.size[1], WINDOW_MIN[1], WINDOW_DEFAULT[1])];
        let position = self.position.filter(|p| p.iter().all(|v| v.is_finite() && v.abs() <= POSITION_LIMIT));
        WindowState { position, size, maximized: self.maximized }
    }
}

/// The view state kept between runs (`"view"` in `settings.json`): what `ui-layout.md` §2 says is
/// persisted. The pane is kept by its protocol name (`"tags"`); the UI maps it to its own type and
/// treats a name it does not know as closed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    /// The library tree's width in points as the user set it; `None` for the default.
    pub tree_width: Option<f32>,
    /// The open side pane's name, `None` when closed.
    pub pane: Option<String>,
    /// The side pane at its large width.
    pub pane_large: bool,
}

/// The library view's options kept between runs (M4; more keys of `"view"` in `settings.json`):
/// the thumbnail size and what the View menu and the view-options popup choose. Names are kept as
/// their protocol names (`"tree"`, `"recent"`, `"filename"`); the UI maps them to its own types and
/// treats a name it does not know as the default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewOptions {
    /// The thumbnail cell edge in points; `None` for the default.
    pub thumb_size: Option<f32>,
    /// `"flat"` or `"tree"`.
    pub tree_mode: Option<String>,
    /// `"date"`, `"recent"`, `"size"` or `"name"`.
    pub folder_sort: Option<String>,
    pub folder_sort_reverse: bool,
    /// The thumbnail caption: `"none"`, `"filename"`, …
    pub caption: Option<String>,
    pub small_pictures: bool,
    pub hidden_pictures: bool,
}

/// The largest stored thumbnail size read, in points (the UI clamps it to its own range).
pub const THUMB_SIZE_LIMIT: f32 = 4096.0;

/// The light table's speed-control thumb (`"scrolling"` in `settings.json`, `ui-layout.md` §5.6):
/// past the dead zone, the scroll speed is `topSpeed · u^exponent` screen heights per second, `u`
/// the thumb's distance from the centre as a share of the way to the track's end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbSpeed {
    /// The curve's exponent (`thumbExponent`): 1 linear, 2 quadratic (the default).
    pub exponent: f32,
    /// The speed at the track's end in screen heights per second (`thumbTopSpeed`; default 40).
    pub top_speed: f32,
}

impl Default for ThumbSpeed {
    fn default() -> Self {
        ThumbSpeed { exponent: THUMB_EXPONENT_DEFAULT, top_speed: THUMB_TOP_SPEED_DEFAULT }
    }
}

/// The default and the range of a stored [`ThumbSpeed::exponent`]; a value outside it is dropped.
pub const THUMB_EXPONENT_DEFAULT: f32 = 2.0;
pub const THUMB_EXPONENT_RANGE: std::ops::RangeInclusive<f32> = 0.25..=8.0;
/// The default and the range of a stored [`ThumbSpeed::top_speed`] (screen heights per second).
pub const THUMB_TOP_SPEED_DEFAULT: f32 = 40.0;
pub const THUMB_TOP_SPEED_RANGE: std::ops::RangeInclusive<f32> = 0.5..=400.0;

/// Options → General → Files: which owned metadata fields Dalica writes into photo files, and how
/// (`"files"` in `settings.json`; `xmp-interop.md` §1–§2). With an option off, the field is still
/// read but never written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetadataWrites {
    /// Write captions, tags and places into photo files (`writeCaptionsTagsPlaces`; on).
    pub captions_tags_places: bool,
    /// Write stars as ratings (XMP) (`writeStarsAsRatings`; off).
    pub stars_as_ratings: bool,
    /// Update file modification time on metadata changes (`updateModificationTime`; off).
    pub update_modification_time: bool,
    /// Write XMP sidecars for RAW files (`writeRawXmpSidecars`; off).
    pub raw_xmp_sidecars: bool,
    /// Write date changes to files (`writeDates`; on).
    pub dates: bool,
}

impl Default for MetadataWrites {
    fn default() -> Self {
        MetadataWrites { captions_tags_places: true, stars_as_ratings: false, update_modification_time: false, raw_xmp_sidecars: false, dates: true }
    }
}

impl MetadataWrites {
    /// Each option with its key in `"files"`.
    fn keyed(&self) -> [(&'static str, bool); 5] {
        [
            ("writeCaptionsTagsPlaces", self.captions_tags_places),
            ("writeStarsAsRatings", self.stars_as_ratings),
            ("updateModificationTime", self.update_modification_time),
            ("writeRawXmpSidecars", self.raw_xmp_sidecars),
            ("writeDates", self.dates),
        ]
    }

    /// The options from `values` in [`MetadataWrites::keyed`] order.
    fn from_keyed(v: [bool; 5]) -> MetadataWrites {
        let [captions_tags_places, stars_as_ratings, update_modification_time, raw_xmp_sidecars, dates] = v;
        MetadataWrites { captions_tags_places, stars_as_ratings, update_modification_time, raw_xmp_sidecars, dates }
    }
}

/// The number of quick tags (`organizing.md` §5.2).
pub const QUICK_TAGS: usize = 10;
/// The most recently used tags kept.
pub const MAX_RECENT_TAGS: usize = 10;
/// The longest stored quick or recent tag read, in bytes; a longer one is dropped.
pub const MAX_TAG_BYTES: usize = 1024;

/// The quick tags (`"tags"` → `quick` and `useRecent` in `settings.json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickTags {
    /// The ten slots, `None` unassigned (shown as "+"). Tag text is trimmed and never empty.
    pub slots: [Option<String>; QUICK_TAGS],
    /// "Use the top two buttons for recently used tags" (on).
    pub use_recent: bool,
}

impl Default for QuickTags {
    fn default() -> Self {
        QuickTags { slots: Default::default(), use_recent: true }
    }
}

/// Stored tag text worth keeping: trimmed, not empty, at most [`MAX_TAG_BYTES`].
fn tag_text(t: &str) -> Option<String> {
    let t = t.trim();
    (!t.is_empty() && t.len() <= MAX_TAG_BYTES).then(|| t.to_string())
}

/// `tags` as tag text: each kept by [`tag_text`], the first of equal ones kept, at most
/// [`MAX_RECENT_TAGS`].
fn recent_list<'a>(tags: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags.into_iter().filter_map(tag_text) {
        if out.len() == MAX_RECENT_TAGS {
            break;
        }
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// The most custom crop ratios kept.
pub const MAX_CROP_RATIOS: usize = 100;
/// The longest custom ratio name, or stored last ratio, read, in bytes.
pub const MAX_RATIO_NAME: usize = 256;
/// The largest side of a custom crop ratio (`4 × 6` has sides 4 and 6).
pub const MAX_RATIO_SIDE: f64 = 100_000.0;

/// One custom crop ratio of the Crop panel (`editing.md` §6.1): "Width × Height" and a name.
#[derive(Clone, Debug, PartialEq)]
pub struct CropRatio {
    /// Trimmed, not empty, at most [`MAX_RATIO_NAME`] bytes; unique among the ratios.
    pub name: String,
    /// Finite, above 0, at most [`MAX_RATIO_SIDE`].
    pub width: f64,
    pub height: f64,
}

impl CropRatio {
    /// The ratio as kept: the name trimmed; `None` when the name or a side is out of bounds.
    pub fn checked(&self) -> Option<CropRatio> {
        let name = self.name.trim();
        let side = |v: f64| v.is_finite() && v > 0.0 && v <= MAX_RATIO_SIDE;
        (!name.is_empty() && name.len() <= MAX_RATIO_NAME && side(self.width) && side(self.height)).then(|| CropRatio {
            name: name.to_string(),
            width: self.width,
            height: self.height,
        })
    }
}

/// Which photo sizes the Crop panel lists (`editing.md` §6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhotoSizes {
    /// "4 × 6", "5 × 7", "8 × 10", "8.5 × 11 (Letter)".
    Us,
    /// "10 × 15", "13 × 18", "20 × 25", "A4".
    Metric,
}

impl PhotoSizes {
    /// The stored text.
    pub fn id(self) -> &'static str {
        match self {
            PhotoSizes::Us => "us",
            PhotoSizes::Metric => "metric",
        }
    }

    /// The sizes stored as `id`.
    pub fn from_id(id: &str) -> Option<PhotoSizes> {
        match id {
            "us" => Some(PhotoSizes::Us),
            "metric" => Some(PhotoSizes::Metric),
            _ => None,
        }
    }

    /// The sizes for a locale name such as `en_US.UTF-8` or `en-US`: US when its region is the US,
    /// metric otherwise (and when there is none).
    pub fn for_locale(locale: Option<&str>) -> PhotoSizes {
        let region = locale.and_then(|l| l.split(['.', '@']).next()).and_then(|l| l.split(['_', '-']).nth(1));
        if region.is_some_and(|r| r.eq_ignore_ascii_case("US")) { PhotoSizes::Us } else { PhotoSizes::Metric }
    }

    /// The system's default: from the locale the environment names for paper sizes (`LC_ALL`, then
    /// `LC_PAPER`, then `LANG`, the first one set and not empty), metric when none is.
    pub fn system() -> PhotoSizes {
        let locale = ["LC_ALL", "LC_PAPER", "LANG"].into_iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty());
        PhotoSizes::for_locale(locale.as_deref())
    }
}

/// The edit view's settings (`"edit"` in `settings.json`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditSettings {
    /// The Crop panel's custom ratios, in the order added.
    pub crop_ratios: Vec<CropRatio>,
    /// The ratio last used in the Crop panel, by the Crop panel's own name for it.
    pub last_crop_ratio: Option<String>,
    /// The photo sizes listed; `None` follows the system ([`PhotoSizes::system`]).
    pub photo_sizes: Option<PhotoSizes>,
    /// "Single-click to leave the edit view" (off).
    pub single_click_leaves: bool,
}

impl EditSettings {
    /// The photo sizes listed: the stored choice, else the system's.
    pub fn photo_sizes(&self) -> PhotoSizes {
        self.photo_sizes.unwrap_or_else(PhotoSizes::system)
    }
}

/// The longest stored folder path read (the export location, the last Save As folder), in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// The longest stored watermark text read, in bytes.
pub const MAX_WATERMARK_BYTES: usize = 1024;
/// The export's "Resize to" size in pixels of the long side: the dialog slider's range
/// (`creating.md` §2); a stored size outside it is the default.
pub const EXPORT_SIZE_RANGE: std::ops::RangeInclusive<u32> = 100..=4000;
/// The default "Resize to" size.
pub const EXPORT_SIZE_DEFAULT: u32 = 1600;
/// The range of the Custom export quality.
pub const QUALITY_RANGE: std::ops::RangeInclusive<u8> = 1..=100;
/// The default Custom export quality.
pub const CUSTOM_QUALITY_DEFAULT: u8 = 75;

/// A stored folder worth keeping: absolute, valid Unicode, not empty, at most [`MAX_PATH_BYTES`].
fn folder_path(p: &Path) -> Option<&str> {
    p.to_str().filter(|s| !s.is_empty() && s.len() <= MAX_PATH_BYTES && !s.contains('\0') && p.is_absolute())
}

/// Where exports go when no location is stored: `<Pictures>/Dalica/Exports` (`creating.md` §2).
pub fn default_export_location(pictures: &Path) -> PathBuf {
    projects_dir_under(pictures).join("Exports")
}

/// The projects directory under the Pictures folder `pictures`: `<Pictures>/Dalica`
/// ([`Session::projects_dir`](crate::Session::projects_dir)).
pub fn projects_dir_under(pictures: &Path) -> PathBuf {
    pictures.join("Dalica")
}

/// The format Save As writes (`"save"` → `lastFormat` in `settings.json`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SaveFormat {
    #[default]
    Jpeg,
    Png,
}

impl SaveFormat {
    /// The stored text.
    pub fn id(self) -> &'static str {
        match self {
            SaveFormat::Jpeg => "jpeg",
            SaveFormat::Png => "png",
        }
    }

    pub fn from_id(id: &str) -> Option<SaveFormat> {
        match id {
            "jpeg" => Some(SaveFormat::Jpeg),
            "png" => Some(SaveFormat::Png),
            _ => None,
        }
    }
}

/// Save's settings (`"save"` in `settings.json`; `data-model.md` §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveSettings {
    /// Explain what Save does before the first one (`explain`; on). "Don't ask again" turns it off.
    pub explain: bool,
    /// The folder Save As last wrote into (`lastFolder`); `None` until the first Save As.
    pub last_folder: Option<PathBuf>,
    /// The format Save As last wrote (`lastFormat`; JPEG).
    pub last_format: SaveFormat,
}

impl Default for SaveSettings {
    fn default() -> Self {
        SaveSettings { explain: true, last_folder: None, last_format: SaveFormat::Jpeg }
    }
}

/// The days Recently Updated covers by default (`organizing.md` §4.1), and the range a stored
/// value must lie in.
pub const RECENT_DAYS_DEFAULT: u32 = 30;
pub const RECENT_DAYS_RANGE: std::ops::RangeInclusive<u32> = 1..=3650;

/// Organizing's settings (`"organize"` in `settings.json`; `organizing.md` §4 and §9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrganizeSettings {
    /// Ask "Move these files on disk?" before a move (`askBeforeMoving`; on). "Don't ask again"
    /// turns it off.
    pub ask_before_moving: bool,
    /// Options → General → Files → "Remove from albums without asking"
    /// (`removeFromAlbumsWithoutAsking`; off).
    pub remove_from_albums_without_asking: bool,
    /// Options → General → Files → "Delete from disk without asking" (`deleteWithoutAsking`; off).
    pub delete_without_asking: bool,
    /// The days Recently Updated covers (`recentDays`; [`RECENT_DAYS_DEFAULT`], in
    /// [`RECENT_DAYS_RANGE`]).
    pub recent_days: u32,
    /// The folder Move Folder last moved a folder into (`lastMoveFolder`; absolute, at most
    /// [`MAX_PATH_BYTES`]); `None` until the first one.
    pub last_move_folder: Option<PathBuf>,
}

impl Default for OrganizeSettings {
    fn default() -> Self {
        OrganizeSettings {
            ask_before_moving: true,
            remove_from_albums_without_asking: false,
            delete_without_asking: false,
            recent_days: RECENT_DAYS_DEFAULT,
            last_move_folder: None,
        }
    }
}

/// The export's "Image quality" (`creating.md` §2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportQuality {
    /// Keeps the original's quality (estimated from its quantisation tables).
    #[default]
    Automatic,
    /// 85.
    Normal,
    /// 95.
    Maximum,
    /// 60.
    Minimum,
    /// The Custom slider's value ([`ExportSettings::custom_quality`]).
    Custom,
}

impl ExportQuality {
    pub const ALL: [ExportQuality; 5] =
        [ExportQuality::Automatic, ExportQuality::Normal, ExportQuality::Maximum, ExportQuality::Minimum, ExportQuality::Custom];

    /// The stored text.
    pub fn id(self) -> &'static str {
        match self {
            ExportQuality::Automatic => "automatic",
            ExportQuality::Normal => "normal",
            ExportQuality::Maximum => "maximum",
            ExportQuality::Minimum => "minimum",
            ExportQuality::Custom => "custom",
        }
    }

    pub fn from_id(id: &str) -> Option<ExportQuality> {
        ExportQuality::ALL.into_iter().find(|q| q.id() == id)
    }

    /// The JPEG quality of a preset; `None` for Automatic and Custom.
    pub fn preset(self) -> Option<u8> {
        match self {
            ExportQuality::Normal => Some(85),
            ExportQuality::Maximum => Some(95),
            ExportQuality::Minimum => Some(60),
            ExportQuality::Automatic | ExportQuality::Custom => None,
        }
    }
}

/// The Export dialog's choices and Options → Export, remembered between exports (`"export"` in
/// `settings.json`; `creating.md` §2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportSettings {
    /// The export location as the user chose it (`location`); `None`: `<Pictures>/Dalica/Exports`
    /// ([`Session::export_location`](crate::Session::export_location)).
    pub location: Option<PathBuf>,
    /// "Resize to" (on) rather than "Use original size" (`resize`).
    pub resize: bool,
    /// The long side to resize to, in [`EXPORT_SIZE_RANGE`] (`size`; 1600).
    pub size: u32,
    /// `quality`: Automatic.
    pub quality: ExportQuality,
    /// The Custom slider's value, in [`QUALITY_RANGE`] (`customQuality`; 75), kept while another
    /// preset is chosen.
    pub custom_quality: u8,
    /// "Add numbers to file names to keep the order" (`numbers`; off).
    pub numbers: bool,
    /// "Add watermark" (`watermark`; off).
    pub watermark: bool,
    /// The watermark's text (`watermarkText`; empty), at most [`MAX_WATERMARK_BYTES`].
    pub watermark_text: String,
    /// Options → Export → "Remove location from exported photos" (`removeLocation`; off).
    pub remove_location: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        ExportSettings {
            location: None,
            resize: true,
            size: EXPORT_SIZE_DEFAULT,
            quality: ExportQuality::Automatic,
            custom_quality: CUSTOM_QUALITY_DEFAULT,
            numbers: false,
            watermark: false,
            watermark_text: String::new(),
            remove_location: false,
        }
    }
}

impl ExportSettings {
    /// The JPEG quality chosen: a preset's, the Custom value, or `None` for Automatic.
    pub fn jpeg_quality(&self) -> Option<u8> {
        match self.quality {
            ExportQuality::Custom => Some(self.custom_quality),
            q => q.preset(),
        }
    }

    /// The long side to resize to; `None` for the original size.
    pub fn long_side(&self) -> Option<u32> {
        self.resize.then_some(self.size)
    }
}

/// `ratios` as kept: each [`CropRatio::checked`], the first of equal names kept, at most
/// [`MAX_CROP_RATIOS`].
fn ratio_list<'a>(ratios: impl IntoIterator<Item = &'a CropRatio>) -> Vec<CropRatio> {
    let mut out: Vec<CropRatio> = Vec::new();
    for r in ratios.into_iter().filter_map(CropRatio::checked) {
        if out.len() == MAX_CROP_RATIOS {
            break;
        }
        if !out.iter().any(|o| o.name == r.name) {
            out.push(r);
        }
    }
    out
}

/// The settings document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    root: Map<String, Value>,
}

impl Settings {
    /// Parses a settings file; `Err` (with the reason) unless it is a JSON object.
    pub fn parse(bytes: &[u8]) -> Result<Settings, String> {
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        match serde_json::from_slice::<Value>(bytes) {
            Ok(Value::Object(root)) => Ok(Settings { root }),
            Ok(_) => Err("the top level is not an object".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// The file as written: sorted keys, two spaces, a final newline.
    pub fn print(&self) -> Vec<u8> {
        let mut out = serde_json::to_vec_pretty(&Value::Object(self.root.clone())).unwrap_or_default();
        out.push(b'\n');
        out
    }

    /// Reads `<dir>/settings.json`: defaults when absent; a damaged file is set aside (named by
    /// `now`) and the reason returned with the defaults.
    pub fn load(dir: &Path, now: Timestamp) -> (Settings, Option<String>) {
        let path = dir.join(FILE);
        let damaged = |reason: String| {
            let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
            let kept = dalica_fs::set_aside(&path, &format!("corrupt-{stamp}Z")).map(|p| p.display().to_string()).unwrap_or_default();
            (Settings::default(), Some(format!("settings.json: {reason}; kept as {kept}, defaults used")))
        };
        match dalica_fs::read(&path, MAX_BYTES) {
            Ok(None) => (Settings::default(), None),
            Ok(Some(r)) => match Settings::parse(&r.bytes) {
                Ok(s) => (s, None),
                Err(reason) => damaged(reason),
            },
            Err(dalica_fs::Error::TooLarge { .. }) => damaged("too large".into()),
            Err(e) => (Settings::default(), Some(format!("settings.json: {e}"))),
        }
    }

    /// The user's shortcut overrides for commands `known` accepts.
    pub fn keymap(&self, known: &dyn Fn(&str) -> bool) -> Vec<Override> {
        let mut grouped: BTreeMap<(String, Context), Vec<Key>> = BTreeMap::new();
        for entry in self.root.get("keymap").and_then(Value::as_array).into_iter().flatten() {
            if let Some((command, context, key)) = read_entry(entry).filter(|(c, ..)| known(c)) {
                let keys = grouped.entry((command, context)).or_default();
                keys.extend(key);
            }
        }
        grouped.into_iter().map(|((command, context), keys)| Override { command, context, keys }).collect()
    }

    /// Stores `overrides` under `"keymap"`, keeping entries for commands `known` rejects and
    /// entries that cannot be read.
    pub fn set_keymap(&mut self, overrides: &[Override], known: &dyn Fn(&str) -> bool) {
        let mut entries: Vec<Value> = self
            .root
            .get("keymap")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|e| read_entry(e).is_none_or(|(c, ..)| !known(&c)))
            .cloned()
            .collect();
        for o in overrides {
            let context = serde_json::to_value(o.context).unwrap_or(Value::Null);
            if o.keys.is_empty() {
                entries.push(json!({ "command": o.command, "context": context, "key": null }));
            }
            for k in &o.keys {
                entries.push(json!({ "command": o.command, "context": context, "key": k.to_string() }));
            }
        }
        self.root.insert("keymap".into(), Value::Array(entries));
    }
}

impl Settings {
    /// The value stored under the top-level `key`, if any.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.root.get(key)
    }

    /// Merges `fields` into the object stored under `key` (made when absent; a value there that is
    /// not an object is replaced): each field is set, `None` removes it, and every other key of the
    /// object is kept verbatim.
    pub fn merge(&mut self, key: &str, fields: &[(&str, Option<Value>)]) {
        let slot = self.root.entry(key.to_string()).or_insert_with(|| Value::Object(Map::new()));
        if !slot.is_object() {
            *slot = Value::Object(Map::new());
        }
        if let Value::Object(obj) = slot {
            for (k, v) in fields {
                match v {
                    Some(v) => {
                        obj.insert((*k).to_string(), v.clone());
                    }
                    None => {
                        obj.remove(*k);
                    }
                }
            }
        }
    }

    /// A field of the object under `key`.
    fn field(&self, key: &str, field: &str) -> Option<&Value> {
        self.root.get(key)?.as_object()?.get(field)
    }

    /// The stored window state, with hostile numbers replaced ([`WindowState::sanitized`]); `None`
    /// when none is stored.
    pub fn window(&self) -> Option<WindowState> {
        let obj = self.root.get("window")?.as_object()?;
        let num = |k: &str| obj.get(k).and_then(Value::as_f64).map(|v| v as f32);
        let position = match (num("x"), num("y")) {
            (Some(x), Some(y)) => Some([x, y]),
            _ => None,
        };
        let size = [num("width").unwrap_or(f32::NAN), num("height").unwrap_or(f32::NAN)];
        let maximized = obj.get("maximized").and_then(Value::as_bool).unwrap_or(false);
        Some(WindowState { position, size, maximized }.sanitized())
    }

    /// Stores `w` (sanitized) under `"window"`. A position the platform did not tell leaves the
    /// stored one as it is.
    pub fn set_window(&mut self, w: &WindowState) {
        let w = w.sanitized();
        let mut fields = vec![("width", Some(json!(w.size[0]))), ("height", Some(json!(w.size[1]))), ("maximized", Some(json!(w.maximized)))];
        if let Some([x, y]) = w.position {
            fields.extend([("x", Some(json!(x))), ("y", Some(json!(y)))]);
        }
        self.merge("window", &fields);
    }

    /// The stored view state; hostile values are dropped (a tree width that is not a finite number
    /// in `0 < w ≤ TREE_WIDTH_LIMIT`, a pane that is not a short string).
    pub fn view(&self) -> ViewState {
        let tree_width =
            self.field("view", "treeWidth").and_then(Value::as_f64).map(|v| v as f32).filter(|w| w.is_finite() && *w > 0.0 && *w <= TREE_WIDTH_LIMIT);
        let pane = self.field("view", "pane").and_then(Value::as_str).filter(|p| !p.is_empty() && p.len() <= MAX_NAME).map(str::to_string);
        let pane_large = self.field("view", "paneLarge").and_then(Value::as_bool).unwrap_or(false);
        ViewState { tree_width, pane, pane_large }
    }

    /// Stores `v` under `"view"` (a closed pane as `null`).
    pub fn set_view(&mut self, v: &ViewState) {
        let tree = v.tree_width.filter(|w| w.is_finite() && *w > 0.0 && *w <= TREE_WIDTH_LIMIT).map(|w| json!(w));
        let pane = Some(v.pane.as_ref().map_or(Value::Null, |p| json!(p)));
        self.merge("view", &[("treeWidth", tree), ("pane", pane), ("paneLarge", Some(json!(v.pane_large)))]);
    }

    /// The stored view options; hostile values are dropped (a thumbnail size that is not a finite
    /// number in `0 < s ≤ THUMB_SIZE_LIMIT`, a name that is not a short string, a flag that is not
    /// a boolean).
    pub fn view_options(&self) -> ViewOptions {
        let name = |k: &str| self.field("view", k).and_then(Value::as_str).filter(|p| !p.is_empty() && p.len() <= MAX_NAME).map(str::to_string);
        let flag = |k: &str| self.field("view", k).and_then(Value::as_bool).unwrap_or(false);
        ViewOptions {
            thumb_size: self.field("view", "thumbSize").and_then(Value::as_f64).map(|v| v as f32).filter(|s| thumb_size_ok(*s)),
            tree_mode: name("treeMode"),
            folder_sort: name("folderSort"),
            folder_sort_reverse: flag("folderSortReverse"),
            caption: name("caption"),
            small_pictures: flag("smallPictures"),
            hidden_pictures: flag("hiddenPictures"),
        }
    }

    /// Stores `v` under `"view"`, next to [`ViewState`]'s keys (a value of `None` removes its key).
    pub fn set_view_options(&mut self, v: &ViewOptions) {
        let name = |n: &Option<String>| n.as_ref().filter(|p| !p.is_empty() && p.len() <= MAX_NAME).map(|p| json!(p));
        self.merge(
            "view",
            &[
                ("thumbSize", v.thumb_size.filter(|s| thumb_size_ok(*s)).map(|s| json!(s))),
                ("treeMode", name(&v.tree_mode)),
                ("folderSort", name(&v.folder_sort)),
                ("folderSortReverse", Some(json!(v.folder_sort_reverse))),
                ("caption", name(&v.caption)),
                ("smallPictures", Some(json!(v.small_pictures))),
                ("hiddenPictures", Some(json!(v.hidden_pictures))),
            ],
        );
    }

    /// Options → General → Interface → Use system title bar (Windows and Linux; default off).
    pub fn system_title_bar(&self) -> bool {
        self.field("interface", "systemTitleBar").and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn set_system_title_bar(&mut self, on: bool) {
        self.merge("interface", &[("systemTitleBar", Some(json!(on)))]);
    }

    /// The speed-control thumb's curve; each value that is not a number in its range is the default.
    pub fn thumb_speed(&self) -> ThumbSpeed {
        let num = |k: &str, range: &std::ops::RangeInclusive<f32>| {
            self.field("scrolling", k).and_then(Value::as_f64).map(|v| v as f32).filter(|v| v.is_finite() && range.contains(v))
        };
        ThumbSpeed {
            exponent: num("thumbExponent", &THUMB_EXPONENT_RANGE).unwrap_or(THUMB_EXPONENT_DEFAULT),
            top_speed: num("thumbTopSpeed", &THUMB_TOP_SPEED_RANGE).unwrap_or(THUMB_TOP_SPEED_DEFAULT),
        }
    }

    /// Stores `v` under `"scrolling"`; a value out of its range is stored as its default.
    pub fn set_thumb_speed(&mut self, v: &ThumbSpeed) {
        let ok = |x: f32, range: &std::ops::RangeInclusive<f32>, default: f32| if x.is_finite() && range.contains(&x) { x } else { default };
        self.merge(
            "scrolling",
            &[
                ("thumbExponent", Some(json!(ok(v.exponent, &THUMB_EXPONENT_RANGE, THUMB_EXPONENT_DEFAULT)))),
                ("thumbTopSpeed", Some(json!(ok(v.top_speed, &THUMB_TOP_SPEED_RANGE, THUMB_TOP_SPEED_DEFAULT)))),
            ],
        );
    }

    /// View → Use Color Management: on unless stored as `false`.
    pub fn color_management(&self) -> bool {
        self.field("view", "colorManagement").and_then(Value::as_bool).unwrap_or(true)
    }

    pub fn set_color_management(&mut self, on: bool) {
        self.merge("view", &[("colorManagement", Some(json!(on)))]);
    }

    /// The thumbnail cache's size in bytes: the stored `cache.thumbnailBytes` when it is a whole
    /// number in `MIN_BUDGET ..= MAX_BUDGET`, else 2 GiB.
    pub fn thumbnail_cache_bytes(&self) -> u64 {
        self.field("cache", "thumbnailBytes")
            .and_then(Value::as_u64)
            .filter(|b| (dalica_thumbs::upkeep::MIN_BUDGET..=dalica_thumbs::upkeep::MAX_BUDGET).contains(b))
            .unwrap_or(dalica_thumbs::pack::DEFAULT_BUDGET)
    }

    /// Stores the thumbnail cache's size, clamped to `MIN_BUDGET ..= MAX_BUDGET`.
    pub fn set_thumbnail_cache_bytes(&mut self, bytes: u64) {
        let bytes = bytes.clamp(dalica_thumbs::upkeep::MIN_BUDGET, dalica_thumbs::upkeep::MAX_BUDGET);
        self.merge("cache", &[("thumbnailBytes", Some(json!(bytes)))]);
    }

    /// Options → File Types: the types scans list. Every type is on unless stored as `false`.
    pub fn file_types(&self) -> FileTypes {
        FileType::ALL.into_iter().fold(FileTypes::all(), |t, ty| match self.field("fileTypes", ty.id()).and_then(Value::as_bool) {
            Some(false) => t.with(ty, false),
            _ => t,
        })
    }

    /// Stores `types` under `"fileTypes"`, one flag per type but JPEG (always on); other keys of the
    /// object are kept.
    pub fn set_file_types(&mut self, types: FileTypes) {
        let fields: Vec<(&str, Option<Value>)> =
            FileType::ALL.into_iter().filter(|t| *t != FileType::Jpeg).map(|t| (t.id(), Some(json!(types.allows(t))))).collect();
        self.merge("fileTypes", &fields);
    }

    /// Options → General → Files: the metadata write options; each value that is not `true` or
    /// `false` is its default.
    pub fn metadata_writes(&self) -> MetadataWrites {
        self.read_metadata_writes(&mut Vec::new())
    }

    /// Stores every metadata write option under `"files"`; other keys of the object are kept.
    pub fn set_metadata_writes(&mut self, w: &MetadataWrites) {
        let fields: Vec<(&str, Option<Value>)> = w.keyed().into_iter().map(|(k, v)| (k, Some(json!(v)))).collect();
        self.merge("files", &fields);
    }

    /// The quick tags; a slot that is not tag text (a string, trimmed, not empty, at most
    /// [`MAX_TAG_BYTES`]) is unassigned, entries past the tenth are ignored, and a `useRecent` that
    /// is not a boolean is on.
    pub fn quick_tags(&self) -> QuickTags {
        self.read_quick_tags(&mut Vec::new())
    }

    /// Stores the quick tags under `"tags"` (unassigned slots as `null`; text kept by the same
    /// rules as reading); `recent` and other keys of the object are kept.
    pub fn set_quick_tags(&mut self, q: &QuickTags) {
        let slots: Vec<Value> = q.slots.iter().map(|s| s.as_deref().and_then(tag_text).map_or(Value::Null, Value::String)).collect();
        self.merge("tags", &[("quick", Some(Value::Array(slots))), ("useRecent", Some(json!(q.use_recent)))]);
    }

    /// The recently used tags, most recent first: at most [`MAX_RECENT_TAGS`], each tag text once;
    /// entries that are not tag text are left out.
    pub fn recent_tags(&self) -> Vec<String> {
        self.read_recent_tags(&mut Vec::new())
    }

    /// Stores `tags` (most recent first) as the recently used tags, kept by the reading rules.
    pub fn set_recent_tags(&mut self, tags: &[String]) {
        let list = recent_list(tags.iter().map(String::as_str));
        self.merge("tags", &[("recent", Some(json!(list)))]);
    }

    /// Records `tags` as just used: they go first, in their order, ahead of the tags used before
    /// (an equal text appears once); the oldest fall off past [`MAX_RECENT_TAGS`].
    pub fn note_recent_tags(&mut self, tags: &[String]) {
        let before = self.recent_tags();
        let list = recent_list(tags.iter().chain(before.iter()).map(String::as_str));
        self.merge("tags", &[("recent", Some(json!(list)))]);
    }

    /// The edit view's settings; a value of the wrong type is its default, and a custom ratio
    /// that is not a `{name, width, height}` in bounds is left out.
    pub fn edit(&self) -> EditSettings {
        self.read_edit(&mut Vec::new())
    }

    /// Stores the edit view's settings under `"edit"` (ratios and the last ratio kept by the
    /// reading rules; `photo_sizes: None` removes the stored choice); other keys are kept, and so
    /// are the unknown keys of each stored ratio whose name is kept.
    pub fn set_edit(&mut self, e: &EditSettings) {
        let stored: Vec<Map<String, Value>> = match self.field("edit", "cropRatios") {
            Some(Value::Array(list)) => list.iter().filter_map(|v| v.as_object().cloned()).collect(),
            _ => Vec::new(),
        };
        let ratios: Vec<Value> = ratio_list(&e.crop_ratios)
            .into_iter()
            .map(|r| {
                let mut obj = stored
                    .iter()
                    .find(|o| o.get("name").and_then(Value::as_str).map(str::trim) == Some(r.name.as_str()))
                    .cloned()
                    .unwrap_or_default();
                obj.insert("name".into(), json!(r.name));
                obj.insert("width".into(), json!(r.width));
                obj.insert("height".into(), json!(r.height));
                Value::Object(obj)
            })
            .collect();
        let last = e.last_crop_ratio.as_deref().map(str::trim).filter(|l| !l.is_empty() && l.len() <= MAX_RATIO_NAME).map(|l| json!(l));
        self.merge(
            "edit",
            &[
                ("cropRatios", Some(Value::Array(ratios))),
                ("lastCropRatio", last),
                ("photoSizes", e.photo_sizes.map(|p| json!(p.id()))),
                ("singleClickLeaves", Some(json!(e.single_click_leaves))),
            ],
        );
    }

    /// Save's settings; a value of the wrong type is its default.
    pub fn save(&self) -> SaveSettings {
        self.read_save(&mut Vec::new())
    }

    /// Stores Save's settings under `"save"` (a last folder that is not an absolute path of valid
    /// Unicode, or `None`, removes the stored one); other keys are kept.
    pub fn set_save(&mut self, s: &SaveSettings) {
        let folder = s.last_folder.as_deref().and_then(folder_path).map(|f| json!(f));
        self.merge("save", &[("explain", Some(json!(s.explain))), ("lastFolder", folder), ("lastFormat", Some(json!(s.last_format.id())))]);
    }

    /// The export's settings; a value of the wrong type or out of its range is its default.
    pub fn export(&self) -> ExportSettings {
        self.read_export(&mut Vec::new())
    }

    /// Stores the export's settings under `"export"` (numbers out of range as their defaults, the
    /// watermark text cut at [`MAX_WATERMARK_BYTES`] on a character boundary, a location that is not
    /// an absolute path of valid Unicode, or `None`, removed); other keys are kept.
    pub fn set_export(&mut self, e: &ExportSettings) {
        let size = if EXPORT_SIZE_RANGE.contains(&e.size) { e.size } else { EXPORT_SIZE_DEFAULT };
        let custom = if QUALITY_RANGE.contains(&e.custom_quality) { e.custom_quality } else { CUSTOM_QUALITY_DEFAULT };
        self.merge(
            "export",
            &[
                ("location", e.location.as_deref().and_then(folder_path).map(|l| json!(l))),
                ("resize", Some(json!(e.resize))),
                ("size", Some(json!(size))),
                ("quality", Some(json!(e.quality.id()))),
                ("customQuality", Some(json!(custom))),
                ("numbers", Some(json!(e.numbers))),
                ("watermark", Some(json!(e.watermark))),
                ("watermarkText", Some(json!(cut_text(&e.watermark_text, MAX_WATERMARK_BYTES)))),
                ("removeLocation", Some(json!(e.remove_location))),
            ],
        );
    }

    /// Organizing's settings; a value of the wrong type or out of its range is its default.
    pub fn organize(&self) -> OrganizeSettings {
        self.read_organize(&mut Vec::new())
    }

    /// Stores organizing's settings under `"organize"` (days out of range as the default, a last
    /// folder that is not an absolute path of valid Unicode, or `None`, removed); other keys are
    /// kept.
    pub fn set_organize(&mut self, o: &OrganizeSettings) {
        let days = if RECENT_DAYS_RANGE.contains(&o.recent_days) { o.recent_days } else { RECENT_DAYS_DEFAULT };
        self.merge(
            "organize",
            &[
                ("askBeforeMoving", Some(json!(o.ask_before_moving))),
                ("removeFromAlbumsWithoutAsking", Some(json!(o.remove_from_albums_without_asking))),
                ("deleteWithoutAsking", Some(json!(o.delete_without_asking))),
                ("recentDays", Some(json!(days))),
                ("lastMoveFolder", o.last_move_folder.as_deref().and_then(folder_path).map(|f| json!(f))),
            ],
        );
    }

    fn read_organize(&self, problems: &mut Vec<String>) -> OrganizeSettings {
        let mut o = OrganizeSettings::default();
        let Some(obj) = self.object("organize", problems) else { return o };
        o.ask_before_moving = Settings::flag(obj, "organize", "askBeforeMoving", true, problems);
        o.remove_from_albums_without_asking = Settings::flag(obj, "organize", "removeFromAlbumsWithoutAsking", false, problems);
        o.delete_without_asking = Settings::flag(obj, "organize", "deleteWithoutAsking", false, problems);
        match obj.get("recentDays") {
            None => {}
            Some(v) => match v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| RECENT_DAYS_RANGE.contains(n)) {
                Some(n) => o.recent_days = n,
                None => problems.push(format!(
                    "organize.recentDays is not a whole number of days from {} to {}; the default ({RECENT_DAYS_DEFAULT}) is used",
                    RECENT_DAYS_RANGE.start(),
                    RECENT_DAYS_RANGE.end()
                )),
            },
        }
        match obj.get("lastMoveFolder") {
            None | Some(Value::Null) => {}
            Some(Value::String(f)) if folder_path(Path::new(f)).is_some() => o.last_move_folder = Some(PathBuf::from(f)),
            Some(_) => problems.push("organize.lastMoveFolder is not an absolute folder path; no folder is remembered".into()),
        }
        o
    }

    /// The stored settings this version reads with a value of the wrong type, one line each, by key
    /// (never by value): their defaults are used. The session reports them in its notices when it
    /// opens the library.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.read_metadata_writes(&mut out);
        self.read_quick_tags(&mut out);
        self.read_recent_tags(&mut out);
        self.read_edit(&mut out);
        self.read_save(&mut out);
        self.read_export(&mut out);
        self.read_organize(&mut out);
        out.into_iter().map(|p| format!("settings.json: {p}")).collect()
    }

    /// A boolean field of `obj`: `None` when absent; a value that is not a boolean is reported and
    /// gives `default`.
    fn flag(obj: &Map<String, Value>, object: &str, key: &str, default: bool, problems: &mut Vec<String>) -> bool {
        match obj.get(key) {
            None => default,
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                let d = if default { "on" } else { "off" };
                problems.push(format!("{object}.{key} is not true or false; the default ({d}) is used"));
                default
            }
        }
    }

    fn read_save(&self, problems: &mut Vec<String>) -> SaveSettings {
        let mut s = SaveSettings::default();
        let Some(obj) = self.object("save", problems) else { return s };
        s.explain = Settings::flag(obj, "save", "explain", true, problems);
        match obj.get("lastFolder") {
            None | Some(Value::Null) => {}
            Some(Value::String(f)) if folder_path(Path::new(f)).is_some() => s.last_folder = Some(PathBuf::from(f)),
            Some(_) => problems.push("save.lastFolder is not an absolute folder path; no folder is remembered".into()),
        }
        match obj.get("lastFormat") {
            None => {}
            Some(v) => match v.as_str().and_then(SaveFormat::from_id) {
                Some(f) => s.last_format = f,
                None => problems.push("save.lastFormat is not \"jpeg\" or \"png\"; the default (jpeg) is used".into()),
            },
        }
        s
    }

    fn read_export(&self, problems: &mut Vec<String>) -> ExportSettings {
        let mut e = ExportSettings::default();
        let Some(obj) = self.object("export", problems) else { return e };
        match obj.get("location") {
            None | Some(Value::Null) => {}
            Some(Value::String(l)) if folder_path(Path::new(l)).is_some() => e.location = Some(PathBuf::from(l)),
            Some(_) => problems.push("export.location is not an absolute folder path; the default location is used".into()),
        }
        e.resize = Settings::flag(obj, "export", "resize", true, problems);
        match obj.get("size") {
            None => {}
            Some(v) => match v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| EXPORT_SIZE_RANGE.contains(n)) {
                Some(n) => e.size = n,
                None => problems.push(format!(
                    "export.size is not a whole number of pixels from {} to {}; the default ({EXPORT_SIZE_DEFAULT}) is used",
                    EXPORT_SIZE_RANGE.start(),
                    EXPORT_SIZE_RANGE.end()
                )),
            },
        }
        match obj.get("quality") {
            None => {}
            Some(v) => match v.as_str().and_then(ExportQuality::from_id) {
                Some(q) => e.quality = q,
                None => problems.push("export.quality is not a known quality; the default (automatic) is used".into()),
            },
        }
        match obj.get("customQuality") {
            None => {}
            Some(v) => match v.as_u64().and_then(|n| u8::try_from(n).ok()).filter(|n| QUALITY_RANGE.contains(n)) {
                Some(n) => e.custom_quality = n,
                None => {
                    problems.push(format!("export.customQuality is not a whole number from 1 to 100; the default ({CUSTOM_QUALITY_DEFAULT}) is used"))
                }
            },
        }
        e.numbers = Settings::flag(obj, "export", "numbers", false, problems);
        e.watermark = Settings::flag(obj, "export", "watermark", false, problems);
        match obj.get("watermarkText") {
            None => {}
            Some(Value::String(t)) if t.len() <= MAX_WATERMARK_BYTES => e.watermark_text = t.clone(),
            Some(_) => problems.push(format!("export.watermarkText is not text of at most {MAX_WATERMARK_BYTES} bytes; no text is used")),
        }
        e.remove_location = Settings::flag(obj, "export", "removeLocation", false, problems);
        e
    }

    /// The object under `key`, `None` when absent; a value there that is not an object is reported
    /// (once) and counts as absent.
    fn object(&self, key: &str, problems: &mut Vec<String>) -> Option<&Map<String, Value>> {
        match self.root.get(key)? {
            Value::Object(o) => Some(o),
            _ => {
                let p = format!("\"{key}\" is not an object; its defaults are used");
                if !problems.contains(&p) {
                    problems.push(p);
                }
                None
            }
        }
    }

    fn read_metadata_writes(&self, problems: &mut Vec<String>) -> MetadataWrites {
        let defaults = MetadataWrites::default().keyed();
        let obj = self.object("files", problems);
        let values = defaults.map(|(key, default)| match obj.and_then(|o| o.get(key)) {
            None => default,
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                let d = if default { "on" } else { "off" };
                problems.push(format!("files.{key} is not true or false; the default ({d}) is used"));
                default
            }
        });
        MetadataWrites::from_keyed(values)
    }

    fn read_quick_tags(&self, problems: &mut Vec<String>) -> QuickTags {
        let mut q = QuickTags::default();
        let Some(obj) = self.object("tags", problems) else { return q };
        match obj.get("useRecent") {
            None => {}
            Some(Value::Bool(b)) => q.use_recent = *b,
            Some(_) => problems.push("tags.useRecent is not true or false; the default (on) is used".into()),
        }
        match obj.get("quick") {
            None => {}
            Some(Value::Array(list)) => {
                let mut bad = 0usize;
                for (slot, v) in q.slots.iter_mut().zip(list) {
                    *slot = match v {
                        Value::Null => None,
                        Value::String(t) => tag_text(t),
                        _ => None,
                    };
                    let unusable = match v {
                        Value::Null => false,
                        Value::String(t) => t.trim().len() > MAX_TAG_BYTES,
                        _ => true,
                    };
                    bad += usize::from(unusable);
                }
                if bad > 0 {
                    problems.push(match bad {
                        1 => "tags.quick: 1 slot holds no usable tag text; it is unassigned".to_string(),
                        n => format!("tags.quick: {n} slots hold no usable tag text; they are unassigned"),
                    });
                }
                if list.len() > QUICK_TAGS {
                    problems.push(format!("tags.quick has more than {QUICK_TAGS} slots; the rest are ignored"));
                }
            }
            Some(_) => problems.push("tags.quick is not a list; every quick tag is unassigned".into()),
        }
        q
    }

    fn read_recent_tags(&self, problems: &mut Vec<String>) -> Vec<String> {
        let Some(obj) = self.object("tags", problems) else { return Vec::new() };
        match obj.get("recent") {
            None => Vec::new(),
            Some(Value::Array(list)) => {
                let bad = list.iter().filter(|v| v.as_str().is_none_or(|t| t.trim().len() > MAX_TAG_BYTES)).count();
                if bad > 0 {
                    problems.push(match bad {
                        1 => "tags.recent: 1 entry holds no usable tag text; it is left out".to_string(),
                        n => format!("tags.recent: {n} entries hold no usable tag text; they are left out"),
                    });
                }
                recent_list(list.iter().filter_map(Value::as_str))
            }
            Some(_) => {
                problems.push("tags.recent is not a list; no recent tags are used".into());
                Vec::new()
            }
        }
    }

    fn read_edit(&self, problems: &mut Vec<String>) -> EditSettings {
        let mut e = EditSettings::default();
        let Some(obj) = self.object("edit", problems) else { return e };
        match obj.get("singleClickLeaves") {
            None => {}
            Some(Value::Bool(b)) => e.single_click_leaves = *b,
            Some(_) => problems.push("edit.singleClickLeaves is not true or false; the default (off) is used".into()),
        }
        match obj.get("photoSizes") {
            None => {}
            Some(v) => match v.as_str().and_then(PhotoSizes::from_id) {
                Some(p) => e.photo_sizes = Some(p),
                None => problems.push("edit.photoSizes is not \"us\" or \"metric\"; the system's sizes are used".into()),
            },
        }
        match obj.get("lastCropRatio") {
            None => {}
            Some(Value::String(l)) if !l.trim().is_empty() && l.trim().len() <= MAX_RATIO_NAME => e.last_crop_ratio = Some(l.trim().to_string()),
            Some(_) => problems.push("edit.lastCropRatio is not a ratio name; no ratio is remembered".into()),
        }
        match obj.get("cropRatios") {
            None => {}
            Some(Value::Array(list)) => {
                let read = |v: &Value| -> Option<CropRatio> {
                    let o = v.as_object()?;
                    CropRatio { name: o.get("name")?.as_str()?.to_string(), width: o.get("width")?.as_f64()?, height: o.get("height")?.as_f64()? }
                        .checked()
                };
                let parsed: Vec<Option<CropRatio>> = list.iter().map(read).collect();
                let bad = parsed.iter().filter(|r| r.is_none()).count();
                if bad > 0 {
                    problems.push(match bad {
                        1 => "edit.cropRatios: 1 entry is not a usable ratio; it is left out".to_string(),
                        n => format!("edit.cropRatios: {n} entries are not usable ratios; they are left out"),
                    });
                }
                let kept: Vec<CropRatio> = parsed.into_iter().flatten().collect();
                if kept.len() > MAX_CROP_RATIOS {
                    problems.push(format!("edit.cropRatios has more than {MAX_CROP_RATIOS} ratios; the rest are ignored"));
                }
                e.crop_ratios = ratio_list(&kept);
            }
            Some(_) => problems.push("edit.cropRatios is not a list; no custom ratios are used".into()),
        }
        e
    }

    /// Reads `<dir>/settings.json` without changing anything: unlike [`Settings::load`], a damaged
    /// or unreadable file gives the defaults and stays where it is (the session that opens the
    /// library sets it aside and reports it). For what the window needs before the library opens.
    pub fn peek(dir: &Path) -> Settings {
        match dalica_fs::read(&dir.join(FILE), MAX_BYTES) {
            Ok(Some(r)) => Settings::parse(&r.bytes).unwrap_or_default(),
            _ => Settings::default(),
        }
    }
}

/// `text` cut to at most `max` bytes on a character boundary.
fn cut_text(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

impl crate::Session {
    /// Where exports go: the stored location, else `<Pictures>/Dalica/Exports` under the Pictures
    /// folder known when the library opened ([`crate::Session::set_pictures_dir`]); `None` when
    /// neither is known.
    pub fn export_location(&self) -> Option<PathBuf> {
        self.settings().export().location.or_else(|| self.pictures_dir.as_deref().map(default_export_location))
    }

    /// The projects directory: always `<Pictures>/Dalica` under the Pictures folder known when the
    /// library opened, whatever export location is chosen (`creating.md` §2: a location such as
    /// `~/Desktop/Out` must not make `~/Desktop` a projects folder); `None` when no Pictures folder
    /// is known. The library tree lists the folders under it in Projects
    /// (`TreeOptions::projects_dir`).
    pub fn projects_dir(&self) -> Option<PathBuf> {
        self.pictures_dir.as_deref().map(projects_dir_under)
    }
}

/// A stored thumbnail size worth reading: a finite number in `0 < s ≤ THUMB_SIZE_LIMIT`.
fn thumb_size_ok(s: f32) -> bool {
    s.is_finite() && s > 0.0 && s <= THUMB_SIZE_LIMIT
}

/// `(command, context, key)` of one entry; `None` if it cannot be read.
fn read_entry(e: &Value) -> Option<(String, Context, Option<Key>)> {
    let command = e.get("command")?.as_str()?.to_string();
    let context: Context = serde_json::from_value(e.get("context")?.clone()).ok()?;
    let key = match e.get("key")? {
        Value::Null => None,
        Value::String(k) => Some(Key::parse(k).ok()?),
        _ => return None,
    };
    Some((command, context, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_and_view_state_survive_a_write_and_a_read() {
        let mut s = Settings::parse(br#"{"keymap": [], "window": {"monitor": "DP-1"}, "future": {"a": 1}}"#).unwrap();
        let w = WindowState { position: Some([-1200.0, 40.0]), size: [1600.0, 1000.0], maximized: true };
        let v = ViewState { tree_width: Some(310.0), pane: Some("tags".into()), pane_large: true };
        s.set_window(&w);
        s.set_view(&v);
        s.set_system_title_bar(true);
        let back = Settings::parse(&s.print()).unwrap();
        assert_eq!(back.window(), Some(w));
        assert_eq!(back.view(), v);
        assert!(back.system_title_bar());
        // Merge, never replace: keys this version does not write are kept.
        assert_eq!(back.get("window").unwrap()["monitor"], "DP-1");
        assert_eq!(back.get("future").unwrap(), &json!({ "a": 1 }));
        assert_eq!(back.get("keymap").unwrap(), &json!([]));
        // A closed pane is stored as null; a position the platform did not tell keeps the old one.
        let mut s = back;
        s.set_view(&ViewState { tree_width: None, pane: None, pane_large: false });
        assert_eq!(s.get("view").unwrap()["pane"], Value::Null);
        assert_eq!(s.view(), ViewState::default());
        s.set_window(&WindowState { position: None, size: [1000.0, 700.0], maximized: false });
        assert_eq!(s.window().unwrap().position, Some([-1200.0, 40.0]));
    }

    #[test]
    fn file_types_survive_a_write_and_a_read_and_hostile_values_count_as_on() {
        let s = Settings::parse(br#"{"fileTypes": {"png": false, "gif": "no", "raw": false, "jpeg": false, "future": 1}}"#).unwrap();
        let t = s.file_types();
        assert!(!t.allows(FileType::Png) && !t.allows(FileType::Raw));
        assert!(t.allows(FileType::Gif) && t.allows(FileType::Jpeg) && t.allows(FileType::Tiff));
        let mut s = s;
        s.set_file_types(FileTypes::all().with(FileType::Video, false));
        let back = Settings::parse(&s.print()).unwrap();
        assert_eq!(back.file_types(), FileTypes::all().with(FileType::Video, false));
        assert_eq!(back.get("fileTypes").unwrap()["future"], 1, "keys it does not write are kept");
        assert_eq!(Settings::default().file_types(), FileTypes::all());
    }

    #[test]
    fn view_options_survive_a_write_and_a_read_beside_the_view_state() {
        let mut s = Settings::parse(br#"{"view": {"treeWidth": 300, "future": [1]}}"#).unwrap();
        let o = ViewOptions {
            thumb_size: Some(200.0),
            tree_mode: Some("tree".into()),
            folder_sort: Some("recent".into()),
            folder_sort_reverse: true,
            caption: Some("filename".into()),
            small_pictures: true,
            hidden_pictures: true,
        };
        s.set_view_options(&o);
        let back = Settings::parse(&s.print()).unwrap();
        assert_eq!(back.view_options(), o);
        // Merged into "view": the view state's keys and unknown keys are kept, and the view state
        // writes leave the options alone.
        assert_eq!(back.view().tree_width, Some(300.0));
        assert_eq!(back.get("view").unwrap()["future"], json!([1]));
        let mut s = back;
        s.set_view(&ViewState { tree_width: Some(250.0), pane: None, pane_large: false });
        assert_eq!(s.view_options(), o);
        // Back to the defaults: the names are removed, the flags stored as false.
        s.set_view_options(&ViewOptions::default());
        assert_eq!(s.view_options(), ViewOptions::default());
        assert!(s.get("view").unwrap().get("treeMode").is_none());
        assert_eq!(s.view().tree_width, Some(250.0));
    }

    #[test]
    fn hostile_view_options_are_dropped() {
        let s = Settings::parse(
            br#"{"view": {"thumbSize": 1e9, "treeMode": 3, "folderSort": "", "folderSortReverse": "yes",
                          "caption": ["date"], "smallPictures": 1, "hiddenPictures": null}}"#,
        )
        .unwrap();
        assert_eq!(s.view_options(), ViewOptions::default());
        for bad in ["-1", "0", "null", r#""144""#, "1e40"] {
            let s = Settings::parse(format!(r#"{{"view": {{"thumbSize": {bad}}}}}"#).as_bytes()).unwrap();
            assert_eq!(s.view_options().thumb_size, None, "{bad}");
        }
        let long = format!(r#"{{"view": {{"caption": "{}"}}}}"#, "c".repeat(100));
        assert_eq!(Settings::parse(long.as_bytes()).unwrap().view_options().caption, None);
        // Values out of range are never written either.
        let mut s = Settings::default();
        s.set_view_options(&ViewOptions { thumb_size: Some(f32::NAN), caption: Some(String::new()), ..ViewOptions::default() });
        assert!(s.get("view").unwrap().get("thumbSize").is_none() && s.get("view").unwrap().get("caption").is_none());
    }

    #[test]
    fn nothing_stored_means_the_defaults() {
        let s = Settings::default();
        assert_eq!(s.window(), None);
        assert_eq!(s.view(), ViewState::default());
        assert_eq!(s.view_options(), ViewOptions::default());
        assert!(!s.system_title_bar());
    }

    #[test]
    fn hostile_stored_values_are_clamped_or_dropped() {
        let s = Settings::parse(
            br#"{"window": {"x": 1e30, "y": 5, "width": -3, "height": 1e9, "maximized": "yes"},
                 "view": {"treeWidth": 1e9, "pane": 7, "paneLarge": 1},
                 "interface": {"systemTitleBar": "on"}}"#,
        )
        .unwrap();
        let w = s.window().unwrap();
        assert_eq!(w, WindowState { position: None, size: [WINDOW_MIN[0], WINDOW_MAX_SIDE], maximized: false });
        assert_eq!(s.view(), ViewState::default());
        assert!(!s.system_title_bar());
        for bad in [r#"{"view": {"treeWidth": -5}}"#, r#"{"view": {"treeWidth": 0}}"#, r#"{"view": "wide"}"#, r#"{"window": [1, 2]}"#] {
            let s = Settings::parse(bad.as_bytes()).unwrap();
            assert_eq!(s.view().tree_width, None, "{bad}");
            assert!(s.window().is_none_or(|w| w == WindowState::default()), "{bad}");
        }
        let long = format!(r#"{{"view": {{"pane": "{}"}}}}"#, "p".repeat(100));
        assert_eq!(Settings::parse(long.as_bytes()).unwrap().view().pane, None);
        // Values that are not numbers in range are never written either.
        let mut s = Settings::default();
        s.set_window(&WindowState { position: Some([f32::NAN, 0.0]), size: [f32::INFINITY, 10.0], maximized: false });
        assert_eq!(s.window().unwrap(), WindowState { position: None, size: [WINDOW_DEFAULT[0], WINDOW_MIN[1]], maximized: false });
        s.set_view(&ViewState { tree_width: Some(f32::NAN), pane: None, pane_large: false });
        assert!(s.get("view").unwrap().get("treeWidth").is_none());
        // A value that is not an object is replaced by one.
        let mut s = Settings::parse(br#"{"view": 3}"#).unwrap();
        s.set_view(&ViewState { tree_width: Some(250.0), ..ViewState::default() });
        assert_eq!(s.view().tree_width, Some(250.0));
    }

    #[test]
    fn the_thumb_speed_is_read_from_the_file_and_hostile_values_are_the_defaults() {
        assert_eq!(Settings::default().thumb_speed(), ThumbSpeed { exponent: 2.0, top_speed: 40.0 });
        let s = Settings::parse(br#"{"scrolling": {"thumbExponent": 1.5, "thumbTopSpeed": 25, "future": true}}"#).unwrap();
        assert_eq!(s.thumb_speed(), ThumbSpeed { exponent: 1.5, top_speed: 25.0 });
        for bad in ["0", "-2", "1e9", "null", r#""2""#, "[2]"] {
            let s = Settings::parse(format!(r#"{{"scrolling": {{"thumbExponent": {bad}, "thumbTopSpeed": {bad}}}}}"#).as_bytes()).unwrap();
            assert_eq!(s.thumb_speed(), ThumbSpeed::default(), "{bad}");
        }
        assert_eq!(Settings::parse(br#"{"scrolling": 3}"#).unwrap().thumb_speed(), ThumbSpeed::default());
        // Written merged, out-of-range values as the defaults; unknown keys kept.
        let mut s = s;
        s.set_thumb_speed(&ThumbSpeed { exponent: 3.0, top_speed: f32::NAN });
        let back = Settings::parse(&s.print()).unwrap();
        assert_eq!(back.thumb_speed(), ThumbSpeed { exponent: 3.0, top_speed: 40.0 });
        assert_eq!(back.get("scrolling").unwrap()["future"], json!(true));
    }

    #[test]
    fn colour_management_and_the_cache_size_survive_a_write_and_hostile_values_are_the_defaults() {
        use dalica_thumbs::pack::DEFAULT_BUDGET;
        use dalica_thumbs::upkeep::{MAX_BUDGET, MIN_BUDGET};
        let s = Settings::default();
        assert!(s.color_management());
        assert_eq!(s.thumbnail_cache_bytes(), DEFAULT_BUDGET);
        let mut s = Settings::parse(br#"{"view": {"thumbSize": 120}, "cache": {"other": 1}}"#).unwrap();
        s.set_color_management(false);
        s.set_thumbnail_cache_bytes(512 << 20);
        let back = Settings::parse(&s.print()).unwrap();
        assert!(!back.color_management());
        assert_eq!(back.thumbnail_cache_bytes(), 512 << 20);
        assert_eq!(back.view_options().thumb_size, Some(120.0), "other view keys kept");
        assert_eq!(back.get("cache").unwrap()["other"], json!(1), "other cache keys kept");
        s.set_thumbnail_cache_bytes(1);
        assert_eq!(s.thumbnail_cache_bytes(), MIN_BUDGET, "stored clamped");
        s.set_thumbnail_cache_bytes(u64::MAX);
        assert_eq!(s.thumbnail_cache_bytes(), MAX_BUDGET);
        for hostile in [r#""4 GB""#, "-1", "1.5e9", "1", "1e300", "null", "true"] {
            let s = Settings::parse(format!(r#"{{"cache": {{"thumbnailBytes": {hostile}}}, "view": {{"colorManagement": {hostile}}}}}"#).as_bytes())
                .unwrap();
            assert_eq!(s.thumbnail_cache_bytes(), DEFAULT_BUDGET, "{hostile}");
            assert_eq!(s.color_management(), hostile != "false", "{hostile}");
        }
    }

    #[test]
    fn peek_reads_without_setting_a_damaged_file_aside() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::peek(dir.path()), Settings::default());
        dalica_fs::replace(&dir.path().join(FILE), b"{ not json", dalica_fs::Expect::Absent).unwrap();
        assert_eq!(Settings::peek(dir.path()), Settings::default());
        assert!(dir.path().join(FILE).is_file(), "left where it is");
        let good = tempfile::tempdir().unwrap();
        dalica_fs::replace(&good.path().join(FILE), br#"{"interface": {"systemTitleBar": true}}"#, dalica_fs::Expect::Absent).unwrap();
        assert!(Settings::peek(good.path()).system_title_bar());
    }
}
