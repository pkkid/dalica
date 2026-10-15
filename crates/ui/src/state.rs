//! `UiState`: the serialisable view state (`architecture.md` §10, `control-protocol.md` §6.5).
//! Plain data, so `ui.state` and `ui.set` can read and drive it and tests can assert on it. Library
//! state never lives here: it is the engine's.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use dalica_core::{AlbumId, FolderId, PhotoId};
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

/// A user album's id ([`AlbumId`]: `a` and 4 to 16 lowercase hex digits) in a form that is `Copy`,
/// so a [`Section`] stays a plain value: the digits as a number and how many there are (leading
/// zeros count: `a0012` and `a12` are different albums). [`AlbumKey::id`] gives the id back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AlbumKey {
    value: u64,
    digits: u8,
}

impl AlbumKey {
    /// The key of `id`.
    pub fn of(id: &AlbumId) -> AlbumKey {
        let hex = id.as_str().get(1..).unwrap_or("");
        // `AlbumId::parse` admits 4 to 16 lowercase hex digits only, so they fit a u64.
        let value = u64::from_str_radix(hex, 16).unwrap_or(0);
        AlbumKey { value, digits: u8::try_from(hex.len()).unwrap_or(16) }
    }

    /// The album id it stands for. Every key is made from a valid id, so this is always `Some`.
    pub fn id(self) -> Option<AlbumId> {
        AlbumId::parse(&self.to_string()).ok()
    }
}

impl fmt::Display for AlbumKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a{:0width$x}", self.value, width = usize::from(self.digits))
    }
}

/// The album sections (`control-protocol.md` §5.4): a user album, Starred Photos, Recently
/// Updated, and the temporary search-results album.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlbumSection {
    User(AlbumKey),
    Starred,
    Recent,
    Search,
}

/// A section of the light table: the photos of one folder or one album. In the protocol it is
/// written as in its widget id, `"folder:12"`, `"album:a91c0e"`, `"album:starred"`,
/// `"album:recent"`, `"album:search"` (`control-protocol.md` §5.4, §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Folder(FolderId),
    Album(AlbumSection),
}

impl Section {
    /// The folder of a folder section; `None` for an album.
    pub fn folder(self) -> Option<FolderId> {
        match self {
            Section::Folder(f) => Some(f),
            Section::Album(_) => None,
        }
    }

    /// Whether this is an album section (a user album or an automatic one).
    pub fn is_album(self) -> bool {
        matches!(self, Section::Album(_))
    }

    /// The user album of an album section; `None` for a folder and the automatic albums.
    pub fn user_album(self) -> Option<AlbumId> {
        match self {
            Section::Album(AlbumSection::User(k)) => k.id(),
            _ => None,
        }
    }
}

impl fmt::Display for Section {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Section::Folder(id) => write!(f, "folder:{id}"),
            Section::Album(AlbumSection::User(k)) => write!(f, "album:{k}"),
            Section::Album(AlbumSection::Starred) => f.write_str("album:starred"),
            Section::Album(AlbumSection::Recent) => f.write_str("album:recent"),
            Section::Album(AlbumSection::Search) => f.write_str("album:search"),
        }
    }
}

impl FromStr for Section {
    type Err = String;

    fn from_str(s: &str) -> Result<Section, String> {
        let bad = || {
            format!(
                "`{}` is not a section; sections are written `folder:<FolderId>`, `album:<AlbumId>`, `album:starred`, `album:recent` or `album:search`",
                s.chars().take(40).collect::<String>()
            )
        };
        match s.split_once(':') {
            Some(("folder", id)) => id.parse::<FolderId>().map(Section::Folder).map_err(|_| bad()),
            Some(("album", "starred")) => Ok(Section::Album(AlbumSection::Starred)),
            Some(("album", "recent")) => Ok(Section::Album(AlbumSection::Recent)),
            Some(("album", "search")) => Ok(Section::Album(AlbumSection::Search)),
            Some(("album", id)) => AlbumId::parse(id).map(|a| Section::Album(AlbumSection::User(AlbumKey::of(&a)))).map_err(|_| bad()),
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
    /// The first-time explanation of Save (`photo.save`, `data-model.md` §6); its draft is
    /// [`UiState::save_explain`].
    SaveExplain,
    /// "Revert to the original?" with [Revert] [Undo Save] [Cancel] (`photo.revert`); its draft is
    /// [`UiState::revert`].
    Revert,
    /// File ▸ Export Pictures to Folder… (`output.export`, `creating.md` §2); its draft is
    /// [`UiState::export`].
    Export,
    /// Album Properties, for a new album (`album.new`) or an album section
    /// (`section.properties`); its draft is [`UiState::album_properties`].
    AlbumProperties,
    /// "Remove the %d selected pictures from this album?" (`album.remove`, `organizing.md` §4.3).
    RemoveFromAlbum,
    /// "Delete the album …? The pictures in it are not deleted." (`album.delete`).
    DeleteAlbum,
    /// The first-time move question with "Don't ask again" (`photo.moveTo`, `organizing.md` §9);
    /// its draft is [`UiState::move_files`].
    MoveFiles,
    /// "Copy %d files to %s?" (asked by the UI before `photo.copyTo`); its draft is
    /// [`UiState::copy_files`].
    CopyFiles,
    /// "The folder already has files with the same names." [Rename Duplicates] [Skip Duplicates]
    /// [Cancel]; its draft is [`UiState::collision`].
    Collision,
    /// The new-folder dialog of Move to New Folder… and Split Folder Here…; its draft is
    /// [`UiState::new_folder`].
    NewFolder,
    /// Rename Files (File ▸ Rename…, F2); its draft is [`UiState::rename_files`].
    RenameFiles,
    /// "Move the selected pictures to the trash?" (`photo.delete`); its draft is
    /// [`UiState::delete_photos`].
    DeletePhotos,
    /// "Move the folder … and everything in it … to the trash?" (`folder.delete`); its draft is
    /// [`UiState::delete_folder`].
    DeleteFolder,
    /// Tools ▸ Options… (`app.preferences`) and Help ▸ Keyboard Shortcuts… (`app.shortcuts`); its
    /// draft is [`UiState::options`].
    Options,
}

impl DialogKind {
    pub const ALL: [DialogKind; 26] = [
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
        DialogKind::SaveExplain,
        DialogKind::Revert,
        DialogKind::Export,
        DialogKind::AlbumProperties,
        DialogKind::RemoveFromAlbum,
        DialogKind::DeleteAlbum,
        DialogKind::MoveFiles,
        DialogKind::CopyFiles,
        DialogKind::Collision,
        DialogKind::NewFolder,
        DialogKind::RenameFiles,
        DialogKind::DeletePhotos,
        DialogKind::DeleteFolder,
        DialogKind::Options,
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
            DialogKind::SaveExplain => "saveExplain",
            DialogKind::Revert => "revert",
            DialogKind::Export => "export",
            DialogKind::AlbumProperties => "albumProperties",
            DialogKind::RemoveFromAlbum => "removeFromAlbum",
            DialogKind::DeleteAlbum => "deleteAlbum",
            DialogKind::MoveFiles => "moveFiles",
            DialogKind::CopyFiles => "copyFiles",
            DialogKind::Collision => "collision",
            DialogKind::NewFolder => "newFolder",
            DialogKind::RenameFiles => "renameFiles",
            DialogKind::DeletePhotos => "deletePhotos",
            DialogKind::DeleteFolder => "deleteFolder",
            DialogKind::Options => "options",
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

/// The Export dialog's image-quality presets (`creating.md` §2): Automatic keeps the original's
/// quality, Normal is 85, Maximum 95, Minimum 60, Custom the dialog's own value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Quality {
    #[default]
    Automatic,
    Normal,
    Maximum,
    Minimum,
    Custom,
}

impl Quality {
    pub const ALL: [Quality; 5] = [Quality::Automatic, Quality::Normal, Quality::Maximum, Quality::Minimum, Quality::Custom];

    /// The preset as the protocol writes it (`automatic`), the parameter `export.run` takes.
    pub fn name(self) -> &'static str {
        match self {
            Quality::Automatic => "automatic",
            Quality::Normal => "normal",
            Quality::Maximum => "maximum",
            Quality::Minimum => "minimum",
            Quality::Custom => "custom",
        }
    }
}

/// How the Export dialog exports videos (`creating.md` §2): their first frame as a photo, or the
/// whole video unresized. The row stays disabled until videos are decoded (M15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Videos {
    #[default]
    FirstFrame,
    FullVideo,
}

/// The Export dialog's size range (the slider's ends, `creating.md` §2) and its default, in pixels
/// of the longest side.
pub const RESIZE_MIN: u32 = 100;
pub const RESIZE_MAX: u32 = 4000;
pub const RESIZE_DEFAULT: u32 = 1600;
/// The longest text the Export dialog's size field keeps, in characters.
pub const MAX_RESIZE_TEXT: usize = 16;
/// The custom JPEG quality's range and default.
pub const CUSTOM_MIN: u8 = 1;
pub const CUSTOM_MAX: u8 = 100;
pub const CUSTOM_DEFAULT: u8 = 75;
/// The longest export location kept, in bytes (the control channel's longest path).
pub const MAX_LOCATION: usize = 4096;

/// The Export dialog's draft (`creating.md` §2, `control-protocol.md` §6.5): what it exports (the
/// photos named, or a section from its header) and every control's value. Nothing is applied
/// before its Export button, which runs `export.run`. `resize` is the size field's text as typed,
/// so a draft can hold what the field shows ("abc", "0"); [`ExportDraft::pixels`] says whether it is
/// a size Export takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ExportDraft {
    /// The photos exported (the tray's, when opened from the menu or the bottom bar).
    pub ids: Vec<PhotoId>,
    /// The section exported, when opened from a section header.
    pub section: Option<Section>,
    /// Export location: the folder the export folder is made in.
    pub location: String,
    /// Name of exported folder.
    pub folder_name: String,
    /// Add numbers to file names to keep the order.
    pub numbers: bool,
    /// Use original size (else Resize to `resize`).
    pub original_size: bool,
    /// The Resize to field's text: digits for a size of the longest side.
    pub resize: String,
    pub quality: Quality,
    /// The Custom preset's quality, `CUSTOM_MIN ..= CUSTOM_MAX`.
    pub custom: u8,
    pub videos: Videos,
    /// Add watermark, and its text.
    pub watermark: bool,
    pub watermark_text: String,
}

