//! The options catalogue (`menus-and-shortcuts.md` §5, `roadmap.md` M10, M11 and M13): one row per
//! option the Options dialog shows on its General, File Types, Slideshow, Network, People, Photo
//! Viewer and Export tabs, read by the dialog,
//! `options.list` / `options.set` (`cmd/options.rs`) and MCP alike, so the three never disagree.
//!
//! A row names its setting by `"<object>.<field>"` in `settings.json` (`interface.animations`
//! is `{"interface": {"animations": …}}`), and reads the value in effect through the setting's typed
//! accessor in [`crate::settings`], so a row's default is its accessor's on an empty file. Options
//! that existed before M10 (`fileTypes`, the `files` group, M7's `edit.singleClickLeaves`, M8's
//! export and M9's ask-first settings) are rows here, never second copies of their settings.
//!
//! An option whose feature does not exist yet (the slideshow's music and video sound until Dalica
//! plays sound, the photo viewer's file types until the OS integration, the network log until it
//! exists) is listed with the reason the dialog shows ([`OptionSpec::unavailable`]); its setting is stored only
//! once its feature reads it, so `options.set` refuses it until then and its value is its default.
//! The map rows and the proxy of the Network tab waited on reserved decisions 4 and 9 until M13.20
//! (their reasons, [`crate::map_tiles::TILES_UNAVAILABLE`] and
//! [`crate::map_tiles::PROXY_UNAVAILABLE`], are now `None`: OpenStreetMap's tiles and Nominatim,
//! off until allowed, both addresses editable). The People tab's face model (its status, Download…, Remove
//! and Use) is the face model dialog's, through `faces.models.*`, not a row here; the model in use
//! is `people.model` ([`crate::settings::PeopleSettings::model`]). "Read photo data left by the
//! previous organizer" can be changed from M14.5 (`files.readLegacyData`, read by
//! [`Settings::legacy`]); a change rescans the watched folders.
//!
//! M15.4: Language can be changed and applies at once; its choices are the languages the app gave
//! the session ([`Settings::languages`], [`OptionSpec::given`]), System first. General ▸ Files'
//! "External editor" (`files.externalEditor`, [`Settings::external_editor`]) joins the catalogue
//! with the window's text field (M15.22). The Photo
//! Viewer's File Types row (`viewer.defaultViewer`) is a list of file types ([`OptionSpec::many`]:
//! any of its choices, each once), still waiting on the OS integration (M15.24).
//!
//! [`OptionSpec::check`] is the one validation of a value, [`OptionSpec::store`] the one way a
//! value is written (a merge into its object, unknown keys kept) and [`OptionSpec::to_json`] the
//! row as `options.list` gives it.

use dalica_scan::FileType;
use serde_json::{Map, Value, json};

use crate::keymap::Platform;
use crate::map_tiles::{DEFAULT_ATTRIBUTION, DEFAULT_GEOCODER, DEFAULT_TILE_SERVER, PROXY_UNAVAILABLE, TILES_UNAVAILABLE};
use crate::settings::{
    CONFIDENCE_DEFAULT, CONFIDENCE_RANGE, CONFIDENCE_STEP, CUSTOM_QUALITY_DEFAULT, DISPLAY_TIME_DEFAULT, DISPLAY_TIME_RANGE, EXPORT_SIZE_DEFAULT,
    EXPORT_SIZE_RANGE, MAX_ATTRIBUTION_BYTES, MAX_HOST_BYTES, MAX_TEMPLATE_BYTES, MAX_WATERMARK_BYTES, PROXY_PORT_DEFAULT, PROXY_PORT_RANGE,
    QUALITY_RANGE, SMALL_PICTURE_DEFAULT, SMALL_PICTURE_RANGE, Settings, UI_SCALE_RANGE, UI_SCALE_STEP, UiScaleSetting,
};

/// A tab of the Options dialog that has options in the catalogue, in the dialog's order (the
/// Keyboard Shortcuts tab is the shortcut editor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tab {
    General,
    FileTypes,
    Slideshow,
    Network,
    People,
    PhotoViewer,
    Export,
}

impl Tab {
    pub const ALL: [Tab; 7] = [Tab::General, Tab::FileTypes, Tab::Slideshow, Tab::Network, Tab::People, Tab::PhotoViewer, Tab::Export];

