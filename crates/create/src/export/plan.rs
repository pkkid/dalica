//! The export plan (`creating.md` §2): the export folder, made under the location with a free
//! name, and each photo's file name, before any picture is rendered. A video is planned as
//! "Export videos as" asks ([`VideoAs`]): its poster frame rendered as a picture, or the file
//! copied as it is; a clip whose frames this build does not decode has no poster, so as a first
//! frame it is skipped with the reason, and as a full video it is copied like any clip.

use std::path::{Path, PathBuf};

use dalica_core::names::{self, MAX_NAME_BYTES};
use dalica_edit::Recipe;
use dalica_meta::Changes;

use super::{Error, ExportOptions};
use crate::output::Spec;

/// The stem an exported photo gets when nothing is left of its own name.
pub const PHOTO_FALLBACK: &str = "Photo";
/// The fewest digits of an export number (`001-`).
pub const NUMBER_DIGITS: usize = 3;
/// Room kept for the extension (`.jpg`, `.png`) when a stem is shortened.
const EXT_ROOM: usize = 4;
/// Room kept for a copied video's own extension at most (longer extensions are kept, the stem
/// shortened by this much and the name then fitted by `fs`).
const MAX_EXT_ROOM: usize = 16;
/// Why a video without a poster frame is skipped when its first frame is asked for.
pub const NO_FIRST_FRAME: &str = "This video can't be played";

/// What an item of the tray is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Photo,
    /// A clip whose frames this build does not decode (its codec failed M15.46's record, or is not
    /// known yet): it has no poster frame, so as a first frame it is skipped with
    /// [`NO_FIRST_FRAME`] (the caller may give the codec's reason instead); as a full video it is
    /// copied.
    Video,
    /// A clip whose codec this build decodes: its poster frame (`FrameSource::poster`, M15.20) is
    /// its first frame.
    PlayableVideo,
}

impl Kind {
    pub fn is_video(self) -> bool {
        matches!(self, Kind::Video | Kind::PlayableVideo)
    }
}

/// "Export videos as" (`creating.md` §2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VideoAs {
    /// The poster frame, exported as a picture like a photo (resized, quality, watermark).
    #[default]
    FirstFrame,
    /// The file copied as it is (no resizing), under the export's name with its own extension.
    FullVideo,
}

/// How an item's new file is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Make {
    /// The photo rendered through the one output pipeline.
    Picture,
    /// The clip's poster frame rendered through the one output pipeline.
    FirstFrame,
    /// The file copied byte for byte, never replacing one.
    Copy,
}

/// One item to export, in tray order: its file, its current recipe (unsaved edits are exported)
/// and the library's owned fields to write into the new file.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub path: PathBuf,
    pub kind: Kind,
    pub recipe: Recipe,
    pub changes: Changes,
}

/// An item that will be exported: its place in the tray and the stem of its new name (the
/// extension follows from what the output pipeline writes).
#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
    /// The item's place in the list given to [`plan`].
    pub index: usize,
    pub source: PathBuf,
    pub recipe: Recipe,
    pub changes: Changes,
    /// `beach` or, with numbers, `001-beach`.
    pub stem: String,
    pub make: Make,
}

/// An item left out, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub index: usize,
    pub path: PathBuf,
    pub reason: String,
}

/// Where the files go and how they are made, shared by every item of one export.
#[derive(Clone, Debug, PartialEq)]
pub struct Destination {
    /// The export folder, made by [`plan`].
    pub folder: PathBuf,
    /// The name it got (`Lisbon`, or `Lisbon-001` when `Lisbon` was taken).
    pub name: String,
    /// The output pipeline's request; each item adds its own owned fields.
    pub spec: Spec,
}

/// A whole export, ready to run item by item.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub destination: Destination,
    pub items: Vec<Planned>,
    pub skipped: Vec<Skipped>,
}

/// [`names_as`] with videos exported as their first frame.
pub fn names(items: Vec<Item>, numbers: bool) -> (Vec<Planned>, Vec<Skipped>) {
    names_as(items, numbers, VideoAs::FirstFrame)
}

