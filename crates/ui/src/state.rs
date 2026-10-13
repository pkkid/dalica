//! `UiState`: the serialisable view state (`architecture.md` §10, `control-protocol.md` §6.5).
//! Plain data, so `ui.state` and `ui.set` can read and drive it and tests can assert on it. Library
//! state never lives here: it is the engine's.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use dalica_core::{FolderId, PhotoId};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Which view fills the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum View {
    #[default]
    Library,
    /// One photo, with the edit panel (`editing.md`); [`UiState::edit`] says which.
    Edit,
}

/// A section of the light table: the photos of one folder (albums join in M9). In the protocol
/// it is written as in its widget id, `"folder:12"` (`control-protocol.md` §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Folder(FolderId),
}

impl fmt::Display for Section {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Section::Folder(id) => write!(f, "folder:{id}"),
        }
    }
}

impl FromStr for Section {
    type Err = String;

    fn from_str(s: &str) -> Result<Section, String> {
        let bad = || format!("`{}` is not a section; sections are written `folder:<FolderId>`", s.chars().take(40).collect::<String>());
        match s.split_once(':') {
            Some(("folder", id)) => id.parse::<FolderId>().map(Section::Folder).map_err(|_| bad()),
            _ => Err(bad()),
        }
    }
}

impl Serialize for Section {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Section {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Section, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// The photos selected on the light table (`ui-layout.md` §5.4). A selection never crosses a
/// section: `section` is the one its photos belong to.
///
/// [`Selection::contains`] is O(1): every thumbnail cell on screen asks it each frame, and a
/// section selected with Ctrl+A may hold 100 000 photos. It looks the id up in an index of `ids`
/// built on first use and kept beside them (not serialised). Build a selection with
/// [`Selection::new`] (or `..Default::default()`) and change its photos with
/// [`Selection::set_ids`] or by building a new one: an index that no longer matches `ids` is never
/// trusted (the lookup falls back to a search), but writing `ids` in place is slow from then on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    /// Selected photo ids, in the order they were selected.
    pub ids: Vec<PhotoId>,
    /// The focused photo: the anchor of a Shift range, drawn with the white outline.
    pub anchor: Option<PhotoId>,
    /// The section the selected photos belong to; `None` when nothing is selected (or, until the
    /// selection model of M4.13, when a script selected photos of several folders).
    pub section: Option<Section>,
    /// The section was selected as a unit (a tree row or a section header): the tray shows it as
    /// one token ("Folder selected — 48 photos", `ui-layout.md` §6.2) instead of its photos.
    pub whole: bool,
    /// The lookup index of `ids` behind [`Selection::contains`].
    #[serde(skip)]
    index: Index,
}

impl Selection {
    /// A selection of `ids` (in the order selected) with its anchor, section and whole flag.
    pub fn new(ids: Vec<PhotoId>, anchor: Option<PhotoId>, section: Option<Section>, whole: bool) -> Selection {
        Selection { ids, anchor, section, whole, index: Index::default() }
    }

    /// Replaces the selected photos (anchor, section and whole flag stay as they are).
    pub fn set_ids(&mut self, ids: Vec<PhotoId>) {
        self.ids = ids;
        self.index = Index::default();
    }

    /// Whether photo `id` is selected, in O(1) (a short selection is searched directly).
    pub fn contains(&self, id: PhotoId) -> bool {
        self.index.contains(&self.ids, id)
    }
}

/// Lists up to this long are searched directly: cheaper than hashing.
const INDEX_FROM: usize = 16;

/// The lookup index of a list of photo ids, built on its first use. It is part of no comparison,
/// never serialised, and a clone starts without one (it is rebuilt on the clone's first lookup).
/// An index that was not built from the list it is asked about (the list was replaced in place) is
/// never trusted: the list is searched instead.
#[derive(Default)]
struct Index(std::sync::OnceLock<Built>);

impl Index {
    fn contains(&self, ids: &[PhotoId], id: PhotoId) -> bool {
        if ids.len() <= INDEX_FROM {
            return ids.contains(&id);
        }
        let built = self.0.get_or_init(|| Built::of(ids));
        if built.key == Built::key(ids) { built.set.contains(&id) } else { ids.contains(&id) }
    }
}

/// An index and the `ids` it was built from: their buffer and length.
struct Built {
    key: (usize, usize),
    set: HashSet<PhotoId>,
}

impl Built {
    fn key(ids: &[PhotoId]) -> (usize, usize) {
        (ids.as_ptr() as usize, ids.len())
    }

    fn of(ids: &[PhotoId]) -> Built {
        Built { key: Built::key(ids), set: ids.iter().copied().collect() }
    }
}

impl Clone for Index {
    fn clone(&self) -> Index {
        Index::default()
    }
}

impl PartialEq for Index {
    fn eq(&self, _: &Index) -> bool {
        true
    }
}

impl Eq for Index {}

impl fmt::Debug for Index {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Index")
    }
}

/// The side pane shown at the right of the light table (`ui-layout.md` §7); one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pane {
    People,
    Places,
    Tags,
    Properties,
}

/// How the library tree shows folders (`organizing.md` §10): every folder as one row (`flat`, the
/// default) or the disk hierarchy under each watched root (`tree`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TreeMode {
    #[default]
    Flat,
    Tree,
}

/// The order of the folders in the tree and of their sections on the light table (`organizing.md`
/// §10, the view-options popup of `ui-layout.md` §3): by date (a folder's oldest capture date,
/// newest first; the default), by recent changes (newest file first), by size, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FolderSort {
    #[default]
    Date,
    Recent,
    Size,
    Name,
}

/// The groups of the library tree (`ui-layout.md` §4), named as in their widget ids
/// (`tree:group:<name>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    Albums,
    People,
    Projects,
    Folders,
    Other,
    Hidden,
}

impl Group {
    pub const ALL: [Group; 6] = [Group::Albums, Group::People, Group::Projects, Group::Folders, Group::Other, Group::Hidden];
}

/// The line under each thumbnail (View ▸ Thumbnail Caption, `menus-and-shortcuts.md` §1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Caption {
    #[default]
    None,
    Filename,
    Caption,
    Tags,
    Resolution,
    Date,
}

/// The Photo Tray's own state (`ui-layout.md` §6.2). What it shows is the selection plus the held
/// photos; it is not persisted across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tray {
    /// Photos kept in the tray by Hold (Ctrl+H), in the order they were held. Change them with
    /// [`Tray::set_held`] so [`Tray::holds`] stays O(1).
    pub held: Vec<PhotoId>,
    /// The tray's own selection (what Clear removes), photos the tray shows.
    pub selected: Vec<PhotoId>,
    /// The lookup index of `held` behind [`Tray::holds`].
    #[serde(skip)]
    held_index: Index,
}

impl Tray {
    /// Whether photo `id` is held, in O(1).
    pub fn holds(&self, id: PhotoId) -> bool {
        self.held_index.contains(&self.held, id)
    }