    /// The id `options.list {tab}` takes.
    pub fn id(self) -> &'static str {
        match self {
            Tab::General => "general",
            Tab::FileTypes => "fileTypes",
            Tab::Slideshow => "slideshow",
            Tab::Network => "network",
            Tab::People => "people",
            Tab::PhotoViewer => "viewer",
            Tab::Export => "export",
        }
    }

    pub fn from_id(id: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.id() == id)
    }

    /// The tab's English title.
    pub fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::FileTypes => "File Types",
            Tab::Slideshow => "Slideshow",
            Tab::Network => "Network",
            Tab::People => "People",
            Tab::PhotoViewer => "Photo Viewer",
            Tab::Export => "Export",
        }
    }
}

/// When a changed value takes effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applies {
    /// At once.
    Now,
    /// After a rescan of the watched folders, which the change starts.
    Rescan,
    /// At the next launch ("Applies at next launch.").
    Restart,
}

impl Applies {
    pub fn id(self) -> &'static str {
        match self {
            Applies::Now => "now",
            Applies::Rescan => "rescan",
            Applies::Restart => "restart",
        }
    }
}

/// An option's type and the values it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `true` or `false`.
    Bool,
    /// A whole number in `min ..= max`, on a step from `min`; `unit` as shown ("px", "%", "").
    Int { min: u32, max: u32, step: u32, unit: &'static str },
    /// `"auto"` or a percentage in `min ..= max` on a step from `min`.
    Scale { min: u16, max: u16, step: u16 },
    /// One of the choices, each `(stored id, English label)`.
    Choice(&'static [(&'static str, &'static str)]),
    /// Text of at most `max_bytes` bytes.
    Text { max_bytes: usize },
    /// An absolute folder path, or `null` for the default folder.
    Folder,
}

impl Kind {
    /// The type's name in `options.list`.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::Int { .. } => "int",
            Kind::Scale { .. } => "scale",
            Kind::Choice(_) => "choice",
            Kind::Text { .. } => "text",
            Kind::Folder => "folder",
        }
    }
}

/// An option's default, as stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Initial {
    Bool(bool),
    Int(u32),
    Str(&'static str),
    /// A list of choice ids (a [`OptionSpec::many`] row's).
    List(&'static [&'static str]),
    /// No value stored: the default folder (the Pictures folder, `<Pictures>/Dalica/Exports`).
    Null,
}

impl Initial {
    pub fn to_json(self) -> Value {
        match self {
            Initial::Bool(b) => json!(b),
            Initial::Int(n) => json!(n),
            Initial::Str(s) => json!(s),
            Initial::List(l) => json!(l),
            Initial::Null => Value::Null,
        }
    }
}

/// Choices a session gives, `(id, label)`, read from its settings.
pub type Choices = fn(&Settings) -> Vec<(String, String)>;

/// One option of the Options dialog.
#[derive(Clone, Copy)]
pub struct OptionSpec {
    /// The setting, `"<object>.<field>"` in `settings.json`; unique in the catalogue.
    pub key: &'static str,
    pub tab: Tab,
    /// The group it is listed under ("Interface", "Files", …).
    pub group: &'static str,
    /// The English label (also the i18n key).
    pub label: &'static str,
    /// A line shown under it, `""` for none.
    pub hint: &'static str,
    pub kind: Kind,
    pub default: Initial,
    pub applies: Applies,
    /// The platforms it is shown on.
    pub platforms: &'static [Platform],
    /// Why it cannot be changed yet (its feature does not exist in this build), `None` when it can.
    pub unavailable: Option<&'static str>,
    /// The value in effect, through the setting's typed accessor.
    pub read: fn(&Settings) -> Value,
    /// A [`Kind::Choice`] whose value is a list of its choices, each once, in the choices' order
    /// (`options.list` gives its type as `"list"`), rather than one of them.
    pub many: bool,
    /// The choices the session gives in place of the kind's, `(id, label)` (Language's: the
    /// languages the app offers, [`Settings::languages`]).
    pub given: Option<Choices>,
}

impl std::fmt::Debug for OptionSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OptionSpec").field("key", &self.key).finish_non_exhaustive()
    }
}

/// A row available on every platform, applying at once, with no hint.
const fn row(
    key: &'static str,
    tab: Tab,
    group: &'static str,
    label: &'static str,
    kind: Kind,
    default: Initial,
    read: fn(&Settings) -> Value,
) -> OptionSpec {
    OptionSpec {
        key,
        tab,
        group,
        label,
        hint: "",
        kind,
        default,
        applies: Applies::Now,
        platforms: &Platform::ALL,
        unavailable: None,
        read,
        many: false,
        given: None,
    }
}

impl OptionSpec {
    const fn hint(self, hint: &'static str) -> OptionSpec {
        OptionSpec { hint, ..self }
    }