impl Default for ExportDraft {
    fn default() -> ExportDraft {
        ExportDraft {
            ids: Vec::new(),
            section: None,
            location: String::new(),
            folder_name: String::new(),
            numbers: false,
            original_size: false,
            resize: RESIZE_DEFAULT.to_string(),
            quality: Quality::Automatic,
            custom: CUSTOM_DEFAULT,
            videos: Videos::FirstFrame,
            watermark: false,
            watermark_text: String::new(),
        }
    }
}

impl ExportDraft {
    /// The Resize to field as a size Export takes: digits only, `RESIZE_MIN ..= RESIZE_MAX`.
    pub fn pixels(&self) -> Option<u32> {
        let text = self.resize.trim();
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        text.parse::<u32>().ok().filter(|v| (RESIZE_MIN..=RESIZE_MAX).contains(v))
    }

    /// Why Export cannot run with this draft, if it cannot (the Export button is disabled then):
    /// nothing to export, no location or folder name, a size that is not one, a watermark with no
    /// text.
    pub fn ready(&self) -> Result<(), String> {
        if self.ids.is_empty() && self.section.is_none() {
            return Err("There is nothing to export.".into());
        }
        if self.location.trim().is_empty() {
            return Err("Choose where to export to.".into());
        }
        if self.folder_name.trim().is_empty() {
            return Err("Name the exported folder.".into());
        }
        if !self.original_size && self.pixels().is_none() {
            return Err(format!("Resize to a size of {RESIZE_MIN} … {RESIZE_MAX} pixels."));
        }
        if self.watermark && self.watermark_text.trim().is_empty() {
            return Err("Type the watermark's text.".into());
        }
        Ok(())
    }

    /// Why this draft cannot be held, if it cannot: too many photos, a text too long, a custom
    /// quality out of range. What `ui.set` refuses (`bad_params`).
    pub fn check(&self) -> Result<(), String> {
        if self.ids.len() > MAX_IDS {
            return Err(format!("`export.ids` holds at most {MAX_IDS} photos"));
        }
        if self.location.len() > MAX_LOCATION {
            return Err(format!("`export.location` is at most {MAX_LOCATION} bytes"));
        }
        for (name, text) in [("folderName", &self.folder_name), ("watermarkText", &self.watermark_text)] {
            if text.chars().count() > MAX_DRAFT {
                return Err(format!("`export.{name}` is at most {MAX_DRAFT} characters"));
            }
        }
        if self.resize.chars().count() > MAX_RESIZE_TEXT {
            return Err(format!("`export.resize` is the size field's text, at most {MAX_RESIZE_TEXT} characters"));
        }
        if !(CUSTOM_MIN..=CUSTOM_MAX).contains(&self.custom) {
            return Err(format!("`export.custom` is {CUSTOM_MIN} … {CUSTOM_MAX}, not {}", self.custom));
        }
        Ok(())
    }

    /// This draft with its texts cut to their limits, repeated ids dropped and the custom quality
    /// in range.
    pub fn sanitized(mut self) -> ExportDraft {
        self.ids.truncate(MAX_IDS);
        dedup(&mut self.ids);
        cut_bytes(&mut self.location, MAX_LOCATION);
        cut(&mut self.folder_name);
        cut(&mut self.watermark_text);
        if let Some((at, _)) = self.resize.char_indices().nth(MAX_RESIZE_TEXT) {
            self.resize.truncate(at);
        }
        self.custom = self.custom.clamp(CUSTOM_MIN, CUSTOM_MAX);
        self
    }
}

/// The first-time explanation of Save while it is open: its "Don't show this again" checkbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveExplain {
    pub dont_show: bool,
}

/// The Revert question while it is open: the photos it would revert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevertQuestion {
    pub photos: Vec<PhotoId>,
}