    /// Replaces the held photos.
    pub fn set_held(&mut self, held: Vec<PhotoId>) {
        self.held = held;
        self.held_index = Index::default();
    }
}

/// Where the light table is scrolled to, kept by its content rather than by pixels so it survives a
/// resize, a new thumbnail size or a caption change: the section at the top of the view and how far
/// below that section's top the view starts, in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScrollAnchor {
    /// `None`: the top of the library.
    pub section: Option<Section>,
    /// `0 ..= SCROLL_OFFSET_MAX`.
    pub offset: f32,
}

/// The farthest below its section's top a scroll anchor may be, in points (more than any section
/// of the largest library is tall).
pub const SCROLL_OFFSET_MAX: f32 = 1.0e9;

/// The most photos the tray holds or selects, and the most folders kept collapsed.
pub const MAX_IDS: usize = 1_000_000;

/// Default, smallest library-tree width, and the width the light table keeps beside it (points,
/// `ui-layout.md` §2: the tree is 200 … W − 480 wide).
pub const TREE_DEFAULT: f32 = 240.0;
pub const TREE_MIN: f32 = 200.0;
pub const LIGHT_TABLE_MIN: f32 = 480.0;
/// The widest tree width kept in the view state: wider than any window (the layout clamps it to
/// the window's W − 480 anyway).
pub const TREE_MAX: f32 = 16_384.0;

/// Smallest, default and largest thumbnail cell edge (points, `design-system.md` §4).
pub const THUMB_MIN: f32 = 64.0;
pub const THUMB_DEFAULT: f32 = 144.0;
pub const THUMB_MAX: f32 = 320.0;

/// `v` clamped to `min ..= max`, or `default` when it is not a finite number.
fn clamp_or(v: f32, min: f32, max: f32, default: f32) -> f32 {
    if v.is_finite() { v.clamp(min, max) } else { default }
}

/// A thumbnail cell edge safe to lay out: `THUMB_MIN ..= THUMB_MAX`, the default when not finite.
pub fn thumb_size(v: f32) -> f32 {
    clamp_or(v, THUMB_MIN, THUMB_MAX, THUMB_DEFAULT)
}

/// A tree width safe to keep: `TREE_MIN ..= TREE_MAX`, the default when not finite.
pub fn tree_width(v: f32) -> f32 {
    clamp_or(v, TREE_MIN, TREE_MAX, TREE_DEFAULT)
}

/// The longest draft an inline editor or a pane field keeps, in characters (a caption, a section
/// description, the Tags pane's "Add a tag:" text). Longer text is refused by `ui.set` and cut by
/// [`UiState::sanitized`].
pub const MAX_DRAFT: usize = 4096;

/// The most rows an autocomplete list holds (`dalica_ui_kit::widgets::MAX_SUGGESTIONS`): a
/// highlight is an index below it.
pub const MAX_HIGHLIGHT: usize = dalica_ui_kit::widgets::MAX_SUGGESTIONS;

/// The inline caption editor under a photo's cell (Picture ▸ Edit Caption, `organizing.md` §5.3):
/// the photo it captions and the text typed so far. Nothing is applied before it commits (Enter, a
/// click elsewhere), which runs `photo.caption.set`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptionEdit {
    pub photo: PhotoId,
    pub draft: String,
}

/// The inline description editor of a section header (`ui-layout.md` §5.2): the section and the
/// text typed so far; it commits through `folder.info.set`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DescriptionEdit {
    pub section: Section,
    pub draft: String,
}

/// The Tags pane's own state (`organizing.md` §5.2): the "Add a tag:" field's text and the row of
/// its autocomplete list that ↑ / ↓ highlight (`None`: none; Enter then adds the typed text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TagsPane {
    pub draft: String,
    pub highlight: Option<usize>,
}

/// The tabs of the About dialog (`contributors.md` → In Dalica).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AboutTab {
    #[default]
    About,
    Contributors,
    Models,
}

/// How About ▸ Contributors lists people: names flowing as text, or a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CreditsList {
    #[default]
    GrabBag,
    Table,
}

/// What About ▸ Contributors sorts by (`contributors.md` → Decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CreditsSort {
    Name,
    Prs,
    Commits,
    LinesAdded,
    LinesDeleted,
    LinesDelta,
    BinaryAdded,
    BinaryDeleted,
    #[default]
    FirstCommit,
    LastCommit,
}

/// Which name About ▸ Contributors shows; a missing display or real name falls back to
/// `@username`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum NameMode {
    #[default]
    Username,
    DisplayName,
    RealName,
}

/// The About dialog's view: its tab, and how the Contributors tab lists people. Kept while the app
/// runs, so the dialog reopens as it was left; not persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct About {
    pub tab: AboutTab,
    pub list: CreditsList,
    pub sort: CreditsSort,
    /// Oldest or A first; counts read biggest first when `false`.
    pub ascending: bool,
    pub names: NameMode,
}

impl Default for About {
    fn default() -> About {
        About { tab: AboutTab::About, list: CreditsList::GrabBag, sort: CreditsSort::FirstCommit, ascending: true, names: NameMode::Username }
    }
}

/// The kinds of in-app dialog the host draws (`crate::dialogs`); each is `dialog:<kind>` on screen
/// and in `ui.inspect` (`control-protocol.md` §7). A new dialog adds its kind here and its module
/// under `dialogs/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DialogKind {
    /// "Clear all photos from the tray?" (`tray.clear` with held photos).
    ClearTray,
    /// Tools ▸ Folder Manager… (`organizing.md` §2).
    FolderManager,
    /// The first-run panel of a new library (`organizing.md` §1).
    FirstRun,
    /// "Remove the folder … from Dalica?" (`folder.remove`, `organizing.md` §2).
    RemoveFolder,
    /// "Watch the whole drive anyway?" (a volume root or the home folder set to Scan Always).
    WatchDrive,
    /// Folder Properties (`section.properties`, `organizing.md` §10).
    FolderProperties,
    /// Adjust Date and Time (`photo.adjustDate`, `organizing.md` §9).
    AdjustDate,
    /// Configure Quick Tags (`quicktags.configure`, `organizing.md` §5.2).
    QuickTags,
    /// Rename Tag (`tag.rename`, the Tags pane's tag menu).
    RenameTag,
    /// About Dalica (`app.about`, `contributors.md`).
    About,
    /// Help ▸ Release Notes (`help.releaseNotes`).
    ReleaseNotes,
    /// The Crop tool's Add Custom Ratio… (`editing.md` §6.1).
    CustomRatio,
}