    const fn applies(self, applies: Applies) -> OptionSpec {
        OptionSpec { applies, ..self }
    }

    const fn only(self, platforms: &'static [Platform]) -> OptionSpec {
        OptionSpec { platforms, ..self }
    }

    const fn later(self, reason: &'static str) -> OptionSpec {
        OptionSpec { unavailable: Some(reason), ..self }
    }

    const fn unavailable_if(self, reason: Option<&'static str>) -> OptionSpec {
        OptionSpec { unavailable: reason, ..self }
    }

    const fn many(self) -> OptionSpec {
        OptionSpec { many: true, ..self }
    }

    const fn given(self, given: Choices) -> OptionSpec {
        OptionSpec { given: Some(given), ..self }
    }

    /// A choice row's choices with `settings`, `(id, label)`: those the session gives, else the
    /// kind's; none for other kinds.
    pub fn choices(&self, settings: &Settings) -> Vec<(String, String)> {
        match (self.given, self.kind) {
            (Some(given), Kind::Choice(_)) => given(settings),
            (None, Kind::Choice(choices)) => choices.iter().map(|(id, label)| ((*id).to_string(), (*label).to_string())).collect(),
            _ => Vec::new(),
        }
    }

    /// The object of `settings.json` it lives in, and its field there.
    pub fn place(&self) -> (&'static str, &'static str) {
        self.key.split_once('.').unwrap_or((self.key, ""))
    }

    pub fn default_value(&self) -> Value {
        self.default.to_json()
    }

    /// Whether it can be changed in this build.
    pub fn available(&self) -> bool {
        self.unavailable.is_none()
    }

    /// Whether it is shown on `platform`.
    pub fn on(&self, platform: Platform) -> bool {
        self.platforms.contains(&platform)
    }

    /// `value` as it is stored, or why it cannot be (naming the option, never echoing the value),
    /// with the choices an empty settings file gives ([`OptionSpec::check_with`]).
    pub fn check(&self, value: &Value) -> Result<Value, String> {
        self.check_with(value, &Settings::default())
    }

