//! The open collage (`creating.md` §3, `roadmap.md` M14 → *Collage*): at most one Collage room at a
//! time, its document (`dalica_create::collage::Collage`) and the room's own history (Ctrl+Z and
//! Ctrl+Y in the room; never the library's undo stack). The commands are `cmd::collage`; the
//! room's last style and settings are [`crate::settings::CollageSettings`].
//!
//! The history keeps whole documents (a document is plain data, at most 1 000 clips), so an undo
//! gives back exactly what was there. The placements are `create`'s layouts over the pictures'
//! aspect ratios from the model (never a decode), kept until the document or the library changes.
//! **Videos** (M15.28, `creating.md` §3): a clip whose codec this build decodes goes in a collage
//! as any photo ([`Session::collage_photos`]), laid out at its poster frame's ratio (the model's
//! size, which the scan turns as the player does, then its own rotations) and drawn from its poster frame
//! (`collage_output`). A clip whose codec failed, or whose facts are not read yet, has no poster:
//! it is left out and counted, and the commands say so with M15.16's notice.
//!
//! Nothing here touches the disk.

use std::collections::HashSet;

use dalica_core::PhotoId;
use dalica_create::collage::{Collage as Doc, Placement, layout};
use dalica_edit::Cropping;
use dalica_library::{AlbumRef, Library, Section};

use crate::error::CmdError;
use crate::session::Session;

/// Whether photo `id` is a clip with a poster frame: a video whose codec this build decodes, by
/// its scan's facts (`video::playback`).
pub(crate) fn has_poster(lib: &Library, id: PhotoId) -> bool {
    crate::video::is_video(lib, id) && lib.video_facts(id).is_some_and(|f| crate::video::playback(f).video.is_ok())
}

/// The most steps the room's history keeps; the oldest go first.
pub const MAX_HISTORY: usize = 100;

/// The Collage room's state.
#[derive(Debug, Default)]
pub struct Collage {
    room: Option<Room>,
}

#[derive(Debug)]
struct Room {
    doc: Doc,
    /// The document as opened or last written (M14.17): the room has unsaved changes while the
    /// document differs from it.
    baseline: Doc,
    undo: Vec<Doc>,
    redo: Vec<Doc>,
    /// The display's size in pixels the window last gave (for Current Display).
    display: Option<(u32, u32)>,
    /// Bumped on every change of the document; with the library's revision, the placements' key.
    generation: u64,
    placements: Option<(u64, u64, Vec<Placement>)>,
}

impl Collage {
    /// Whether a collage is open in the room.
    pub fn is_open(&self) -> bool {
        self.room.is_some()
    }

    /// The steps of the room's history that can be undone and redone.
    pub fn history_depth(&self) -> (usize, usize) {
        self.room.as_ref().map_or((0, 0), |r| (r.undo.len(), r.redo.len()))
    }

    /// The open collage's document.
    pub fn document(&self) -> Option<&Doc> {
        self.room.as_ref().map(|r| &r.doc)
    }

    /// Whether the open collage has changes not yet written (Close asks first).
    pub fn is_changed(&self) -> bool {
        self.room.as_ref().is_some_and(|r| r.doc != r.baseline)
    }

    /// The display's size the window last gave the room.
    pub fn display(&self) -> Option<(u32, u32)> {
        self.room.as_ref().and_then(|r| r.display)
    }

    /// Opens the room on `doc` (an empty history, no unsaved changes), replacing any open one.
    pub(crate) fn open(&mut self, doc: Doc, display: Option<(u32, u32)>) {
        self.room = Some(Room { baseline: doc.clone(), doc, undo: Vec::new(), redo: Vec::new(), display, generation: 0, placements: None });
    }

    /// Closes the room, its document and history gone.
    pub(crate) fn close(&mut self) {
        self.room = None;
    }

    /// Replaces the document as one step of the room's history; answers whether it changed.
    pub(crate) fn change(&mut self, doc: Doc) -> bool {
        let Some(r) = self.room.as_mut() else { return false };
        if r.doc == doc {
            return false;
        }
        let before = std::mem::replace(&mut r.doc, doc);
        r.undo.push(before);
        if r.undo.len() > MAX_HISTORY {
            r.undo.remove(0);
        }
        r.redo.clear();
        r.generation = r.generation.wrapping_add(1);
        true
    }

    /// Undoes the room's last step; answers whether there was one.
    pub(crate) fn undo(&mut self) -> bool {
        let Some(r) = self.room.as_mut() else { return false };
        let Some(doc) = r.undo.pop() else { return false };
        r.redo.push(std::mem::replace(&mut r.doc, doc));
        r.generation = r.generation.wrapping_add(1);
        true
    }