impl DialogKind {
    pub const ALL: [DialogKind; 12] = [
        DialogKind::ClearTray,
        DialogKind::FolderManager,
        DialogKind::FirstRun,
        DialogKind::RemoveFolder,
        DialogKind::WatchDrive,
        DialogKind::FolderProperties,
        DialogKind::AdjustDate,
        DialogKind::QuickTags,
        DialogKind::RenameTag,
        DialogKind::About,
        DialogKind::ReleaseNotes,
        DialogKind::CustomRatio,
    ];

    /// The kind as the protocol writes it (`clearTray`), the `<kind>` of `dialog:<kind>`.
    pub fn name(self) -> &'static str {
        match self {
            DialogKind::ClearTray => "clearTray",
            DialogKind::FolderManager => "folderManager",
            DialogKind::FirstRun => "firstRun",
            DialogKind::RemoveFolder => "removeFolder",
            DialogKind::WatchDrive => "watchDrive",
            DialogKind::FolderProperties => "folderProperties",
            DialogKind::AdjustDate => "adjustDate",
            DialogKind::QuickTags => "quickTags",
            DialogKind::RenameTag => "renameTag",
            DialogKind::About => "about",
            DialogKind::ReleaseNotes => "releaseNotes",
            DialogKind::CustomRatio => "customRatio",
        }
    }

    /// The kind named `name`, if there is one.
    pub fn from_name(name: &str) -> Option<DialogKind> {
        DialogKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// A command a dialog runs when it is confirmed: the confirmation of a command that asks first
/// (`control-protocol.md` §5.5) keeps what was asked for here and runs it with `"confirm": true`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DialogCommand {
    pub id: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// An open dialog: its kind and its draft. Nothing a dialog edits is applied before its default
/// button; the draft is plain data, so `ui.state` shows it and tests assert on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dialog {
    pub kind: DialogKind,
    /// The draft: field values by name (`field:dialog.<name>`).
    #[serde(default)]
    pub fields: serde_json::Map<String, serde_json::Value>,
    /// The command its default button runs, for a confirmation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<DialogCommand>,
}

impl Dialog {
    /// A dialog of `kind` with an empty draft.
    pub fn new(kind: DialogKind) -> Dialog {
        Dialog { kind, fields: serde_json::Map::new(), command: None }
    }
}

/// The tabs of the edit panel (`editing.md` §5), named as in their widget ids (`tab:edit.basic`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum EditTab {
    /// Basic fixes: Crop, Straighten, Retouch, the automatic fixes, Fill Light.
    #[default]
    Basic,
    /// Lighting and color tuning.
    Tuning,
    /// The three effect tabs (M12).
    Effects,
    Effects2,
    Effects3,
}

/// The tools of the edit view that hold a draft until Apply or Cancel (`editing.md` §6–§7), named
/// as in their widget ids (`tool:crop`). Crop, Straighten and Retouch replace the tab's content
/// with their panel; a Tuning session is the Tuning tab's sliders being moved, one history step
/// when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    Crop,
    Straighten,
    Retouch,
    Tuning,
}

impl ToolKind {
    pub const ALL: [ToolKind; 4] = [ToolKind::Crop, ToolKind::Straighten, ToolKind::Retouch, ToolKind::Tuning];

    /// The tool as the protocol writes it (`crop`).
    pub fn name(self) -> &'static str {
        match self {
            ToolKind::Crop => "crop",
            ToolKind::Straighten => "straighten",
            ToolKind::Retouch => "retouch",
            ToolKind::Tuning => "tuning",
        }
    }
}

/// The compare toggle of the edit top bar (`editing.md` §4): one photo, two different photos
/// (A│B), the same photo twice (A│A). Only `one` is drawn enabled until M11.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Compare {
    #[default]
    One,
    Two,
    Same,
}

/// The smallest and largest zoom scale kept (relative to the photo's actual pixels: 1 is 100 %,
/// 4 is the slider's 400 % end, `editing.md` §3). A scale below the fitted one is shown fitted.
pub const ZOOM_MIN: f32 = 0.001;
pub const ZOOM_MAX: f32 = 4.0;

/// The edit view's zoom: fitted to the photo area, or a scale of the photo's actual pixels
/// (`"fit"` or a number in the protocol).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Zoom {
    #[default]
    Fit,
    /// `ZOOM_MIN ..= ZOOM_MAX`; 1 is 100 %.
    Scale(f32),
}

impl Serialize for Zoom {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Zoom::Fit => s.serialize_str("fit"),
            Zoom::Scale(v) => s.serialize_f32(*v),
        }
    }
}

impl<'de> Deserialize<'de> for Zoom {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Zoom, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Name(String),
            Scale(f32),
        }
        match Raw::deserialize(d).map_err(|_| serde::de::Error::custom("`zoom` is \"fit\" or a scale (1 is 100 %)"))? {
            Raw::Name(n) if n == "fit" => Ok(Zoom::Fit),
            Raw::Name(n) => Err(serde::de::Error::custom(format!("`zoom` is \"fit\" or a number, not `{}`", n.chars().take(40).collect::<String>()))),
            Raw::Scale(v) => Ok(Zoom::Scale(v)),
        }
    }
}

/// The longest crop-ratio name kept, in characters.
pub const MAX_RATIO: usize = 64;

/// The Crop tool's draft (`editing.md` §6.1): the size row chosen and the rectangle drawn so far,
/// `[x, y, w, h]` normalised to the photo in the stored frame (`data-model.md` §3.3), `null` before
/// one is drawn. `ratio` is `manual`, `current` or `W:H` (`4:6`, `8.5:11`, `1:1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CropDraft {
    pub ratio: String,
    pub rect: Option<[f32; 4]>,
}

impl Default for CropDraft {
    fn default() -> CropDraft {
        CropDraft { ratio: "manual".into(), rect: None }
    }
}

/// Whether `ratio` names a crop size: `manual`, `current`, or `W:H` with both sides finite and
/// in `(0, 10000]`.
pub fn is_ratio(ratio: &str) -> bool {
    if ratio == "manual" || ratio == "current" {
        return true;
    }
    let side = |s: &str| s.parse::<f32>().ok().is_some_and(|v| v.is_finite() && v > 0.0 && v <= 10_000.0);
    ratio.chars().count() <= MAX_RATIO && ratio.split_once(':').is_some_and(|(w, h)| side(w) && side(h))
}

/// The Straighten tool's draft: the angle in degrees, `−STRAIGHTEN_MAX ..= STRAIGHTEN_MAX`
/// (`editing.md` §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StraightenDraft {
    pub angle: f32,
}

pub const STRAIGHTEN_MAX: f32 = 10.0;

/// One Retouch patch: the blemish's centre, where its texture comes from (both normalised to the
/// photo in the stored frame) and its radius (a fraction of the photo's longer side).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Patch {
    pub target: [f32; 2],
    pub source: [f32; 2],
    pub radius: f32,
}

/// The most patches one Retouch session holds.
pub const MAX_PATCHES: usize = 1000;
/// The brush radius range and default, as a fraction of the photo's longer side.
pub const BRUSH_MIN: f32 = 0.002;
pub const BRUSH_MAX: f32 = 0.25;
pub const BRUSH_DEFAULT: f32 = 0.03;