/// How `kind` is made when videos are exported `as`; `None`: skipped.
fn make_of(kind: Kind, videos: VideoAs) -> Option<Make> {
    match (kind, videos) {
        (Kind::Photo, _) => Some(Make::Picture),
        (Kind::PlayableVideo, VideoAs::FirstFrame) => Some(Make::FirstFrame),
        (Kind::Video, VideoAs::FirstFrame) => None,
        (Kind::Video | Kind::PlayableVideo, VideoAs::FullVideo) => Some(Make::Copy),
    }
}

/// The new names of `items`, without touching the disk: each item's stem sanitized
/// (`core::names::sanitize`), numbered `001-`, `002-`, … in tray order when `numbers` is on (more
/// digits when there are more than 999), and shortened so the name with its extension fits 255
/// bytes. Videos are planned as `videos` asks ([`VideoAs`]); a [`Kind::Video`] as a first frame
/// is skipped with [`NO_FIRST_FRAME`]. Two items with one stem keep it: their files get `-001`
/// through exclusive creation when they are written.
pub fn names_as(items: Vec<Item>, numbers: bool, videos: VideoAs) -> (Vec<Planned>, Vec<Skipped>) {
    let count = items.iter().filter(|i| make_of(i.kind, videos).is_some()).count();
    let digits = NUMBER_DIGITS.max(count.to_string().len());
    let mut planned = Vec::with_capacity(count);
    let mut skipped = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let Some(make) = make_of(item.kind, videos) else {
            skipped.push(Skipped { index, path: item.path, reason: NO_FIRST_FRAME.into() });
            continue;
        };
        let ext_room = match make {
            Make::Copy => copy_extension(&item.path).map_or(0, |e| e.len() + 1).min(MAX_EXT_ROOM),
            Make::Picture | Make::FirstFrame => EXT_ROOM,
        };
        let prefix = if numbers { format!("{:0digits$}-", planned.len() + 1) } else { String::new() };
        let stem = stem_of(&item.path, MAX_NAME_BYTES.saturating_sub(prefix.len() + ext_room));
        planned.push(Planned { index, source: item.path, recipe: item.recipe, changes: item.changes, stem: format!("{prefix}{stem}"), make });
    }
    (planned, skipped)
}

/// The extension a copied video keeps: its own, as its file name spells it (`None` without one).
pub fn copy_extension(path: &Path) -> Option<String> {
    let file = path.file_name()?.to_string_lossy().into_owned();
    let (_, ext) = names::split_ext(&file);
    ext.filter(|e| !e.is_empty()).map(str::to_string)
}

/// The sanitized stem of `path`'s file name, at most `room` bytes.
fn stem_of(path: &Path, room: usize) -> String {
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, _) = names::split_ext(&file);
    let clean = names::sanitize(stem, PHOTO_FALLBACK);
    // Sanitizing a cut of a clean name only strips a trailing dot or space; it never grows.
    names::sanitize(names::fit(&clean, room), PHOTO_FALLBACK)
}

/// Plans an export of `items` (in tray order) into a new folder under `location`: checks the
/// options, makes `location` if it is missing, then makes the export folder under the first free
/// name of `folder_name` sanitized (`fallback`, such as `Export 2026-06-04`, when nothing is left
/// of it; `<name>-001` when it is taken; never an existing folder), and names every item
/// ([`names`]).
pub fn plan(location: &Path, folder_name: &str, fallback: &str, items: Vec<Item>, options: &ExportOptions) -> Result<Plan, Error> {
    plan_as(location, folder_name, fallback, items, options, VideoAs::FirstFrame)
}