    /// Redoes the room's last undone step; answers whether there was one.
    pub(crate) fn redo(&mut self) -> bool {
        let Some(r) = self.room.as_mut() else { return false };
        let Some(doc) = r.redo.pop() else { return false };
        r.undo.push(std::mem::replace(&mut r.doc, doc));
        r.generation = r.generation.wrapping_add(1);
        true
    }

    /// The document as it is now counts as written (Create Collage and Save Draft, M14.17): Close
    /// no longer asks until the next change.
    pub fn mark_saved(&mut self) {
        if let Some(r) = self.room.as_mut() {
            r.baseline = r.doc.clone();
        }
    }

    pub(crate) fn set_display(&mut self, display: (u32, u32)) {
        if let Some(r) = self.room.as_mut() {
            r.display = Some(display);
        }
    }
}

impl Session {
    /// The Collage room's state (`creating.md` §3).
    pub fn collage(&self) -> &Collage {
        &self.collage
    }

    /// Each clip's aspect ratio (width over height) by its position in the open collage's Clips:
    /// the photo as its recipe shows it (turns, frames and crop counted), from the model's size;
    /// `NaN` (laid out as square) for a size
    /// the model does not know or a photo no longer indexed. A clip's model size is its poster
    /// frame's (the scan turns it as the player does).
    pub fn collage_aspects(&self) -> Vec<f64> {
        let Some(doc) = self.collage.document() else { return Vec::new() };
        let lib = self.library();
        doc.clips
            .iter()
            .map(|c| match lib.photo(c.id) {
                Some(p) => {
                    let (w, h) = dalica_edit::shown_size(&lib.info(c.id).edits, p.width, p.height, Cropping::Counted);
                    if w == 0 || h == 0 { f64::NAN } else { f64::from(w) / f64::from(h) }
                }
                None => f64::NAN,
            })
            .collect()
    }

    /// The photos of `ids` or `section` a collage takes, in order (`create.collage`,
    /// `collage.clips.add`): the slideshow's stills (hidden photos left out), and the clips with a
    /// poster frame where they stand; the other clips are left out and counted (the second
    /// value), for M15.16's notice.
    pub(crate) fn collage_photos(&mut self, section: Option<&str>, ids: Option<&[PhotoId]>) -> Result<(Vec<PhotoId>, usize), CmdError> {
        let list = crate::cmd::slideshow::playlist(self, section, ids, None, false)?;
        if list.videos == 0 {
            return Ok((list.photos, 0));
        }
        // The order the stills came in, with the clips where they stand.
        let order: Vec<PhotoId> = match (ids, section) {
            (Some(ids), _) => ids.to_vec(),
            (None, Some(text)) => match text.parse::<Section>() {
                Ok(Section::Album(AlbumRef::Search)) => self.search_results_now()?.unwrap_or_default(),
                Ok(section) => {
                    let options = self.search_tree_options();
                    dalica_library::section_photos(self.library(), std::slice::from_ref(&section), &options)
                        .into_iter()
                        .flat_map(|(_, ids)| ids)
                        .collect()
                }
                Err(_) => Vec::new(),
            },
            (None, None) => Vec::new(),
        };
        let hidden_shown = self.search_tree_options().hidden_pictures;
        let stills: HashSet<PhotoId> = list.photos.iter().copied().collect();
        let lib = self.library();
        let mut seen = HashSet::with_capacity(order.len());
        let (mut out, mut playable) = (Vec::with_capacity(order.len()), 0usize);
        for id in order {
            if !seen.insert(id) {
                continue;
            }
            if stills.contains(&id) {
                out.push(id);
                continue;
            }
            let Some(p) = lib.photo(id) else { continue };
            let hidden = lib.is_hidden(id) || lib.is_folder_hidden(p.folder);
            if (hidden_shown || !hidden) && has_poster(lib, id) {
                out.push(id);
                playable += 1;
            }
        }
        Ok((out, list.videos.saturating_sub(playable)))
    }

    /// The open collage's placements, bottom first (`create::collage::layout`): laid out again only
    /// when the document or the library changed since the last ask.
    pub fn collage_placements(&mut self) -> Vec<Placement> {
        let revision = self.library().revision();
        let Some(generation) = self.collage.room.as_ref().map(|r| r.generation) else { return Vec::new() };
        if let Some((g, rev, p)) = self.collage.room.as_ref().and_then(|r| r.placements.as_ref())
            && *g == generation
            && *rev == revision
        {
            return p.clone();
        }
        let aspects = self.collage_aspects();
        let Some(room) = self.collage.room.as_mut() else { return Vec::new() };
        let placements = layout(&room.doc, &aspects);
        room.placements = Some((generation, revision, placements.clone()));
        placements
    }
}