/// The Retouch tool's draft (`editing.md` §6.4): the patches made, how many of them are in effect
/// (Undo Patch lowers it, Redo Patch raises it; a new patch drops those past it) and the brush.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetouchDraft {
    pub patches: Vec<Patch>,
    pub position: usize,
    pub brush: f32,
}

impl Default for RetouchDraft {
    fn default() -> RetouchDraft {
        RetouchDraft { patches: Vec::new(), position: 0, brush: BRUSH_DEFAULT }
    }
}

/// A Tuning session's values (`editing.md` §7): Fill Light 0 … 1, Highlights and Shadows 0 … 0.5,
/// Color Temperature −1 … 1, and the neutral colour picked (sRGB, each 0 … 1; `null` for
/// none). They are the parameters of the one `tuning` operation the session writes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TuningDraft {
    pub fill: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub temperature: f32,
    pub neutral: Option<[f32; 3]>,
}

/// The edit view's state (`editing.md`, `control-protocol.md` §6.5): the photo shown, the tab of the
/// edit panel, the open tool, the compare mode, the zoom, whether the edit panel is shown, and
/// each tool's draft while it is open (nothing is applied before the tool's Apply). Starts fresh;
/// nothing here is persisted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditState {
    /// The photo shown; `null` outside the edit view.
    pub photo: Option<PhotoId>,
    pub tab: EditTab,
    pub tool: Option<ToolKind>,
    pub compare: Compare,
    pub zoom: Zoom,
    /// The edit panel is shown (Ctrl+9, View ▸ Show Edit Controls).
    pub panel_visible: bool,
    #[serde(default)]
    pub crop: Option<CropDraft>,
    #[serde(default)]
    pub straighten: Option<StraightenDraft>,
    #[serde(default)]
    pub retouch: Option<RetouchDraft>,
    #[serde(default)]
    pub tuning: Option<TuningDraft>,
}

impl Default for EditState {
    fn default() -> EditState {
        EditState {
            photo: None,
            tab: EditTab::Basic,
            tool: None,
            compare: Compare::One,
            zoom: Zoom::Fit,
            panel_visible: true,
            crop: None,
            straighten: None,
            retouch: None,
            tuning: None,
        }
    }
}

/// Whether `v` is a finite number in `min ..= max`.
fn within(v: f32, min: f32, max: f32) -> bool {
    v.is_finite() && (min..=max).contains(&v)
}

impl EditState {
    /// Drops every draft and closes the tool: what Cancel and leaving the edit view do.
    pub fn close_tools(&mut self) {
        self.tool = None;
        self.crop = None;
        self.straighten = None;
        self.retouch = None;
        self.tuning = None;
    }

    /// Why this edit state cannot be used, if it cannot: a number out of its range, a ratio that is
    /// not one, too many patches, a tool open with no photo. What `ui.set` refuses (`bad_params`).
    pub fn check(&self) -> Result<(), String> {
        if let Zoom::Scale(v) = self.zoom
            && !within(v, ZOOM_MIN, ZOOM_MAX)
        {
            return Err(format!("`edit.zoom` is \"fit\" or {ZOOM_MIN} … {ZOOM_MAX}, not {v}"));
        }
        if self.tool.is_some() && self.photo.is_none() {
            return Err("`edit.tool` needs `edit.photo`".into());
        }
        if let Some(c) = &self.crop {
            if !is_ratio(&c.ratio) {
                return Err(format!("`edit.crop.ratio` is \"manual\", \"current\" or \"W:H\" (at most {MAX_RATIO} characters)"));
            }
            if let Some([x, y, w, h]) = c.rect {
                let unit = [x, y, w, h].iter().all(|v| within(*v, 0.0, 1.0));
                if !(unit && w > 0.0 && h > 0.0 && x + w <= 1.0 + 1e-6 && y + h <= 1.0 + 1e-6) {
                    return Err("`edit.crop.rect` is [x, y, w, h] inside the photo (0 … 1, w and h above 0)".into());
                }
            }
        }
        if let Some(s) = &self.straighten
            && !within(s.angle, -STRAIGHTEN_MAX, STRAIGHTEN_MAX)
        {
            return Err(format!("`edit.straighten.angle` is −{STRAIGHTEN_MAX} … {STRAIGHTEN_MAX} degrees"));
        }
        if let Some(r) = &self.retouch {
            if r.patches.len() > MAX_PATCHES {
                return Err(format!("`edit.retouch.patches` holds at most {MAX_PATCHES} patches"));
            }
            if r.position > r.patches.len() {
                return Err("`edit.retouch.position` is 0 … the number of patches".into());
            }
            if !within(r.brush, BRUSH_MIN, BRUSH_MAX) {
                return Err(format!("`edit.retouch.brush` is {BRUSH_MIN} … {BRUSH_MAX}"));
            }
            let point = |p: [f32; 2]| p.iter().all(|v| within(*v, 0.0, 1.0));
            if r.patches.iter().any(|p| !point(p.target) || !point(p.source) || !within(p.radius, BRUSH_MIN, BRUSH_MAX)) {
                return Err(format!("each patch's `target` and `source` are inside the photo (0 … 1) and its `radius` is {BRUSH_MIN} … {BRUSH_MAX}"));
            }
        }
        if let Some(t) = &self.tuning {
            let ok = within(t.fill, 0.0, 1.0)
                && within(t.highlights, 0.0, 0.5)
                && within(t.shadows, 0.0, 0.5)
                && within(t.temperature, -1.0, 1.0)
                && t.neutral.is_none_or(|c| c.iter().all(|v| within(*v, 0.0, 1.0)));
            if !ok {
                return Err("`edit.tuning` is fill 0 … 1, highlights and shadows 0 … 0.5, temperature −1 … 1, neutral [r, g, b] 0 … 1 or null".into());
            }
        }
        Ok(())
    }