/// [`plan`] with videos exported as `videos` asks ([`names_as`]).
pub fn plan_as(
    location: &Path,
    folder_name: &str,
    fallback: &str,
    items: Vec<Item>,
    options: &ExportOptions,
    videos: VideoAs,
) -> Result<Plan, Error> {
    let spec = options.spec().map_err(Error::Options)?;
    let wanted = names::sanitize(folder_name, fallback);
    dalica_fs::create_dir_all(location).map_err(|e| Error::Folder(e.to_string()))?;
    let name = dalica_fs::create::new_folder(location, &wanted).map_err(|e| Error::Folder(e.to_string()))?;
    let (items, skipped) = names_as(items, options.numbers, videos);
    Ok(Plan { destination: Destination { folder: location.join(&name), name, spec }, items, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, kind: Kind) -> Item {
        Item { path: Path::new("photos").join(name), kind, recipe: Recipe::default(), changes: Changes::default() }
    }

    fn stems(planned: &[Planned]) -> Vec<&str> {
        planned.iter().map(|p| p.stem.as_str()).collect()
    }

    #[test]
    fn stems_come_from_the_file_names() {
        let (planned, skipped) =
            names(vec![item("beach.jpg", Kind::Photo), item("archive.tar.png", Kind::Photo), item("README", Kind::Photo)], false);
        assert_eq!(stems(&planned), ["beach", "archive.tar", "README"]);
        assert!(skipped.is_empty());
    }

    #[test]
    fn numbers_follow_the_tray_order_and_skip_videos() {
        let items = vec![item("c.jpg", Kind::Photo), item("clip.mp4", Kind::Video), item("a.jpg", Kind::Photo), item("b.png", Kind::Photo)];
        let (planned, skipped) = names(items, true);
        assert_eq!(stems(&planned), ["001-c", "002-a", "003-b"]);
        assert_eq!(planned.iter().map(|p| p.index).collect::<Vec<_>>(), [0, 2, 3]);
        assert_eq!(skipped.len(), 1);
        assert_eq!((skipped[0].index, skipped[0].reason.as_str()), (1, NO_FIRST_FRAME));
    }

    #[test]
    fn videos_are_planned_as_asked() {
        let items = || vec![item("a.jpg", Kind::Photo), item("clip.MOV", Kind::PlayableVideo), item("old.mkv", Kind::Video)];
        let (first, skipped) = names_as(items(), true, VideoAs::FirstFrame);
        assert_eq!(first.iter().map(|p| (p.stem.as_str(), p.make)).collect::<Vec<_>>(), [("001-a", Make::Picture), ("002-clip", Make::FirstFrame)]);
        assert_eq!(skipped.iter().map(|s| s.index).collect::<Vec<_>>(), [2]);
        let (full, skipped) = names_as(items(), true, VideoAs::FullVideo);
        assert_eq!(
            full.iter().map(|p| (p.stem.as_str(), p.make)).collect::<Vec<_>>(),
            [("001-a", Make::Picture), ("002-clip", Make::Copy), ("003-old", Make::Copy)]
        );
        assert!(skipped.is_empty());
        assert_eq!(copy_extension(Path::new("x/clip.MOV")).as_deref(), Some("MOV"));
        assert_eq!(copy_extension(Path::new("x/clip")), None);
    }

    #[test]
    fn numbers_grow_past_999_and_keep_their_order() {
        let items: Vec<Item> = (0..1200).map(|i| item(&format!("p{i}.jpg"), Kind::Photo)).collect();
        let (planned, _) = names(items, true);
        assert_eq!(planned[0].stem, "0001-p0");
        assert_eq!(planned[1199].stem, "1200-p1199");
        let mut sorted = stems(&planned);
        sorted.sort_unstable();
        assert_eq!(sorted, stems(&planned), "names sort in tray order");
    }

    #[test]
    fn hostile_and_long_stems_are_sanitized_to_fit() {
        let long = format!("{}.jpg", "é".repeat(200));
        let (planned, _) = names(
            vec![item("CON.jpg", Kind::Photo), item("...jpg", Kind::Photo), item(&long, Kind::Photo), item("-x\u{202E}y.jpg", Kind::Photo)],
            true,
        );
        assert_eq!(planned[0].stem, "001-_CON");
        assert_eq!(planned[1].stem, "002-Photo");
        assert!(planned[2].stem.len() + EXT_ROOM <= MAX_NAME_BYTES, "{}", planned[2].stem.len());
        assert_eq!(planned[3].stem, "004-xy");
    }
}