    /// [`OptionSpec::check`] with the choices `settings` gives (the languages offered).
    pub fn check_with(&self, value: &Value, settings: &Settings) -> Result<Value, String> {
        let key = self.key;
        match self.kind {
            Kind::Bool => value.as_bool().map(Value::Bool).ok_or_else(|| format!("{key} must be true or false")),
            Kind::Int { min, max, step, .. } => value
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| (min..=max).contains(n) && (n - min) % step.max(1) == 0)
                .map(|n| json!(n))
                .ok_or_else(|| match step {
                    0 | 1 => format!("{key} must be a whole number from {min} to {max}"),
                    s => format!("{key} must be a whole number from {min} to {max} in steps of {s}"),
                }),
            Kind::Scale { min, max, step } => UiScaleSetting::from_json(value)
                .filter(|s| match s {
                    UiScaleSetting::Auto => true,
                    UiScaleSetting::Percent(p) => (min..=max).contains(p) && (p - min) % step.max(1) == 0,
                })
                .map(UiScaleSetting::to_json)
                .ok_or_else(|| format!("{key} must be \"auto\" or a percentage from {min} to {max} in steps of {step}")),
            Kind::Choice(_) => {
                let choices = self.choices(settings);
                let known = |v: &str| choices.iter().any(|(id, _)| id == v);
                let ids = || choices.iter().map(|(id, _)| format!("\"{id}\"")).collect::<Vec<_>>().join(", ");
                if self.many {
                    let items = value.as_array().filter(|items| items.iter().all(|i| i.as_str().is_some_and(known)));
                    let picked: Option<Vec<&str>> = items.map(|items| items.iter().filter_map(Value::as_str).collect());
                    picked
                        .map(|picked| json!(choices.iter().filter(|(id, _)| picked.contains(&id.as_str())).map(|(id, _)| id).collect::<Vec<_>>()))
                        .ok_or_else(|| format!("{key} must be a list of {}", ids()))
                } else {
                    value.as_str().filter(|v| known(v)).map(|v| json!(v)).ok_or_else(|| format!("{key} must be one of {}", ids()))
                }
            }
            Kind::Text { max_bytes } => match value {
                Value::String(t) if t.len() <= max_bytes => Ok(json!(t)),
                _ => Err(format!("{key} must be text of at most {max_bytes} bytes")),
            },
            Kind::Folder => match value {
                Value::Null => Ok(Value::Null),
                Value::String(p) if crate::settings::folder_path(std::path::Path::new(p)).is_some() => Ok(json!(p)),
                _ => Err(format!("{key} must be an absolute folder path or null")),
            },
        }
    }

    /// Checks `value` and merges it into `settings` under its key (a folder's `null` removes the
    /// stored one, so the default folder is used); every other key is kept. Nothing changes on
    /// `Err`.
    pub fn store(&self, settings: &mut Settings, value: &Value) -> Result<(), String> {
        let stored = self.check_with(value, settings)?;
        let (object, field) = self.place();
        settings.merge(object, &[(field, (!stored.is_null()).then_some(stored))]);
        Ok(())
    }

    /// The row as `options.list` gives it: `{key, tab, group, label, hint, type, default, value,
    /// applies, available, reason, platforms}` and, by type, `min`, `max`, `step`, `unit` or
    /// `choices` (`[{id, label}]`); `value` is what `settings` holds in effect.
    pub fn to_json(&self, settings: &Settings) -> Value {
        let mut row = Map::new();
        row.insert("key".into(), json!(self.key));
        row.insert("tab".into(), json!(self.tab.id()));
        row.insert("group".into(), json!(self.group));
        row.insert("label".into(), json!(self.label));
        row.insert("hint".into(), json!(self.hint));
        row.insert("type".into(), json!(if self.many { "list" } else { self.kind.id() }));
        row.insert("default".into(), self.default_value());
        row.insert("value".into(), (self.read)(settings));
        row.insert("applies".into(), json!(self.applies.id()));
        row.insert("available".into(), json!(self.available()));
        row.insert("reason".into(), json!(self.unavailable));
        row.insert("platforms".into(), json!(self.platforms.iter().map(|p| p.name()).collect::<Vec<_>>()));
        match self.kind {
            Kind::Int { min, max, step, unit } => {
                row.extend([("min".into(), json!(min)), ("max".into(), json!(max)), ("step".into(), json!(step)), ("unit".into(), json!(unit))]);
            }
            Kind::Scale { min, max, step } => {
                row.extend([("min".into(), json!(min)), ("max".into(), json!(max)), ("step".into(), json!(step)), ("unit".into(), json!("%"))]);
            }
            Kind::Choice(_) => {
                row.insert("choices".into(), self.choices(settings).iter().map(|(id, label)| json!({ "id": id, "label": label })).collect());
            }
            Kind::Text { max_bytes } => {
                row.insert("maxBytes".into(), json!(max_bytes));
            }
            Kind::Bool | Kind::Folder => {}
        }
        Value::Object(row)
    }
}

/// Every option, in the order the dialog lists them.
pub fn catalogue() -> &'static [OptionSpec] {
    CATALOGUE
}

/// The option stored under `key`.
pub fn find(key: &str) -> Option<&'static OptionSpec> {
    CATALOGUE.iter().find(|o| o.key == key)
}

/// The options of `tab`, in order.
pub fn on_tab(tab: Tab) -> impl Iterator<Item = &'static OptionSpec> {
    CATALOGUE.iter().filter(move |o| o.tab == tab)
}

const SOUND: &str = "Arrives with sound support";
const OS_INTEGRATION: &str = "Arrives with OS integration";
const TRANSITIONS: &[(&str, &str)] = &[
    ("cut", "Cut"),
    ("panAndZoom", "Pan and Zoom"),
    ("dissolve", "Dissolve"),
    ("wipe", "Wipe"),
    ("circle", "Circle"),
    ("push", "Push"),
    ("rectangle", "Rectangle"),
];
const WHEEL: &[(&str, &str)] =
    &[("zoom", "Mouse wheel zooms (hold Ctrl to navigate)"), ("navigate", "Mouse wheel moves through photos (hold Ctrl to zoom)")];
const FONT_SIZES: &[(&str, &str)] = &[("small", "Small"), ("medium", "Medium"), ("large", "Large")];
/// The languages an empty settings file offers ([`Settings::languages`]): the app gives its own.
const LANGUAGES: &[(&str, &str)] = &[("system", "System"), ("en", "English")];
/// The Photo Viewer's file types ([`crate::settings::VIEWER_TYPES`]), by their ids.
const VIEWER_TYPES: &[(&str, &str)] = &[
    ("jpeg", "JPEG"),
    ("png", "PNG"),
    ("gif", "GIF"),
    ("bmp", "BMP"),
    ("tiff", "TIFF"),
    ("webp", "WebP"),
    ("heif", "HEIC/HEIF"),
    ("avif", "AVIF"),
    ("jxl", "JPEG XL"),
    ("psd", "PSD"),
    ("raw", "RAW formats"),
];
const QUALITIES: &[(&str, &str)] =
    &[("automatic", "Automatic"), ("normal", "Normal"), ("maximum", "Maximum"), ("minimum", "Minimum"), ("custom", "Custom")];