    /// This state with every number in its range (a non-finite one at its default) and the lists
    /// bounded: what a writer that did not check hands over is made safe before it is drawn.
    pub fn sanitized(mut self) -> EditState {
        if let Zoom::Scale(v) = self.zoom {
            self.zoom = if v.is_finite() { Zoom::Scale(v.clamp(ZOOM_MIN, ZOOM_MAX)) } else { Zoom::Fit };
        }
        if self.photo.is_none() {
            self.close_tools();
        }
        if let Some(c) = &mut self.crop {
            if !is_ratio(&c.ratio) {
                c.ratio = "manual".into();
            }
            c.rect = c.rect.and_then(|[x, y, w, h]| {
                let [x, y] = [x, y].map(|v| clamp_or(v, 0.0, 1.0, 0.0));
                let (w, h) = (clamp_or(w, 0.0, 1.0 - x, 0.0), clamp_or(h, 0.0, 1.0 - y, 0.0));
                (w > 0.0 && h > 0.0).then_some([x, y, w, h])
            });
        }
        if let Some(s) = &mut self.straighten {
            s.angle = clamp_or(s.angle, -STRAIGHTEN_MAX, STRAIGHTEN_MAX, 0.0);
        }
        if let Some(r) = &mut self.retouch {
            r.patches.truncate(MAX_PATCHES);
            for p in &mut r.patches {
                p.target = p.target.map(|v| clamp_or(v, 0.0, 1.0, 0.5));
                p.source = p.source.map(|v| clamp_or(v, 0.0, 1.0, 0.5));
                p.radius = clamp_or(p.radius, BRUSH_MIN, BRUSH_MAX, BRUSH_DEFAULT);
            }
            r.position = r.position.min(r.patches.len());
            r.brush = clamp_or(r.brush, BRUSH_MIN, BRUSH_MAX, BRUSH_DEFAULT);
        }
        if let Some(t) = &mut self.tuning {
            t.fill = clamp_or(t.fill, 0.0, 1.0, 0.0);
            t.highlights = clamp_or(t.highlights, 0.0, 0.5, 0.0);
            t.shadows = clamp_or(t.shadows, 0.0, 0.5, 0.0);
            t.temperature = clamp_or(t.temperature, -1.0, 1.0, 0.0);
            t.neutral = t.neutral.map(|c| c.map(|v| clamp_or(v, 0.0, 1.0, 0.5)));
        }
        self
    }
}

/// The most dialogs open at once (a confirmation over a dialog over a dialog is already rare).
pub const MAX_DIALOGS: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiState {
    pub view: View,
    /// Library-tree width in points as the user set it; the layout clamps it to `TREE_MIN ..= W −
    /// LIGHT_TABLE_MIN` (`panels::layout`).
    pub tree_width: f32,
    /// The open side pane, `None` when closed.
    pub pane: Option<Pane>,
    /// The side pane at its large width (« in its header).
    pub pane_large: bool,
    /// Thumbnail cell edge in points, `THUMB_MIN ..= THUMB_MAX`.
    pub thumb_size: f32,
    /// The folder whose section is selected (folder list and light table), if any.
    pub folder: Option<FolderId>,
    pub selection: Selection,
    /// The window fills the screen (F11).
    pub full_screen: bool,
    /// Flat list or folder tree (View ▸ Folder View, the toolbar's Flat | Tree).
    pub tree_mode: TreeMode,
    /// The folder order (the view-options popup).
    pub folder_sort: FolderSort,
    /// Reverse Order in the view-options popup.
    pub folder_sort_reverse: bool,
    /// The tree groups folded by their disclosure triangle; their sections leave the light table.
    pub collapsed_groups: Vec<Group>,
    /// Tree mode: the folders whose children are folded.
    pub collapsed_folders: Vec<FolderId>,
    /// View ▸ Thumbnail Caption.
    pub caption: Caption,
    /// View ▸ Small Pictures: show folders and photos of small pictures (both sides under 64 px).
    pub small_pictures: bool,
    /// View ▸ Hidden Pictures: show hidden photos (dimmed) and the Hidden Folders group.
    pub hidden_pictures: bool,
    /// The Photo Tray's held photos and own selection.
    pub tray: Tray,
    /// Where the light table is scrolled to.
    pub scroll: ScrollAnchor,
    /// The open dialogs, the last on top (the one that takes input); drawn by `crate::dialogs`.
    #[serde(default)]
    pub dialogs: Vec<Dialog>,
    /// The activity popover (the jobs under the toolbar's activity indicator) is open.
    #[serde(default)]
    pub activity: bool,
    /// The inline caption editor, while one is open (`photo.caption.edit`).
    #[serde(default)]
    pub caption_edit: Option<CaptionEdit>,
    /// The inline description editor of a section header, while one is open.
    #[serde(default)]
    pub description_edit: Option<DescriptionEdit>,
    /// The Properties pane lists every metadata field (its [Show All Metadata]).
    #[serde(default)]
    pub properties_show_all: bool,
    /// The Tags pane's "Add a tag:" text and autocomplete highlight.
    #[serde(default)]
    pub tags_pane: TagsPane,
    /// The About dialog's tab and how its Contributors tab lists people.
    #[serde(default)]
    pub about: About,
    /// The edit view's state: the photo it shows, the tab, the open tool and its draft.
    #[serde(default)]
    pub edit: EditState,
    /// The status line's text as last drawn (`ui-layout.md` §6.1): `"No selection"`, the photo or
    /// the pictures selected, or a transient [`UiState::status`] message. Written by the status line
    /// each frame; `ui.state` reports it as `status`, and `ui.set` cannot change it.
    #[serde(rename = "status")]
    pub status_line: String,
    /// A transient message for the status line: the result of the last action, or why it was
    /// refused. It replaces the line's text for 4 s, then clears itself. Not part of the protocol's
    /// view state (`status` there is [`UiState::status_line`]).
    #[serde(skip)]
    pub status: String,
}

impl UiState {
    /// This state with every number in its range (a non-finite one at its default): what a writer
    /// that did not check (a settings file, a script) hands over is made safe before it is used.
    pub fn sanitized(mut self) -> UiState {
        self.thumb_size = thumb_size(self.thumb_size);
        self.tree_width = tree_width(self.tree_width);
        self.scroll.offset = clamp_or(self.scroll.offset, 0.0, SCROLL_OFFSET_MAX, 0.0);
        dedup(&mut self.collapsed_groups);
        dedup(&mut self.collapsed_folders);
        let mut held = std::mem::take(&mut self.tray.held);
        dedup(&mut held);
        self.tray.set_held(held);
        dedup(&mut self.tray.selected);
        if let Some(e) = &mut self.caption_edit {
            cut(&mut e.draft);
        }
        if let Some(e) = &mut self.description_edit {
            cut(&mut e.draft);
        }
        cut(&mut self.tags_pane.draft);
        self.tags_pane.highlight = self.tags_pane.highlight.filter(|h| *h < MAX_HIGHLIGHT);
        self.edit = std::mem::take(&mut self.edit).sanitized();
        if self.view == View::Edit && self.edit.photo.is_none() {
            self.view = View::Library;
        }
        self
    }

    /// Why the M7 fields of this state cannot be used, if they cannot ([`EditState::check`]), or
    /// the edit view is asked for with no photo. What `ui.set` refuses (`bad_params`).
    pub fn check_m7(&self) -> Result<(), String> {
        if self.view == View::Edit && self.edit.photo.is_none() {
            return Err("the edit view needs `edit.photo` (or run view.edit)".into());
        }
        self.edit.check()
    }