/// The export progress window ("Exporting 12 of 48…", `creating.md` §2): the export job it follows
/// and whether it is shown (the job goes on in the background while it is hidden). It is not a
/// dialog of the stack: the window under it keeps taking input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportProgress {
    pub job: dalica_engine::JobId,
    pub shown: bool,
}

/// The search toggles of the toolbar (`organizing.md` §8.2), combinable with each other and with
/// the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    pub starred: bool,
    pub faces: bool,
    pub videos: bool,
    pub geotagged: bool,
}

impl Filters {
    /// Whether any toggle is on.
    pub fn any(&self) -> bool {
        self.starred || self.faces || self.videos || self.geotagged
    }
}

/// The search box, the filter toggles and the age slider (`organizing.md` §8, `ui-layout.md` §9):
/// what is typed, the highlighted row of the autocomplete list (`None`: none), the toggles, the age
/// slider's position (0 = everything … [`AGE_MAX`] = only the newest; the slider's own logarithmic
/// map turns it into an age), and the light table's scroll anchor saved when search mode started
/// (restored when it ends). `duplicates` mirrors the engine's Find Duplicates mode (no text,
/// filters or age, yet the banner shows); it is not view state of its own, so `ui.state` leaves it
/// out and the panels set it from the session every frame (`crate::commands::search::sync`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct SearchState {
    pub text: String,
    pub highlight: Option<usize>,
    pub filters: Filters,
    pub age: f32,
    pub saved_scroll: Option<ScrollAnchor>,
    #[serde(skip)]
    pub duplicates: bool,
}

/// The age slider's far end; 0 is everything.
pub const AGE_MAX: f32 = 1.0;

impl SearchState {
    /// Search mode: some text, a filter or an age is set, or Find Duplicates' groups are shown (the
    /// light table shows the matches under the filter banner).
    pub fn active(&self) -> bool {
        !self.text.trim().is_empty() || self.filters.any() || self.age > 0.0 || self.duplicates
    }
}

/// What Edit ▸ Cut and Edit ▸ Copy keep for Paste (`menus-and-shortcuts.md` §1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipOp {
    Cut,
    Copy,
}

/// The photos cut or copied, waiting for Paste into a folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Clipboard {
    pub op: ClipOp,
    pub ids: Vec<PhotoId>,
}

/// What a drop would do where the pointer is (`ui-layout.md` §5.5): move or copy to a folder, add
/// to an album, reorder within an album, hold in the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DragOp {
    Move,
    Copy,
    Add,
    Reorder,
    Hold,
}

/// A drag of photos in progress (`ui.inspect` → `drag`, `control-protocol.md` §6.6): where it
/// started (a widget id, `thumb:12`), the photos it carries, the drop target under the pointer (a
/// widget id, `null` for none), what a drop there would do, and the drag pill's text. Not part of
/// `ui.state`: a drag lives only while the button is held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Drag {
    pub from: String,
    pub ids: Vec<PhotoId>,
    pub target: Option<String>,
    pub op: Option<DragOp>,
    pub pill: String,
    /// What the target is (`crate::drag`): what a release there runs. Not reported.
    #[serde(skip)]
    pub at: Option<DropAt>,
}

/// What a drag of photos can be dropped on (`ui-layout.md` §4, §5.5, §6.2): a library folder's
/// tree row, a user album's tree row, the Photo Tray, or the gap before or after a cell of a user
/// album's section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropAt {
    Folder(FolderId),
    Album(AlbumKey),
    Tray,
    Gap { album: AlbumKey, anchor: PhotoId, after: bool },
}

/// The longest date text a dialog's date field keeps, in characters (`YYYY-MM-DD` as typed).
pub const MAX_DATE_TEXT: usize = 64;

/// Album Properties' draft (`organizing.md` §4.2): the album edited (`null` for a new one, which
/// is made with `ids`), its name, date as typed with "Use the date of the newest photo", place and
/// description. OK runs `album.create` or `album.info.set`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct AlbumDraft {
    pub album: Option<AlbumId>,
    pub ids: Vec<PhotoId>,
    pub name: String,
    pub date: String,
    pub use_newest: bool,
    pub place: String,
    pub description: String,
}

/// The name a new album starts with, selected.
pub const NEW_ALBUM_NAME: &str = "New Album";

impl AlbumDraft {
    /// This draft within its limits: the photos cut to [`MAX_IDS`] without repeats, the texts to
    /// [`MAX_DRAFT`] characters, the date to [`MAX_DATE_TEXT`].
    pub fn sanitized(mut self) -> AlbumDraft {
        self.ids.truncate(MAX_IDS);
        dedup(&mut self.ids);
        for t in [&mut self.name, &mut self.place, &mut self.description] {
            cut(t);
        }
        cut_chars(&mut self.date, MAX_DATE_TEXT);
        self
    }
}

impl Default for AlbumDraft {
    fn default() -> AlbumDraft {
        AlbumDraft {
            album: None,
            ids: Vec::new(),
            name: NEW_ALBUM_NAME.into(),
            date: String::new(),
            use_newest: true,
            place: String::new(),
            description: String::new(),
        }
    }
}

/// Rename Files' draft (`organizing.md` §9): the photos, the new name, and whether the date taken
/// and the image size join it. Rename runs `photo.rename`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct RenameDraft {
    pub ids: Vec<PhotoId>,
    pub name: String,
    pub add_date: bool,
    pub add_size: bool,
}

/// The new-folder dialog's draft (Move to New Folder…, Split Folder Here…): the photos moved, and
/// the folder's name, date as typed with "Use the date of the oldest photo", place and
/// description. OK runs `photo.moveToNewFolder`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct NewFolderDraft {
    pub ids: Vec<PhotoId>,
    pub name: String,
    pub date: String,
    pub use_oldest: bool,
    pub place: String,
    pub description: String,
}

/// The name a new folder starts with.
pub const NEW_FOLDER_NAME: &str = "New Folder";

impl NewFolderDraft {
    /// This draft within its limits, as [`AlbumDraft::sanitized`].
    pub fn sanitized(mut self) -> NewFolderDraft {
        self.ids.truncate(MAX_IDS);
        dedup(&mut self.ids);
        for t in [&mut self.name, &mut self.place, &mut self.description] {
            cut(t);
        }
        cut_chars(&mut self.date, MAX_DATE_TEXT);
        self
    }
}

impl RenameDraft {
    /// This draft within its limits: the photos cut to [`MAX_IDS`] without repeats, the name to
    /// [`MAX_DRAFT`] characters.
    pub fn sanitized(mut self) -> RenameDraft {
        self.ids.truncate(MAX_IDS);
        dedup(&mut self.ids);
        cut(&mut self.name);
        self
    }
}

impl Default for NewFolderDraft {
    fn default() -> NewFolderDraft {
        NewFolderDraft {
            ids: Vec::new(),
            name: NEW_FOLDER_NAME.into(),
            date: String::new(),
            use_oldest: true,
            place: String::new(),
            description: String::new(),
        }
    }
}

/// The first-time move question while it is open: the photos, the folder they go to, and its
/// "Don't ask again".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveQuestion {
    pub ids: Vec<PhotoId>,
    pub folder: FolderId,
    #[serde(default)]
    pub dont_ask: bool,
}