const PROXIES: &[(&str, &str)] = &[("system", "Use the system's proxy settings"), ("manual", "Manual")];
const NETWORK_LOG: &[(&str, &str)] = &[("errors", "Errors only"), ("requests", "Requests"), ("details", "Details")];
const CONFIDENCE: Kind = Kind::Int { min: *CONFIDENCE_RANGE.start(), max: *CONFIDENCE_RANGE.end(), step: CONFIDENCE_STEP, unit: "" };
const HEIF: Option<&str> = if cfg!(feature = "heif") { None } else { Some("HEIC support isn't included in this build") };

const fn file_type(key: &'static str, label: &'static str, read: fn(&Settings) -> Value) -> OptionSpec {
    row(key, Tab::FileTypes, "Show JPEG files and:", label, Kind::Bool, Initial::Bool(true), read).applies(Applies::Rescan)
}

static CATALOGUE: &[OptionSpec] = &[
    // General → Interface.
    row("interface.animations", Tab::General, "Interface", "Use animations", Kind::Bool, Initial::Bool(true), |s| json!(s.interface().animations))
        .hint("Smooth transitions when rotating, applying edits and previewing."),
    row("interface.tooltips", Tab::General, "Interface", "Show tooltips", Kind::Bool, Initial::Bool(true), |s| json!(s.interface().tooltips)),
    row("interface.systemTitleBar", Tab::General, "Interface", "Use system title bar", Kind::Bool, Initial::Bool(false), |s| {
        json!(s.interface().system_title_bar)
    })
    .hint("Applies at next launch.")
    .applies(Applies::Restart)
    .only(&[Platform::Windows, Platform::Linux]),
    row(
        "edit.singleClickLeaves",
        Tab::General,
        "Interface",
        "Single click on the photo returns to the library",
        Kind::Bool,
        Initial::Bool(false),
        |s| json!(s.edit().single_click_leaves),
    ),
    row(
        "interface.uiScale",
        Tab::General,
        "Interface",
        "UI scale",
        Kind::Scale { min: *UI_SCALE_RANGE.start(), max: *UI_SCALE_RANGE.end(), step: UI_SCALE_STEP },
        Initial::Str("auto"),
        |s| s.interface().ui_scale.to_json(),
    )
    .hint("Auto follows the system's scale and the monitor."),
    row("interface.fontSize", Tab::General, "Interface", "UI font size", Kind::Choice(FONT_SIZES), Initial::Str("small"), |s| {
        json!(s.interface().font_size.id())
    }),
    row("interface.language", Tab::General, "Interface", "Language", Kind::Choice(LANGUAGES), Initial::Str("system"), |s| {
        json!(s.interface().language)
    })
    .given(Settings::languages),
    // General → Files.
    row("files.readLegacyData", Tab::General, "Files", "Read photo data left by the previous organizer", Kind::Bool, Initial::Bool(true), |s| {
        json!(s.legacy().read_legacy_data)
    })
    .applies(Applies::Rescan),
    row("import.detectDuplicates", Tab::General, "Files", "Detect duplicates while importing", Kind::Bool, Initial::Bool(true), |s| {
        json!(s.import().detect_duplicates)
    }),
    row("organize.deleteWithoutAsking", Tab::General, "Files", "Delete from disk without asking", Kind::Bool, Initial::Bool(false), |s| {
        json!(s.organize().delete_without_asking)
    }),
    row(
        "organize.removeFromAlbumsWithoutAsking",
        Tab::General,
        "Files",
        "Remove from albums without asking",
        Kind::Bool,
        Initial::Bool(false),
        |s| json!(s.organize().remove_from_albums_without_asking),
    ),
    row(
        "files.writeCaptionsTagsPlaces",
        Tab::General,
        "Files",
        "Write captions, tags and places into photo files",
        Kind::Bool,
        Initial::Bool(true),
        |s| json!(s.metadata_writes().captions_tags_places),
    ),
    row("files.writeStarsAsRatings", Tab::General, "Files", "Write stars as ratings (XMP)", Kind::Bool, Initial::Bool(false), |s| {
        json!(s.metadata_writes().stars_as_ratings)
    }),
    row(
        "files.updateModificationTime",
        Tab::General,
        "Files",
        "Update file modification time on metadata changes",
        Kind::Bool,
        Initial::Bool(false),
        |s| json!(s.metadata_writes().update_modification_time),
    ),
    row("files.writeRawXmpSidecars", Tab::General, "Files", "Write XMP sidecars for RAW files", Kind::Bool, Initial::Bool(false), |s| {
        json!(s.metadata_writes().raw_xmp_sidecars)
    }),
    row("files.writeDates", Tab::General, "Files", "Write date changes to files", Kind::Bool, Initial::Bool(true), |s| {
        json!(s.metadata_writes().dates)
    }),
    // General → Import.
    row("import.folder", Tab::General, "Import", "Save imported pictures in", Kind::Folder, Initial::Null, |s| json!(s.import().folder))
        .hint("The Pictures folder unless chosen."),
    // File Types.
    file_type("fileTypes.png", "PNG", |s| json!(s.file_types().allows(FileType::Png))),
    file_type("fileTypes.gif", "GIF", |s| json!(s.file_types().allows(FileType::Gif))),
    file_type("fileTypes.bmp", "BMP", |s| json!(s.file_types().allows(FileType::Bmp))),
    file_type("fileTypes.tiff", "TIFF", |s| json!(s.file_types().allows(FileType::Tiff))),
    file_type("fileTypes.webp", "WebP", |s| json!(s.file_types().allows(FileType::WebP))),
    file_type("fileTypes.heif", "HEIC/HEIF", |s| json!(s.file_types().allows(FileType::Heif))).unavailable_if(HEIF),
    file_type("fileTypes.avif", "AVIF", |s| json!(s.file_types().allows(FileType::Avif))),
    file_type("fileTypes.jxl", "JPEG XL", |s| json!(s.file_types().allows(FileType::JpegXl))),
    file_type("fileTypes.psd", "PSD", |s| json!(s.file_types().allows(FileType::Psd))),
    file_type("fileTypes.raw", "RAW formats", |s| json!(s.file_types().allows(FileType::Raw))),
    file_type("fileTypes.video", "Videos (MP4, MOV, M4V, MKV)", |s| json!(s.file_types().allows(FileType::Video))),
    row(
        "fileTypes.smallPictureThreshold",
        Tab::FileTypes,
        "Small pictures",
        "Small-picture threshold",
        Kind::Int { min: *SMALL_PICTURE_RANGE.start(), max: *SMALL_PICTURE_RANGE.end(), step: 1, unit: "px" },
        Initial::Int(SMALL_PICTURE_DEFAULT),
        |s| json!(s.small_picture_threshold()),
    )
    .hint("Pictures smaller than this on both sides show only with View › Small Pictures.")
    .applies(Applies::Rescan),
    // Slideshow.
    row("slideshow.loop", Tab::Slideshow, "Slideshow", "Loop slideshow", Kind::Bool, Initial::Bool(false), |s| json!(s.slideshow().looping)),
    row("slideshow.music", Tab::Slideshow, "Slideshow", "Play music during slideshows", Kind::Bool, Initial::Bool(false), |s| {
        json!(s.slideshow().music)
    })
    .later(SOUND),
    row("slideshow.musicFolder", Tab::Slideshow, "Slideshow", "Music folder", Kind::Folder, Initial::Null, |s| json!(s.slideshow().music_folder))
        .hint("Every MP3, OGG, FLAC and WAV file in it plays in name order, looping.")
        .later(SOUND),
    row(
        "slideshow.displayTime",
        Tab::Slideshow,
        "Slideshow",
        "Display time",
        Kind::Int { min: *DISPLAY_TIME_RANGE.start(), max: *DISPLAY_TIME_RANGE.end(), step: 1, unit: "s" },
        Initial::Int(DISPLAY_TIME_DEFAULT),
        |s| json!(s.slideshow().display_time),
    ),
    row("slideshow.transition", Tab::Slideshow, "Slideshow", "Transition", Kind::Choice(TRANSITIONS), Initial::Str("dissolve"), |s| {
        json!(s.slideshow().transition.id())
    }),
    row("slideshow.captions", Tab::Slideshow, "Slideshow", "Show captions", Kind::Bool, Initial::Bool(true), |s| json!(s.slideshow().captions)),
    row("slideshow.videoSound", Tab::Slideshow, "Slideshow", "Play video sound", Kind::Bool, Initial::Bool(false), |s| {
        json!(s.slideshow().video_sound)
    })
    .later(SOUND),
    // Network → Map.
    row("network.allowMapTiles", Tab::Network, "Map", "Allow map tiles", Kind::Bool, Initial::Bool(false), |s| json!(s.network().allow_map_tiles))
        .hint("The Places pane fetches map pictures from the tile server while it is open.")
        .unavailable_if(TILES_UNAVAILABLE),
    row(
        "network.tileServer",
        Tab::Network,
        "Map",
        "Map tile server",
        Kind::Text { max_bytes: MAX_TEMPLATE_BYTES },
        Initial::Str(DEFAULT_TILE_SERVER),
        |s| json!(s.network().tile_server),
    )
    .hint("A web address with {z}, {x} and {y} for the tile's zoom, column and row.")
    .unavailable_if(TILES_UNAVAILABLE),
    row(
        "network.tileAttribution",
        Tab::Network,
        "Map",
        "Map attribution",
        Kind::Text { max_bytes: MAX_ATTRIBUTION_BYTES },
        Initial::Str(DEFAULT_ATTRIBUTION),
        |s| json!(s.network().tile_attribution),
    )
    .hint("Shown on the map under the tiles.")
    .unavailable_if(TILES_UNAVAILABLE),
    row(
        "network.geocoder",
        Tab::Network,
        "Map",
        "Geocoding service",
        Kind::Text { max_bytes: MAX_TEMPLATE_BYTES },
        Initial::Str(DEFAULT_GEOCODER),
        |s| json!(s.network().geocoder),
    )
    .hint("A web address with {query} for the place typed in \"Search for a place\".")
    .unavailable_if(TILES_UNAVAILABLE),
    // Network → Proxy.
    row("network.proxy", Tab::Network, "Proxy", "Proxy", Kind::Choice(PROXIES), Initial::Str("system"), |s| json!(s.network().proxy.id()))
        .unavailable_if(PROXY_UNAVAILABLE),
    row("network.proxyHost", Tab::Network, "Proxy", "Proxy host", Kind::Text { max_bytes: MAX_HOST_BYTES }, Initial::Str(""), |s| {
        json!(s.network().proxy_host)
    })
    .unavailable_if(PROXY_UNAVAILABLE),
    row(
        "network.proxyPort",
        Tab::Network,
        "Proxy",
        "Proxy port",
        Kind::Int { min: *PROXY_PORT_RANGE.start() as u32, max: *PROXY_PORT_RANGE.end() as u32, step: 1, unit: "" },
        Initial::Int(PROXY_PORT_DEFAULT as u32),
        |s| json!(s.network().proxy_port),
    )
    .unavailable_if(PROXY_UNAVAILABLE),
    // Network → Troubleshooting.
    row("network.logLevel", Tab::Network, "Troubleshooting", "Network log level", Kind::Choice(NETWORK_LOG), Initial::Str("errors"), |s| {
        json!(s.network().log_level.id())
    })
    .later("Arrives with the network log"),
    // People → Faces.
    row("people.findFaces", Tab::People, "Faces", "Find faces in my photos", Kind::Bool, Initial::Bool(true), |s| json!(s.people().find_faces))
        .hint("Faces are found on this computer; nothing is sent anywhere."),
    row("people.suggestNames", Tab::People, "Faces", "Suggest names", Kind::Bool, Initial::Bool(true), |s| json!(s.people().suggest_names)),
    row("people.suggestionConfidence", Tab::People, "Faces", "Suggestion confidence", CONFIDENCE, Initial::Int(CONFIDENCE_DEFAULT), |s| {
        json!(s.people().suggestion_confidence)
    })
    .hint("Higher values make fewer, more certain suggestions."),
    row("people.groupingConfidence", Tab::People, "Faces", "Grouping confidence", CONFIDENCE, Initial::Int(CONFIDENCE_DEFAULT), |s| {
        json!(s.people().grouping_confidence)
    })
    .hint("Higher values make smaller, more certain groups of unnamed faces."),
    // People → Files.
    row("people.storeNamesInPhotos", Tab::People, "Files", "Store people names in photos", Kind::Bool, Initial::Bool(false), |s| {
        json!(s.people().store_names_in_photos)
    })
    .hint("Writes the names of the people in a photo into its file as face regions (XMP)."),
    // Photo Viewer.
    row(
        "viewer.defaultViewer",
        Tab::PhotoViewer,
        "File Types",
        "Use Dalica as the default viewer for these file types",
        Kind::Choice(VIEWER_TYPES),
        Initial::List(&[]),
        |s| json!(s.viewer().default_viewer.iter().map(|t| t.id()).collect::<Vec<_>>()),
    )
    .many()
    .later(OS_INTEGRATION),
    row("viewer.wheel", Tab::PhotoViewer, "Interface", "Mouse wheel", Kind::Choice(WHEEL), Initial::Str("zoom"), |s| json!(s.viewer().wheel.id())),
    row("viewer.hideControls", Tab::PhotoViewer, "Interface", "Hide controls automatically", Kind::Bool, Initial::Bool(true), |s| {
        json!(s.viewer().hide_controls)
    }),
    row("viewer.translucent", Tab::PhotoViewer, "Interface", "Translucent background", Kind::Bool, Initial::Bool(true), |s| {
        json!(s.viewer().translucent)
    }),
    // Export.
    row("export.location", Tab::Export, "Location", "Default export location", Kind::Folder, Initial::Null, |s| json!(s.export().location))
        .hint("Pictures/Dalica/Exports unless chosen."),
    row("export.resize", Tab::Export, "Size and quality", "Resize to", Kind::Bool, Initial::Bool(true), |s| json!(s.export().resize))
        .hint("Off: use the original size."),
    row(
        "export.size",
        Tab::Export,
        "Size and quality",
        "Size",
        Kind::Int { min: *EXPORT_SIZE_RANGE.start(), max: *EXPORT_SIZE_RANGE.end(), step: 1, unit: "px" },
        Initial::Int(EXPORT_SIZE_DEFAULT),
        |s| json!(s.export().size),
    ),
    row("export.quality", Tab::Export, "Size and quality", "Image quality", Kind::Choice(QUALITIES), Initial::Str("automatic"), |s| {
        json!(s.export().quality.id())
    }),
    row(
        "export.customQuality",
        Tab::Export,
        "Size and quality",
        "Custom quality",
        Kind::Int { min: *QUALITY_RANGE.start() as u32, max: *QUALITY_RANGE.end() as u32, step: 1, unit: "" },
        Initial::Int(CUSTOM_QUALITY_DEFAULT as u32),
        |s| json!(s.export().custom_quality),
    ),
    row("export.removeLocation", Tab::Export, "Privacy", "Remove location from exported photos", Kind::Bool, Initial::Bool(false), |s| {
        json!(s.export().remove_location)
    }),
    row(
        "export.watermarkText",
        Tab::Export,
        "Watermark",
        "Default watermark text",
        Kind::Text { max_bytes: MAX_WATERMARK_BYTES },
        Initial::Str(""),
        |s| json!(s.export().watermark_text),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// One test of the value checks; the catalogue itself is tested in `tests/options_catalogue.rs`.
    #[test]
    fn values_are_checked_by_type_and_a_folders_null_removes_it() {
        let scale = find("interface.uiScale").unwrap();
        for ok in [json!("auto"), json!(75), json!(150), json!(300)] {
            assert_eq!(scale.check(&ok), Ok(ok.clone()));
        }
        for bad in [json!(80), json!(50), json!(325), json!("150"), json!(null), json!(1.5)] {
            assert!(scale.check(&bad).unwrap_err().starts_with("interface.uiScale must be"), "{bad}");
        }
        let threshold = find("fileTypes.smallPictureThreshold").unwrap();
        assert!(threshold.check(&json!(16)).is_ok() && threshold.check(&json!(1024)).is_ok());
        assert!(threshold.check(&json!(15)).is_err() && threshold.check(&json!(-1)).is_err() && threshold.check(&json!(true)).is_err());
        let font = find("interface.fontSize").unwrap();
        assert!(font.check(&json!("large")).is_ok() && font.check(&json!("huge")).is_err());
        let text = find("export.watermarkText").unwrap();
        assert!(text.check(&json!("x".repeat(MAX_WATERMARK_BYTES))).is_ok());
        assert_eq!(
            text.check(&json!("secret".repeat(1000))),
            Err(format!("export.watermarkText must be text of at most {MAX_WATERMARK_BYTES} bytes"))
        );

        let location = find("export.location").unwrap();
        let mut st = Settings::parse(br#"{"export": {"future": 1}}"#).unwrap();
        let folder = std::env::temp_dir().join("Out");
        location.store(&mut st, &json!(folder)).unwrap();
        assert_eq!(st.export().location, Some(folder));
        assert!(location.store(&mut st, &json!("relative/folder")).is_err());
        location.store(&mut st, &Value::Null).unwrap();
        assert_eq!(st.get("export").unwrap(), &json!({ "future": 1 }), "null removes it; other keys kept");
    }
}