    /// Why the M6 fields of this state cannot be used, if they cannot: a draft longer than
    /// [`MAX_DRAFT`] characters, a highlight past [`MAX_HIGHLIGHT`]. What `ui.set` refuses
    /// (`bad_params`) before it changes anything.
    pub fn check_m6(&self) -> Result<(), String> {
        let long = |name: &str, text: &str| {
            if text.chars().count() > MAX_DRAFT { Err(format!("`{name}` is at most {MAX_DRAFT} characters")) } else { Ok(()) }
        };
        if let Some(e) = &self.caption_edit {
            long("captionEdit.draft", &e.draft)?;
        }
        if let Some(e) = &self.description_edit {
            long("descriptionEdit.draft", &e.draft)?;
        }
        long("tagsPane.draft", &self.tags_pane.draft)?;
        match self.tags_pane.highlight {
            Some(h) if h >= MAX_HIGHLIGHT => Err(format!("`tagsPane.highlight` is 0 … {}, or null", MAX_HIGHLIGHT - 1)),
            _ => Ok(()),
        }
    }
}

/// Cuts `text` to [`MAX_DRAFT`] characters.
fn cut(text: &mut String) {
    if let Some((at, _)) = text.char_indices().nth(MAX_DRAFT) {
        text.truncate(at);
    }
}