/// The copy question while it is open: the photos and the folder they are copied to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyQuestion {
    pub ids: Vec<PhotoId>,
    pub folder: FolderId,
}

/// Whether a file operation moves or copies photos, or moves a folder (whose merge into a folder of
/// the same name asks the collision question too).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileOp {
    Move,
    Copy,
    MoveFolder,
}

/// The most clashing file names the collision question keeps (it names a few and counts the rest).
pub const MAX_CLASHES: usize = 1000;

/// The collision question while it is open: the move or copy it interrupts, and the names that
/// clash in the destination. For a move or copy of photos `folder` is where they go; for a folder
/// move (`moveFolder`) it is the folder that moves, `to` the folder it goes into and `ids` empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollisionQuestion {
    pub op: FileOp,
    pub ids: Vec<PhotoId>,
    pub folder: FolderId,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<std::path::PathBuf>,
}

/// The delete question for photos while it is open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeletePhotos {
    pub ids: Vec<PhotoId>,
}

/// The delete question for a folder while it is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteFolder {
    pub folder: FolderId,
}

/// The tabs of the Options dialog (`menus-and-shortcuts.md` §5) that exist: the four whose options
/// exist (Slideshow, Network, People and Photo Viewer join with their milestones). Named as in
/// their widget ids (`tab:options.fileTypes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OptionsTab {
    #[default]
    General,
    FileTypes,
    Export,
    /// Keyboard Shortcuts: the shortcut editor (`menus-and-shortcuts.md` §2.0).
    Shortcuts,
}

impl OptionsTab {
    pub const ALL: [OptionsTab; 4] = [OptionsTab::General, OptionsTab::FileTypes, OptionsTab::Export, OptionsTab::Shortcuts];

    /// The tab as the protocol writes it (`fileTypes`), the `<tab>` of `tab:options.<tab>`.
    pub fn name(self) -> &'static str {
        match self {
            OptionsTab::General => "general",
            OptionsTab::FileTypes => "fileTypes",
            OptionsTab::Export => "export",
            OptionsTab::Shortcuts => "shortcuts",
        }
    }
}

/// Options ▸ General ▸ UI scale as the dialog previews it (`design-system.md` §11.4): Auto, or a
/// fixed scale in percent, [`ScaleChoice::MIN`] … [`ScaleChoice::MAX`] in steps of
/// [`ScaleChoice::STEP`]. Written `"auto"` or the number (`150`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScaleChoice {
    #[default]
    Auto,
    Percent(u16),
}

impl ScaleChoice {
    pub const MIN: u16 = dalica_ui_kit::theme::UiScale::MIN_PERCENT;
    pub const MAX: u16 = dalica_ui_kit::theme::UiScale::MAX_PERCENT;
    pub const STEP: u16 = 25;

    /// The choice `percent` names, if it is one of the fixed steps.
    pub fn percent(percent: u64) -> Option<ScaleChoice> {
        let p = u16::try_from(percent).ok()?;
        ((Self::MIN..=Self::MAX).contains(&p) && p % Self::STEP == 0).then_some(ScaleChoice::Percent(p))
    }

    /// The `ui-kit` scale this choice is.
    pub fn ui_scale(self) -> dalica_ui_kit::theme::UiScale {
        match self {
            ScaleChoice::Auto => dalica_ui_kit::theme::UiScale::Auto,
            ScaleChoice::Percent(p) => dalica_ui_kit::theme::UiScale::Percent(p),
        }
    }
}

impl Serialize for ScaleChoice {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ScaleChoice::Auto => s.serialize_str("auto"),
            ScaleChoice::Percent(p) => s.serialize_u16(*p),
        }
    }
}

impl<'de> Deserialize<'de> for ScaleChoice {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<ScaleChoice, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        let choice = match &v {
            serde_json::Value::String(s) if s == "auto" => Some(ScaleChoice::Auto),
            serde_json::Value::Number(n) => n.as_u64().and_then(ScaleChoice::percent),
            _ => None,
        };
        choice.ok_or_else(|| {
            serde::de::Error::custom(format!(
                "a UI scale is \"auto\" or {} … {} in steps of {}, not {v}",
                ScaleChoice::MIN,
                ScaleChoice::MAX,
                ScaleChoice::STEP
            ))
        })
    }
}

/// Options ▸ General ▸ UI font size as the dialog previews it (`design-system.md` §11.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FontChoice {
    #[default]
    Small,
    Medium,
    Large,
}

impl FontChoice {
    /// The `ui-kit` font size this choice is.
    pub fn font_size(self) -> dalica_ui_kit::theme::FontSize {
        match self {
            FontChoice::Small => dalica_ui_kit::theme::FontSize::Small,
            FontChoice::Medium => dalica_ui_kit::theme::FontSize::Medium,
            FontChoice::Large => dalica_ui_kit::theme::FontSize::Large,
        }
    }
}

/// The longest command id the shortcut editor's draft names, in characters.
pub const MAX_COMMAND_ID: usize = 128;
/// The most changed bindings the shortcut editor's draft holds.
pub const MAX_SHORTCUT_CHANGES: usize = 1024;
/// The most key presses kept for the shortcut editor between two of its frames.
pub const MAX_CAPTURED: usize = 16;
/// The most option values the Options dialog's draft holds, and the longest settings key.
pub const MAX_OPTION_VALUES: usize = 256;
pub const MAX_OPTION_KEY: usize = 128;
/// The keys of a command per context in the shortcut editor (`menus-and-shortcuts.md` §2.0): slot
/// 0 and slot 1.
pub const SHORTCUT_SLOTS: u8 = 2;

/// A key cell of the shortcut editor: one of a command's two keys (`slot` 0 or 1) in a context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeySlot {
    pub command: String,
    pub context: dalica_engine::Context,
    pub slot: u8,
}

/// A binding the shortcut editor's draft changes: the key cell and its new key (`Ctrl+1` as the
/// keymap writes it; `null` removes it). Nothing is applied before the dialog's Apply or OK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShortcutChange {
    pub command: String,
    pub context: dalica_engine::Context,
    pub slot: u8,
    /// Required: a key, or `null` to remove it.
    #[serde(deserialize_with = "present")]
    pub key: Option<String>,
}

/// An `Option` field that must be given (`null` for none): missing is an error, not `None`.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}

/// A conflict waiting for [Replace] or [Cancel] ("Ctrl+1 is already Small Thumbnails in
/// Library."): the cell being recorded and the key pressed in it, and the binding that already has
/// that key (`command` in `context`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShortcutConflict {
    pub cell: KeySlot,
    pub key: String,
    pub command: String,
    pub context: dalica_engine::Context,
}

