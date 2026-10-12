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
//! Stored numbers are hostile like any input: a value that is not a finite number in its range is
//! dropped (the default is used), never trusted.

use std::collections::BTreeMap;
use std::path::Path;

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