/// Drops repeated entries, keeping the first of each, in one pass (lists are bounded by
/// [`MAX_IDS`]).
fn dedup<T: Eq + std::hash::Hash + Copy>(v: &mut Vec<T>) {
    let mut seen: HashSet<T> = HashSet::with_capacity(v.len());
    v.retain(|x| seen.insert(*x));
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            view: View::Library,
            tree_width: TREE_DEFAULT,
            pane: None,
            pane_large: false,
            thumb_size: THUMB_DEFAULT,
            folder: None,
            selection: Selection::default(),
            full_screen: false,
            tree_mode: TreeMode::Flat,
            folder_sort: FolderSort::Date,
            folder_sort_reverse: false,
            collapsed_groups: Vec::new(),
            collapsed_folders: Vec::new(),
            caption: Caption::None,
            small_pictures: false,
            hidden_pictures: false,
            tray: Tray::default(),
            scroll: ScrollAnchor::default(),
            dialogs: Vec::new(),
            activity: false,
            caption_edit: None,
            description_edit: None,
            properties_show_all: false,
            tags_pane: TagsPane::default(),
            about: About::default(),
            edit: EditState::default(),
            status_line: String::new(),
            status: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_round_trips_as_camel_case_json_and_refuses_unknown_fields() {
        let mut s = UiState::default();
        s.selection.ids = vec![PhotoId::new(3), PhotoId::new(1)];
        s.selection.anchor = Some(PhotoId::new(1));
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["thumbSize"], 144.0);
        assert_eq!(v["view"], "library");
        assert_eq!(
            (&v["treeWidth"], &v["pane"], &v["paneLarge"], &v["fullScreen"]),
            (&240.0.into(), &serde_json::Value::Null, &false.into(), &false.into())
        );
        assert_eq!(v["selection"], serde_json::json!({ "ids": [3, 1], "anchor": 1, "section": null, "whole": false }));
        assert_eq!(serde_json::from_value::<UiState>(v).unwrap(), s);
        assert!(serde_json::from_value::<UiState>(serde_json::json!({ "view": "library", "zoom": 2 })).is_err());
    }

    /// `contains` answers from an index, so asking it for every cell of a 100 000-photo selection
    /// is linear, not quadratic; the index never outlives the ids it was built from.
    #[test]
    fn selection_lookups_are_constant_time_and_never_stale() {
        let n = 100_000u64;
        let all: Vec<PhotoId> = (1..=n).map(PhotoId::new).collect();
        let sel = Selection::new(all.clone(), Some(PhotoId::new(1)), None, false);
        let started = std::time::Instant::now();
        // 200 000 lookups: a linear search would take about 10^10 comparisons.
        let hits = (1..=2 * n).filter(|&k| sel.contains(PhotoId::new(k))).count();
        assert_eq!(hits as u64, n);
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
        // The serialised shape is unchanged: no index in it, and a round trip compares equal.
        let v = serde_json::to_value(&sel).unwrap();
        assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), ["anchor", "ids", "section", "whole"]);
        let back: Selection = serde_json::from_value(v).unwrap();
        assert_eq!(back, sel);
        assert!(back.contains(PhotoId::new(n)) && !back.contains(PhotoId::new(n + 1)));
        // A clone, a new set of ids, and ids written in place all answer for their own ids.
        let mut other = sel.clone();
        assert!(other.contains(PhotoId::new(5)));
        other.set_ids((n + 1..=n + 50).map(PhotoId::new).collect());
        assert!(!other.contains(PhotoId::new(5)) && other.contains(PhotoId::new(n + 50)));
        let mut written = sel.clone();
        assert!(written.contains(PhotoId::new(7)));
        written.ids = (n + 1..=2 * n).map(PhotoId::new).collect();
        assert!(!written.contains(PhotoId::new(7)) && written.contains(PhotoId::new(2 * n)), "a stale index is not trusted");
        written.ids.truncate(10);
        assert!(written.contains(PhotoId::new(n + 10)) && !written.contains(PhotoId::new(n + 11)));
        assert!(!Selection::default().contains(PhotoId::new(1)));
    }

    #[test]
    fn status_is_the_line_as_drawn_and_the_transient_message_is_not_serialised() {
        let s = UiState { status_line: "No selection".into(), status: "Select some photos first.".into(), ..UiState::default() };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["status"], "No selection");
        assert!(v.as_object().unwrap().values().all(|f| f != "Select some photos first."), "{v}");
        let back: UiState = serde_json::from_value(v).unwrap();
        assert_eq!((back.status_line.as_str(), back.status.as_str()), ("No selection", ""));
    }

    #[test]
    fn the_tree_and_pane_fields_are_plain_serde_fields() {
        let v = serde_json::json!({ "treeWidth": 300.5, "pane": "people", "paneLarge": true });
        let mut merged = serde_json::to_value(UiState::default()).unwrap();
        merged.as_object_mut().unwrap().extend(v.as_object().unwrap().clone());
        let s: UiState = serde_json::from_value(merged).unwrap();
        assert_eq!((s.tree_width, s.pane, s.pane_large), (300.5, Some(Pane::People), true));
    }

    #[test]
    fn sanitized_puts_every_number_in_its_range() {
        for (bad, thumb, tree) in [
            (f32::NAN, THUMB_DEFAULT, TREE_DEFAULT),
            (f32::INFINITY, THUMB_DEFAULT, TREE_DEFAULT),
            (f32::NEG_INFINITY, THUMB_DEFAULT, TREE_DEFAULT),
            (-1.0, THUMB_MIN, TREE_MIN),
            (1e9, THUMB_MAX, TREE_MAX),
            (f32::MIN_POSITIVE, THUMB_MIN, TREE_MIN),
        ] {
            let s = UiState { thumb_size: bad, tree_width: bad, ..UiState::default() }.sanitized();
            assert_eq!((s.thumb_size, s.tree_width), (thumb, tree), "{bad}");
        }
        let fine = UiState { thumb_size: 200.0, tree_width: 300.0, ..UiState::default() };
        assert_eq!(fine.clone().sanitized(), fine, "numbers in range are kept");
    }

    #[test]
    fn the_side_pane_is_named_as_in_the_protocol() {
        let mut s = UiState { pane: Some(Pane::Properties), pane_large: true, ..UiState::default() };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!((&v["pane"], &v["paneLarge"]), (&"properties".into(), &true.into()));
        for (name, pane) in [("people", Pane::People), ("places", Pane::Places), ("tags", Pane::Tags)] {
            s.pane = Some(pane);
            assert_eq!(serde_json::to_value(&s).unwrap()["pane"], name);
        }
        let mut bad = v.clone();
        bad["pane"] = "attic".into();
        assert!(serde_json::from_value::<UiState>(bad).is_err());
    }

    #[test]
    fn the_m4_fields_have_their_protocol_names_and_defaults() {
        let v = serde_json::to_value(UiState::default()).unwrap();
        let fields = ["treeMode", "folderSort", "folderSortReverse", "caption", "smallPictures", "hiddenPictures"].map(|k| v[k].clone());
        assert_eq!(fields, [serde_json::json!("flat"), "date".into(), false.into(), "none".into(), false.into(), false.into()]);
        assert_eq!((&v["collapsedGroups"], &v["collapsedFolders"]), (&serde_json::json!([]), &serde_json::json!([])));
        assert_eq!(v["tray"], serde_json::json!({ "held": [], "selected": [] }));
        assert_eq!(v["scroll"], serde_json::json!({ "section": null, "offset": 0.0 }));
        let names = |all: &[serde_json::Value]| all.iter().map(|g| g.as_str().unwrap().to_string()).collect::<Vec<_>>();
        let groups: Vec<serde_json::Value> = Group::ALL.iter().map(|g| serde_json::to_value(g).unwrap()).collect();
        assert_eq!(names(&groups), ["albums", "people", "projects", "folders", "other", "hidden"]);
        for (sort, name) in [(FolderSort::Date, "date"), (FolderSort::Recent, "recent"), (FolderSort::Size, "size"), (FolderSort::Name, "name")] {
            assert_eq!(serde_json::to_value(sort).unwrap(), name);
        }
        for (c, name) in [
            (Caption::None, "none"),
            (Caption::Filename, "filename"),
            (Caption::Caption, "caption"),
            (Caption::Tags, "tags"),
            (Caption::Resolution, "resolution"),
            (Caption::Date, "date"),
        ] {
            assert_eq!(serde_json::to_value(c).unwrap(), name);
        }
        assert_eq!(serde_json::to_value(TreeMode::Tree).unwrap(), "tree");
    }

    #[test]
    fn dialogs_are_a_stack_of_kinds_and_drafts_and_the_activity_popover_a_flag() {
        let v = serde_json::to_value(UiState::default()).unwrap();
        assert_eq!((&v["dialogs"], &v["activity"]), (&serde_json::json!([]), &false.into()));
        let mut d = Dialog::new(DialogKind::ClearTray);
        d.command = Some(DialogCommand { id: "tray.clear".into(), params: serde_json::json!({}) });
        let s = UiState { dialogs: vec![Dialog::new(DialogKind::FolderManager), d], activity: true, ..UiState::default() };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(
            v["dialogs"],
            serde_json::json!([{ "kind": "folderManager", "fields": {} }, { "kind": "clearTray", "fields": {}, "command": { "id": "tray.clear", "params": {} } }])
        );
        assert_eq!(serde_json::from_value::<UiState>(v).unwrap(), s);
        for k in DialogKind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), k.name());
            assert_eq!(DialogKind::from_name(k.name()), Some(k));
        }
        assert_eq!(DialogKind::from_name("attic"), None);
        assert!(serde_json::from_value::<Dialog>(serde_json::json!({ "kind": "attic" })).is_err());
        assert!(serde_json::from_value::<Dialog>(serde_json::json!({ "kind": "firstRun", "zzz": 1 })).is_err());
    }

    #[test]
    fn the_m6_fields_have_their_protocol_names_and_defaults() {
        let v = serde_json::to_value(UiState::default()).unwrap();
        assert_eq!(
            (&v["captionEdit"], &v["descriptionEdit"], &v["propertiesShowAll"]),
            (&serde_json::Value::Null, &serde_json::Value::Null, &false.into())
        );
        assert_eq!(v["tagsPane"], serde_json::json!({ "draft": "", "highlight": null }));
        assert_eq!(
            v["about"],
            serde_json::json!({ "tab": "about", "list": "grabBag", "sort": "firstCommit", "ascending": true, "names": "username" })
        );
        let s = UiState {
            caption_edit: Some(CaptionEdit { photo: PhotoId::new(4), draft: "Lisbon".into() }),
            description_edit: Some(DescriptionEdit { section: Section::Folder(FolderId::new(2)), draft: "trams".into() }),
            properties_show_all: true,
            tags_pane: TagsPane { draft: "bea".into(), highlight: Some(1) },
            about: About {
                tab: AboutTab::Models,
                list: CreditsList::Table,
                sort: CreditsSort::LinesDelta,
                ascending: false,
                names: NameMode::RealName,
            },
            ..UiState::default()
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["captionEdit"], serde_json::json!({ "photo": 4, "draft": "Lisbon" }));
        assert_eq!(v["descriptionEdit"], serde_json::json!({ "section": "folder:2", "draft": "trams" }));
        assert_eq!(
            v["about"],
            serde_json::json!({ "tab": "models", "list": "table", "sort": "linesDelta", "ascending": false, "names": "realName" })
        );
        assert_eq!(serde_json::from_value::<UiState>(v).unwrap(), s);
        for k in DialogKind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), k.name());
        }
        let names: Vec<&str> = DialogKind::ALL[5..11].iter().map(|k| k.name()).collect();
        assert_eq!(names, ["folderProperties", "adjustDate", "quickTags", "renameTag", "about", "releaseNotes"]);
    }

    #[test]
    fn drafts_and_highlights_are_bounded() {
        let long = "é".repeat(MAX_DRAFT + 5);
        let s = UiState {
            caption_edit: Some(CaptionEdit { photo: PhotoId::new(1), draft: long.clone() }),
            tags_pane: TagsPane { draft: long.clone(), highlight: Some(MAX_HIGHLIGHT) },
            ..UiState::default()
        };
        assert!(s.check_m6().unwrap_err().contains("captionEdit.draft"));
        let s = s.sanitized();
        assert_eq!(s.caption_edit.as_ref().unwrap().draft.chars().count(), MAX_DRAFT);
        assert_eq!((s.tags_pane.draft.chars().count(), s.tags_pane.highlight), (MAX_DRAFT, None));
        assert!(s.check_m6().is_ok());
        let high = UiState { tags_pane: TagsPane { draft: String::new(), highlight: Some(MAX_HIGHLIGHT) }, ..UiState::default() };
        assert!(high.check_m6().unwrap_err().contains("highlight"));
        let fine = UiState { tags_pane: TagsPane { draft: "x".repeat(MAX_DRAFT), highlight: Some(MAX_HIGHLIGHT - 1) }, ..UiState::default() };
        assert_eq!(fine.check_m6(), Ok(()));
        assert_eq!(fine.clone().sanitized(), fine);
    }

    #[test]
    fn the_edit_state_has_its_protocol_names_and_checks_its_ranges() {
        let v = serde_json::to_value(UiState::default()).unwrap();
        assert_eq!(
            v["edit"],
            serde_json::json!({ "photo": null, "tab": "basic", "tool": null, "compare": "one", "zoom": "fit", "panelVisible": true,
                                "crop": null, "straighten": null, "retouch": null, "tuning": null })
        );
        for k in ToolKind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), k.name());
        }
        let tabs =
            [EditTab::Basic, EditTab::Tuning, EditTab::Effects, EditTab::Effects2, EditTab::Effects3].map(|t| serde_json::to_value(t).unwrap());
        assert_eq!(tabs, ["basic", "tuning", "effects", "effects2", "effects3"].map(serde_json::Value::from));
        assert_eq!(serde_json::to_value(Zoom::Scale(2.0)).unwrap(), 2.0);
        assert_eq!(serde_json::from_value::<Zoom>("fit".into()).unwrap(), Zoom::Fit);
        for bad in [serde_json::json!("wide"), serde_json::json!(null), serde_json::json!([1])] {
            assert!(serde_json::from_value::<Zoom>(bad.clone()).is_err(), "{bad}");
        }
        for (ratio, ok) in
            [("manual", true), ("current", true), ("4:3", true), ("8.5:11", true), ("0:1", false), ("4:", false), ("x", false), ("1:NaN", false)]
        {
            assert_eq!(is_ratio(ratio), ok, "{ratio}");
        }
        assert!(!is_ratio(&format!("1:{}", "1".repeat(70))));

        let photo = Some(PhotoId::new(1));
        let mut e = EditState { photo, tool: Some(ToolKind::Crop), crop: Some(CropDraft::default()), ..EditState::default() };
        assert_eq!(e.check(), Ok(()));
        let s = UiState { view: View::Edit, edit: e.clone(), ..UiState::default() };
        assert_eq!(s.check_m7(), Ok(()));
        assert!(UiState { view: View::Edit, ..UiState::default() }.check_m7().is_err(), "the edit view needs a photo");
        e.zoom = Zoom::Scale(f32::NAN);
        assert!(e.check().is_err());
        let e = e.sanitized();
        assert_eq!(e.zoom, Zoom::Fit);
        let hostile = EditState {
            photo,
            zoom: Zoom::Scale(1e9),
            crop: Some(CropDraft { ratio: "bad".into(), rect: Some([0.75, f32::NAN, 0.5, 2.0]) }),
            straighten: Some(StraightenDraft { angle: f32::INFINITY }),
            retouch: Some(RetouchDraft {
                patches: vec![Patch { target: [f32::NAN, 2.0], source: [-1.0, 0.5], radius: 9.0 }; MAX_PATCHES + 5],
                position: usize::MAX,
                brush: 0.0,
            }),
            tuning: Some(TuningDraft { fill: 3.0, highlights: -1.0, shadows: f32::NAN, temperature: -5.0, neutral: Some([2.0, -1.0, f32::NAN]) }),
            ..EditState::default()
        };
        assert!(hostile.check().is_err());
        let safe = hostile.sanitized();
        assert_eq!(safe.check(), Ok(()), "{safe:?}");
        assert_eq!(safe.zoom, Zoom::Scale(ZOOM_MAX));
        assert_eq!(safe.crop.as_ref().map(|c| (c.ratio.as_str(), c.rect)), Some(("manual", Some([0.75, 0.0, 0.25, 1.0]))));
        let r = safe.retouch.as_ref().unwrap();
        assert_eq!((r.patches.len(), r.position, r.brush), (MAX_PATCHES, MAX_PATCHES, BRUSH_MIN));
        // A tool without a photo is closed; the edit view without one falls back to the library.
        let s =
            UiState { view: View::Edit, edit: EditState { tool: Some(ToolKind::Retouch), ..EditState::default() }, ..UiState::default() }.sanitized();
        assert_eq!((s.view, s.edit.tool), (View::Library, None));
    }

    #[test]
    fn a_section_is_written_as_its_widget_id() {
        let s = Section::Folder(FolderId::new(12));
        assert_eq!(s.to_string(), "folder:12");
        assert_eq!(serde_json::to_value(s).unwrap(), "folder:12");
        assert_eq!(serde_json::from_value::<Section>("folder:12".into()).unwrap(), s);
        for bad in ["", "folder:", "folder:-1", "folder:1.5", "folder: 1", "album:a91c0e", "12", "folder:99999999999999999999999", ":12"] {
            assert!(bad.parse::<Section>().is_err(), "{bad:?}");
        }
        assert!(serde_json::from_value::<Section>(serde_json::json!(12)).is_err());
    }

    #[test]
    fn sanitized_drops_repeats_and_keeps_the_scroll_offset_in_range() {
        let s = UiState {
            collapsed_groups: vec![Group::Folders, Group::Albums, Group::Folders],
            collapsed_folders: vec![FolderId::new(2), FolderId::new(2)],
            tray: Tray {
                held: vec![PhotoId::new(1), PhotoId::new(1), PhotoId::new(2)],
                selected: vec![PhotoId::new(2), PhotoId::new(2)],
                ..Tray::default()
            },
            scroll: ScrollAnchor { section: None, offset: f32::NAN },
            ..UiState::default()
        }
        .sanitized();
        assert_eq!(s.collapsed_groups, [Group::Folders, Group::Albums]);
        assert_eq!(s.collapsed_folders, [FolderId::new(2)]);
        assert_eq!(
            (s.tray.held.as_slice(), s.tray.selected.as_slice()),
            ([PhotoId::new(1), PhotoId::new(2)].as_slice(), [PhotoId::new(2)].as_slice())
        );
        assert_eq!(s.scroll.offset, 0.0);
        for (bad, kept) in [(-5.0, 0.0), (f32::INFINITY, 0.0), (2e9, SCROLL_OFFSET_MAX), (120.5, 120.5)] {
            let s = UiState { scroll: ScrollAnchor { section: None, offset: bad }, ..UiState::default() }.sanitized();
            assert_eq!(s.scroll.offset, kept, "{bad}");
        }
    }
}