/// The shortcut editor's state (Options ▸ Keyboard Shortcuts, `menus-and-shortcuts.md` §2.0): the
/// search text, the Context filter (`null`: All), "Show commands without a shortcut", the key cell
/// recording, a conflict waiting for an answer, and the draft: the bindings changed and whether
/// Reset All was pressed, applied by the dialog's Apply or OK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ShortcutEditor {
    pub search: String,
    pub context: Option<dalica_engine::Context>,
    pub show_unbound: bool,
    pub recording: Option<KeySlot>,
    pub conflict: Option<ShortcutConflict>,
    pub changes: Vec<ShortcutChange>,
    pub reset_all: bool,
    /// The key presses taken for the recording cell since the editor last read them
    /// (`crate::keys::capture_for_editor`), oldest first, at most [`MAX_CAPTURED`]. Not reported.
    #[serde(skip)]
    pub captured: Vec<dalica_engine::Key>,
}

impl ShortcutEditor {
    /// Whether the draft changes anything (Apply is enabled).
    pub fn changed(&self) -> bool {
        self.reset_all || !self.changes.is_empty()
    }
}

/// The Options dialog's draft while it is open (Tools ▸ Options…, `menus-and-shortcuts.md` §5,
/// `design-system.md` §11.3): the tab shown, the option values changed since the last Apply (by
/// settings key, as `options.list` names them and `options.set {values}` takes them), the last
/// Apply's error, the UI scale and font size being previewed (`null`: the saved ones), and the
/// shortcut editor. Cancel and Esc drop it, so the preview reverts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct OptionsDraft {
    pub tab: OptionsTab,
    pub values: serde_json::Map<String, serde_json::Value>,
    pub error: Option<String>,
    pub preview_scale: Option<ScaleChoice>,
    pub preview_font_size: Option<FontChoice>,
    pub shortcuts: ShortcutEditor,
}

impl OptionsDraft {
    /// A draft showing `tab`, nothing changed.
    pub fn on(tab: OptionsTab) -> OptionsDraft {
        OptionsDraft { tab, ..OptionsDraft::default() }
    }

    /// Whether the draft changes anything (Apply is enabled).
    pub fn changed(&self) -> bool {
        !self.values.is_empty() || self.shortcuts.changed()
    }

    /// Why this draft cannot be used, if it cannot: too many values or a key too long, a value that
    /// is not a single null, boolean, number or text (texts at most [`MAX_DRAFT`] characters), a
    /// text over its length, a command id over [`MAX_COMMAND_ID`], a slot past 1, a key the keymap
    /// cannot read, too many changes. What `ui.set` refuses (`bad_params`).
    pub fn check(&self) -> Result<(), String> {
        let long = |name: &str, text: &str, max: usize| {
            if text.chars().count() > max { Err(format!("`{name}` is at most {max} characters")) } else { Ok(()) }
        };
        if self.values.len() > MAX_OPTION_VALUES {
            return Err(format!("`options.values` holds at most {MAX_OPTION_VALUES} options"));
        }
        for (k, v) in &self.values {
            if k.is_empty() || k.chars().count() > MAX_OPTION_KEY {
                return Err(format!("`options.values` keys are settings keys of 1 … {MAX_OPTION_KEY} characters"));
            }
            match v {
                serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
                serde_json::Value::String(s) => long(&format!("options.values.{k}"), s, MAX_DRAFT)?,
                _ => return Err(format!("`options.values.{k}` is a null, a boolean, a number or a text, not {v}")),
            }
        }
        long("options.error", self.error.as_deref().unwrap_or(""), MAX_DRAFT)?;
        let e = &self.shortcuts;
        long("options.shortcuts.search", &e.search, MAX_DRAFT)?;
        if e.changes.len() > MAX_SHORTCUT_CHANGES {
            return Err(format!("`options.shortcuts.changes` holds at most {MAX_SHORTCUT_CHANGES} bindings"));
        }
        let cell = |name: &str, command: &str, slot: u8| {
            if command.is_empty() || command.chars().count() > MAX_COMMAND_ID {
                Err(format!("`{name}.command` is a command id of 1 … {MAX_COMMAND_ID} characters"))
            } else if slot >= SHORTCUT_SLOTS {
                Err(format!("`{name}.slot` is 0 or 1, not {slot}"))
            } else {
                Ok(())
            }
        };
        let key = |name: &str, key: &str| dalica_engine::Key::parse(key).map(|_| ()).map_err(|e| format!("`{name}.key`: {e}"));
        if let Some(r) = &e.recording {
            cell("options.shortcuts.recording", &r.command, r.slot)?;
        }
        if let Some(c) = &e.conflict {
            cell("options.shortcuts.conflict.cell", &c.cell.command, c.cell.slot)?;
            cell("options.shortcuts.conflict", &c.command, 0)?;
            key("options.shortcuts.conflict", &c.key)?;
        }
        for c in &e.changes {
            cell("options.shortcuts.changes", &c.command, c.slot)?;
            if let Some(k) = &c.key {
                key("options.shortcuts.changes", k)?;
            }
        }
        Ok(())
    }

    /// Every command the shortcut editor's draft names, for `ui.set` to check that the keymap knows
    /// them.
    pub fn commands(&self) -> Vec<&str> {
        let e = &self.shortcuts;
        let mut out: Vec<&str> = Vec::new();
        out.extend(e.recording.as_ref().map(|r| r.command.as_str()));
        if let Some(c) = &e.conflict {
            out.extend([c.cell.command.as_str(), c.command.as_str()]);
        }
        out.extend(e.changes.iter().map(|c| c.command.as_str()));
        out
    }

    /// This draft within its limits: values past [`MAX_OPTION_VALUES`] dropped, texts cut, changes
    /// past [`MAX_SHORTCUT_CHANGES`] and captured keys past [`MAX_CAPTURED`] (the oldest) dropped.
    pub fn sanitized(mut self) -> OptionsDraft {
        if self.values.len() > MAX_OPTION_VALUES {
            self.values = std::mem::take(&mut self.values).into_iter().take(MAX_OPTION_VALUES).collect();
        }
        for v in self.values.values_mut() {
            if let serde_json::Value::String(s) = v {
                cut(s);
            }
        }
        if let Some(e) = &mut self.error {
            cut(e);
        }
        let e = &mut self.shortcuts;
        cut(&mut e.search);
        e.changes.truncate(MAX_SHORTCUT_CHANGES);
        let extra = e.captured.len().saturating_sub(MAX_CAPTURED);
        e.captured.drain(..extra);
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
    /// The Export dialog's draft while it is open (`output.export`).
    #[serde(default)]
    pub export: Option<ExportDraft>,
    /// The first-time Save explanation's checkbox while it is open (`photo.save`).
    #[serde(default)]
    pub save_explain: Option<SaveExplain>,
    /// The Revert question's photos while it is open (`photo.revert`).
    #[serde(default)]
    pub revert: Option<RevertQuestion>,
    /// The export progress window: the job it follows, shown or hidden.
    #[serde(default)]
    pub export_progress: Option<ExportProgress>,
    /// The search box, the filters and the age slider, and the scroll anchor search mode saved.
    #[serde(default)]
    pub search: SearchState,
    /// The photos cut or copied for Paste (Edit ▸ Cut / Copy).
    #[serde(default)]
    pub clipboard: Option<Clipboard>,
    /// Album Properties' draft while it is open (`album.new`, `section.properties` on an album).
    #[serde(default)]
    pub album_properties: Option<AlbumDraft>,
    /// Rename Files' draft while it is open (`photo.rename` from the menu or F2).
    #[serde(default)]
    pub rename_files: Option<RenameDraft>,
    /// The new-folder dialog's draft while it is open (Move to New Folder…, Split Folder Here…).
    #[serde(default)]
    pub new_folder: Option<NewFolderDraft>,
    /// The first-time move question while it is open (`photo.moveTo`).
    #[serde(default)]
    pub move_files: Option<MoveQuestion>,
    /// The copy question while it is open (Paste after Copy, a Ctrl-drag).
    #[serde(default)]
    pub copy_files: Option<CopyQuestion>,
    /// The collision question while it is open (a move or copy onto files of the same names).
    #[serde(default)]
    pub collision: Option<CollisionQuestion>,
    /// The delete question for photos while it is open (`photo.delete`).
    #[serde(default)]
    pub delete_photos: Option<DeletePhotos>,
    /// The delete question for a folder while it is open (`folder.delete`).
    #[serde(default)]
    pub delete_folder: Option<DeleteFolder>,
    /// The Options dialog's draft while it is open (`app.preferences`, `app.shortcuts`): its tab,
    /// the changed values, the last Apply's error, the scale and font size previewed, and the
    /// shortcut editor.
    #[serde(default)]
    pub options: Option<OptionsDraft>,
    /// A drag of photos in progress (`crate::drag`); `ui.inspect` reports it, `ui.state` does not.
    #[serde(skip)]
    pub drag: Option<Drag>,
    /// Edit ▸ Find (Ctrl+F, F3) asked the toolbar to focus the search field; the toolbar takes it.
    #[serde(skip)]
    pub focus_search: bool,
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
        self.export = self.export.take().map(ExportDraft::sanitized);
        if let Some(r) = &mut self.revert {
            r.photos.truncate(MAX_IDS);
            dedup(&mut r.photos);
        }
        self.sanitize_m9();
        self.options = self.options.take().map(OptionsDraft::sanitized);
        if self.view == View::Edit && self.edit.photo.is_none() {
            self.view = View::Library;
        }
        self
    }

    /// Why the M10 fields of this state cannot be used, if they cannot
    /// ([`OptionsDraft::check`]). What `ui.set` refuses (`bad_params`).
    pub fn check_m10(&self) -> Result<(), String> {
        self.options.as_ref().map_or(Ok(()), OptionsDraft::check)
    }

    /// Every M9 field within its limits: lists cut to [`MAX_IDS`] without repeats, texts to their
    /// lengths, the highlight below [`MAX_HIGHLIGHT`], the age in its range.
    fn sanitize_m9(&mut self) {
        let ids = |v: &mut Vec<PhotoId>| {
            v.truncate(MAX_IDS);
            dedup(v);
        };
        let s = &mut self.search;
        cut(&mut s.text);
        s.highlight = s.highlight.filter(|h| *h < MAX_HIGHLIGHT);
        s.age = clamp_or(s.age, 0.0, AGE_MAX, 0.0);
        if let Some(a) = &mut s.saved_scroll {
            a.offset = clamp_or(a.offset, 0.0, SCROLL_OFFSET_MAX, 0.0);
        }
        if let Some(c) = &mut self.clipboard {
            ids(&mut c.ids);
        }
        self.album_properties = self.album_properties.take().map(AlbumDraft::sanitized);
        self.rename_files = self.rename_files.take().map(RenameDraft::sanitized);
        self.new_folder = self.new_folder.take().map(NewFolderDraft::sanitized);
        if let Some(q) = &mut self.move_files {
            ids(&mut q.ids);
        }
        if let Some(q) = &mut self.copy_files {
            ids(&mut q.ids);
        }
        if let Some(q) = &mut self.collision {
            ids(&mut q.ids);
            q.files.truncate(MAX_CLASHES);
            for f in &mut q.files {
                cut(f);
            }
            if q.to.as_ref().is_some_and(|to| to.to_string_lossy().chars().count() > MAX_DRAFT) {
                q.to = None;
            }
        }
        if let Some(q) = &mut self.delete_photos {
            ids(&mut q.ids);
        }
        if let Some(d) = &mut self.drag {
            ids(&mut d.ids);
        }
    }

    /// Why the M9 fields of this state cannot be used, if they cannot: a list longer than
    /// [`MAX_IDS`] (or [`MAX_CLASHES`] names), a text over its length, a highlight past
    /// [`MAX_HIGHLIGHT`], an age outside 0 … [`AGE_MAX`], a scroll offset out of range. What
    /// `ui.set` refuses (`bad_params`) before it changes anything.
    pub fn check_m9(&self) -> Result<(), String> {
        let long = |name: &str, text: &str, max: usize| {
            if text.chars().count() > max { Err(format!("`{name}` is at most {max} characters")) } else { Ok(()) }
        };
        let many = |name: &str, n: usize| if n > MAX_IDS { Err(format!("`{name}` holds at most {MAX_IDS} photos")) } else { Ok(()) };
        let s = &self.search;
        long("search.text", &s.text, MAX_DRAFT)?;
        if s.highlight.is_some_and(|h| h >= MAX_HIGHLIGHT) {
            return Err(format!("`search.highlight` is 0 … {}, or null", MAX_HIGHLIGHT - 1));
        }
        if !within(s.age, 0.0, AGE_MAX) {
            return Err(format!("`search.age` is the age slider's position, 0 (everything) … {AGE_MAX}, not {}", s.age));
        }
        if s.saved_scroll.is_some_and(|a| !within(a.offset, 0.0, SCROLL_OFFSET_MAX)) {
            return Err(format!("`search.savedScroll.offset` is 0 … {SCROLL_OFFSET_MAX}"));
        }
        if let Some(c) = &self.clipboard {
            many("clipboard.ids", c.ids.len())?;
        }
        if let Some(d) = &self.album_properties {
            many("albumProperties.ids", d.ids.len())?;
            long("albumProperties.name", &d.name, MAX_DRAFT)?;
            long("albumProperties.date", &d.date, MAX_DATE_TEXT)?;
            long("albumProperties.place", &d.place, MAX_DRAFT)?;
            long("albumProperties.description", &d.description, MAX_DRAFT)?;
        }
        if let Some(d) = &self.rename_files {
            many("renameFiles.ids", d.ids.len())?;
            long("renameFiles.name", &d.name, MAX_DRAFT)?;
        }
        if let Some(d) = &self.new_folder {
            many("newFolder.ids", d.ids.len())?;
            long("newFolder.name", &d.name, MAX_DRAFT)?;
            long("newFolder.date", &d.date, MAX_DATE_TEXT)?;
            long("newFolder.place", &d.place, MAX_DRAFT)?;
            long("newFolder.description", &d.description, MAX_DRAFT)?;
        }
        if let Some(q) = &self.move_files {
            many("moveFiles.ids", q.ids.len())?;
        }
        if let Some(q) = &self.copy_files {
            many("copyFiles.ids", q.ids.len())?;
        }
        if let Some(q) = &self.collision {
            many("collision.ids", q.ids.len())?;
            if q.files.len() > MAX_CLASHES {
                return Err(format!("`collision.files` holds at most {MAX_CLASHES} names"));
            }
            for f in &q.files {
                long("collision.files", f, MAX_DRAFT)?;
            }
            if let Some(to) = &q.to {
                long("collision.to", &to.to_string_lossy(), MAX_DRAFT)?;
            }
        }
        if let Some(q) = &self.delete_photos {
            many("deletePhotos.ids", q.ids.len())?;
        }
        Ok(())
    }

    /// Every photo the M9 fields name (the clipboard and the drafts), for `ui.set` to check that
    /// they exist.
    pub fn m9_photos(&self) -> impl Iterator<Item = &PhotoId> {
        let lists: [&[PhotoId]; 8] = [
            self.clipboard.as_ref().map_or(&[], |c| c.ids.as_slice()),
            self.album_properties.as_ref().map_or(&[], |d| d.ids.as_slice()),
            self.rename_files.as_ref().map_or(&[], |d| d.ids.as_slice()),
            self.new_folder.as_ref().map_or(&[], |d| d.ids.as_slice()),
            self.move_files.as_ref().map_or(&[], |q| q.ids.as_slice()),
            self.copy_files.as_ref().map_or(&[], |q| q.ids.as_slice()),
            self.collision.as_ref().map_or(&[], |q| q.ids.as_slice()),
            self.delete_photos.as_ref().map_or(&[], |q| q.ids.as_slice()),
        ];
        lists.into_iter().flatten()
    }

    /// Every folder the M9 drafts name, for `ui.set` to check that they exist.
    pub fn m9_folders(&self) -> Vec<FolderId> {
        let mut out = Vec::new();
        out.extend(self.move_files.as_ref().map(|q| q.folder));
        out.extend(self.copy_files.as_ref().map(|q| q.folder));
        out.extend(self.collision.as_ref().map(|q| q.folder));
        out.extend(self.delete_folder.map(|q| q.folder));
        out
    }

    /// Why the M8 fields of this state cannot be used, if they cannot: the Export draft's
    /// ([`ExportDraft::check`]), too many photos to revert. What `ui.set` refuses (`bad_params`).
    pub fn check_m8(&self) -> Result<(), String> {
        if let Some(e) = &self.export {
            e.check()?;
        }
        match &self.revert {
            Some(r) if r.photos.len() > MAX_IDS => Err(format!("`revert.photos` holds at most {MAX_IDS} photos")),
            _ => Ok(()),
        }
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

/// Cuts `text` to `max` characters.
fn cut_chars(text: &mut String, max: usize) {
    if let Some((at, _)) = text.char_indices().nth(max) {
        text.truncate(at);
    }
}

/// Cuts `text` to at most `max` bytes, at a character boundary.
fn cut_bytes(text: &mut String, max: usize) {
    if text.len() > max {
        let at = (0..=max).rev().find(|i| text.is_char_boundary(*i)).unwrap_or(0);
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
            export: None,
            save_explain: None,
            revert: None,
            export_progress: None,
            search: SearchState::default(),
            clipboard: None,
            album_properties: None,
            rename_files: None,
            new_folder: None,
            move_files: None,
            copy_files: None,
            collision: None,
            delete_photos: None,
            delete_folder: None,
            options: None,
            drag: None,
            focus_search: false,
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

    /// The default view state, as `ui.state` sends it: every field's protocol name and default in
    /// one golden file (`tests/golden/default-state.json`; `DALICA_BLESS=1` writes it).
    #[test]
    fn the_default_state_is_the_golden_one() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/default-state.json");
        let v = serde_json::to_value(UiState::default()).unwrap();
        if std::env::var_os("DALICA_BLESS").is_some() {
            std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
        }
        let golden: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v, golden, "the default state differs from {}", path.display());
    }

    #[test]
    fn the_view_enums_have_their_protocol_names() {
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
    fn the_m6_fields_round_trip_with_their_protocol_names() {
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
    fn the_m8_fields_have_their_protocol_names_and_are_bounded() {
        assert_eq!(
            serde_json::to_value(ExportDraft::default()).unwrap(),
            serde_json::json!({ "ids": [], "section": null, "location": "", "folderName": "", "numbers": false, "originalSize": false,
                                "resize": "1600", "quality": "automatic", "custom": 75, "videos": "firstFrame", "watermark": false, "watermarkText": "" })
        );
        for q in Quality::ALL {
            assert_eq!(serde_json::to_value(q).unwrap(), q.name());
        }
        for k in [DialogKind::SaveExplain, DialogKind::Revert, DialogKind::Export] {
            assert_eq!(DialogKind::from_name(k.name()), Some(k));
        }
        let ok = ExportDraft { resize: "4000".into(), ..ExportDraft::default() };
        assert_eq!(ok.pixels(), Some(4000));
        for bad in ["", "0", "99", "4001", "abc", "-5", "1e3"] {
            assert_eq!(ExportDraft { resize: bad.into(), ..ExportDraft::default() }.pixels(), None, "{bad:?}");
        }
        let hostile = ExportDraft {
            ids: vec![PhotoId::new(1), PhotoId::new(1)],
            location: "é".repeat(MAX_LOCATION),
            folder_name: "x".repeat(MAX_DRAFT + 1),
            resize: "9".repeat(40),
            custom: 0,
            ..ExportDraft::default()
        };
        assert!(hostile.check().is_err());
        let s = UiState { export: Some(hostile), revert: Some(RevertQuestion { photos: vec![PhotoId::new(2); 3] }), ..UiState::default() };
        assert!(s.check_m8().is_err());
        let s = s.sanitized();
        assert_eq!(s.check_m8(), Ok(()));
        let e = s.export.as_ref().unwrap();
        assert_eq!((e.ids.len(), e.location.len() <= MAX_LOCATION, e.resize.len(), e.custom), (1, true, MAX_RESIZE_TEXT, CUSTOM_MIN));
        assert_eq!(s.revert.as_ref().map(|r| r.photos.len()), Some(1));
    }

    #[test]
    fn the_m9_fields_have_their_protocol_names_and_are_bounded() {
        assert_eq!(
            serde_json::to_value(AlbumDraft::default()).unwrap(),
            serde_json::json!({ "album": null, "ids": [], "name": "New Album", "date": "", "useNewest": true, "place": "", "description": "" })
        );
        assert_eq!(
            serde_json::to_value(NewFolderDraft::default()).unwrap(),
            serde_json::json!({ "ids": [], "name": "New Folder", "date": "", "useOldest": true, "place": "", "description": "" })
        );
        // Every field round-trips.
        let p = |n| PhotoId::new(n);
        let s = UiState {
            search: SearchState {
                text: "lisbon".into(),
                highlight: Some(2),
                filters: Filters { starred: true, ..Filters::default() },
                age: 0.25,
                saved_scroll: Some(ScrollAnchor { section: Some(Section::Folder(FolderId::new(3))), offset: 40.0 }),
                duplicates: false,
            },
            clipboard: Some(Clipboard { op: ClipOp::Cut, ids: vec![p(1)] }),
            album_properties: Some(AlbumDraft { album: Some(AlbumId::parse("a91c0e").unwrap()), ..AlbumDraft::default() }),
            rename_files: Some(RenameDraft { ids: vec![p(2)], name: "Lisbon".into(), add_date: true, add_size: false }),
            new_folder: Some(NewFolderDraft { ids: vec![p(3)], ..NewFolderDraft::default() }),
            move_files: Some(MoveQuestion { ids: vec![p(1)], folder: FolderId::new(2), dont_ask: true }),
            copy_files: Some(CopyQuestion { ids: vec![p(1)], folder: FolderId::new(2) }),
            collision: Some(CollisionQuestion {
                op: FileOp::Copy,
                ids: vec![p(1)],
                folder: FolderId::new(2),
                files: vec!["IMG_1.jpg".into()],
                to: None,
            }),
            delete_photos: Some(DeletePhotos { ids: vec![p(4)] }),
            delete_folder: Some(DeleteFolder { folder: FolderId::new(5) }),
            ..UiState::default()
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["clipboard"], serde_json::json!({ "op": "cut", "ids": [1] }));
        assert_eq!(v["moveFiles"], serde_json::json!({ "ids": [1], "folder": 2, "dontAsk": true }));
        assert_eq!(v["collision"]["op"], "copy");
        assert_eq!(v["albumProperties"]["album"], "a91c0e");
        assert_eq!(serde_json::from_value::<UiState>(v).unwrap(), s);
        assert_eq!(s.check_m9(), Ok(()));
        assert_eq!(s.clone().sanitized(), s, "values in range are kept");
        assert_eq!(s.m9_photos().count(), 7);
        assert_eq!(s.m9_folders(), [FolderId::new(2), FolderId::new(2), FolderId::new(2), FolderId::new(5)]);
        assert!(s.search.active());
        assert!(!SearchState::default().active());
        // Hostile values are refused by the check and made safe by sanitizing.
        let long = "x".repeat(MAX_DRAFT + 1);
        let hostile = UiState {
            search: SearchState { text: long.clone(), highlight: Some(MAX_HIGHLIGHT), age: f32::NAN, ..SearchState::default() },
            album_properties: Some(AlbumDraft {
                name: long.clone(),
                date: "1".repeat(MAX_DATE_TEXT + 1),
                ids: vec![p(1), p(1)],
                ..AlbumDraft::default()
            }),
            rename_files: Some(RenameDraft { name: long.clone(), ..RenameDraft::default() }),
            collision: Some(CollisionQuestion {
                op: FileOp::Move,
                ids: vec![],
                folder: FolderId::new(1),
                files: vec![long.clone(); MAX_CLASHES + 1],
                to: Some(std::path::PathBuf::from(&long)),
            }),
            ..UiState::default()
        };
        assert!(hostile.check_m9().is_err());
        for (bad, field) in [
            (UiState { search: SearchState { age: 1.5, ..SearchState::default() }, ..UiState::default() }, "search.age"),
            (UiState { search: SearchState { highlight: Some(MAX_HIGHLIGHT), ..SearchState::default() }, ..UiState::default() }, "search.highlight"),
            (
                UiState { new_folder: Some(NewFolderDraft { description: long.clone(), ..NewFolderDraft::default() }), ..UiState::default() },
                "newFolder.description",
            ),
        ] {
            assert!(bad.check_m9().unwrap_err().contains(field), "{field}");
        }
        let safe = hostile.sanitized();
        assert_eq!(safe.check_m9(), Ok(()));
        assert_eq!((safe.search.text.chars().count(), safe.search.highlight, safe.search.age), (MAX_DRAFT, None, 0.0));
        let d = safe.album_properties.as_ref().unwrap();
        assert_eq!((d.name.chars().count(), d.date.chars().count(), d.ids.len()), (MAX_DRAFT, MAX_DATE_TEXT, 1));
        assert_eq!(safe.collision.as_ref().map(|c| (c.files.len(), c.to.is_none())), Some((MAX_CLASHES, true)), "a path too long is dropped");
        // A folder move's question names the folder it goes into; a path that long is refused.
        let folder_move = UiState {
            collision: Some(CollisionQuestion {
                op: FileOp::MoveFolder,
                ids: vec![],
                folder: FolderId::new(4),
                files: vec!["IMG_1.jpg".into()],
                to: Some(std::path::PathBuf::from("/photos/2024")),
            }),
            ..UiState::default()
        };
        let v = serde_json::to_value(&folder_move).unwrap();
        assert_eq!(v["collision"]["op"], "moveFolder");
        assert_eq!(v["collision"]["to"], "/photos/2024");
        assert_eq!(serde_json::from_value::<UiState>(v).unwrap(), folder_move);
        assert_eq!(folder_move.check_m9(), Ok(()));
        let mut far = folder_move.clone();
        if let Some(q) = &mut far.collision {
            q.to = Some(std::path::PathBuf::from(&long));
        }
        assert!(far.check_m9().unwrap_err().contains("collision.to"));
        // A bad value in a draft is a deserialisation error (`bad_params` in ui.set).
        for bad in [
            serde_json::json!({ "clipboard": { "op": "paste", "ids": [] } }),
            serde_json::json!({ "search": { "filters": { "people": true } } }),
            serde_json::json!({ "albumProperties": { "album": "b1234" } }),
            serde_json::json!({ "moveFiles": { "ids": [1] } }),
        ] {
            let mut merged = serde_json::to_value(UiState::default()).unwrap();
            merged.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
            assert!(serde_json::from_value::<UiState>(merged).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_section_is_written_as_its_widget_id() {
        let s = Section::Folder(FolderId::new(12));
        assert_eq!(s.to_string(), "folder:12");
        assert_eq!(serde_json::to_value(s).unwrap(), "folder:12");
        assert_eq!(serde_json::from_value::<Section>("folder:12".into()).unwrap(), s);
        for bad in [
            "",
            "folder:",
            "folder:-1",
            "folder:1.5",
            "folder: 1",
            "12",
            "folder:99999999999999999999999",
            ":12",
            "album:",
            "album:A91C0E",
            "album:a12",
            "album:91c0e0",
            "album:Starred",
            "person:p1234",
            format!("album:a{}", "f".repeat(17)).as_str(),
        ] {
            assert!(bad.parse::<Section>().is_err(), "{bad:?}");
        }
        // Album sections: user albums by id (leading zeros kept), and the automatic ones.
        for text in ["album:a91c0e", "album:a0012", "album:a00000000000000f", "album:starred", "album:recent", "album:search"] {
            let s: Section = text.parse().unwrap();
            assert_eq!(s.to_string(), text);
            assert!(s.is_album() && s.folder().is_none());
            assert_eq!(serde_json::from_value::<Section>(serde_json::to_value(s).unwrap()).unwrap(), s);
        }
        let a: Section = "album:a0012".parse().unwrap();
        assert_ne!(a, "album:a00012".parse::<Section>().unwrap(), "leading zeros make another album");
        assert_eq!(a.user_album().map(|id| id.as_str().to_string()).as_deref(), Some("a0012"));
        assert_eq!(Section::Album(AlbumSection::Starred).user_album(), None);
        assert_eq!(s.folder(), Some(FolderId::new(12)));
        assert!(!s.is_album());
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
