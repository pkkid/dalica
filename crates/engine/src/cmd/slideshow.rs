//! What a slideshow plays, and its photos ready before their transitions (`viewing.md` §1.1–§1.2,
//! M11.11).
//!
//! - `slideshow.list {section?, ids?, search?, start?}` (a query): the photos to play, in the
//!   display order of a light-table section (a folder's visible photos in its sort, an album in its
//!   order, `album:search` the search mode's matches), of a search (`search`: text in the search
//!   box's grammar, its matches in light-table order) or as given (`ids`: the tray, its order kept,
//!   duplicates dropped); hidden photos only while the view shows hidden pictures; videos left out
//!   and counted (Dalica does not play them yet); and the place to start from: `start`'s place
//!   when it is in the list, else the first.
//! - [`Slideshow`]: the show the UI plays, M11.4's [`Show`] on the session's clock over that list,
//!   with the previews it needs made ahead through the preview's two panes at the display's size:
//!   the photo shown in one pane; the next photo opened in the other as soon as no change is under
//!   way (during a change that pane still holds the outgoing photo); the next two prefetched
//!   ([`Session::prefetch_previews`]) the moment a photo is shown. So the next photo's screen-size
//!   preview is made during the whole display time of the one before it, and a transition never
//!   waits on the disk. A photo that has left the library is skipped when the show reaches it, and
//!   the one after it is the one made ready. [`Slideshow::stats`] counts the changes and those
//!   whose incoming photo was not ready when they began.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_create::slideshow::{self as create, Show, View};
use dalica_library::search::{Filters, Query as SearchQuery};
use dalica_library::{AlbumRef, Section, section_photos};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::search::check_text;
use crate::error::CmdError;
use crate::preview::PANES;
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;
use crate::settings::{SlideshowSettings, Transition};

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "slideshow.list",
        label: "Slideshow Photos",
        menu: "",
        keys: &[],
        params: r#"{"section"?:"folder:<id>"|"album:<id>"|"album:starred"|"album:recent"|"album:search"|"person:<id>" (its photos in display order) | "ids"?:[PhotoId] (the tray, in its order) | "search"?:string (the search box's grammar; its matches in light-table order) (exactly one of the three), "start"?:PhotoId (the photo to start from)}: the photos a slideshow plays: {photos:[PhotoId], start: the place of start in photos (0 when it is not there), videos: how many videos were left out}; hidden photos only while the view shows hidden pictures"#,
        effect: Effect::Query,
        history: History::None,
        enabled: has_target,
        run: list,
    }]
}

/// The most photos named at once.
const MAX_IDS: usize = 100_000;
/// The file extensions of videos (as the scan lists them).
const VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "mov", "m4v", "mkv"];
const NO_TARGET: &str = "Select a folder or a photo first.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListParams {
    section: Option<String>,
    ids: Option<Vec<PhotoId>>,
    search: Option<String>,
    start: Option<PhotoId>,
}

fn has_target(_: &Session, p: &Value) -> Result<(), Disabled> {
    let given = |k: &str| p.get(k).is_some_and(|v| !v.is_null());
    if given("section") || given("ids") || given("search") { Ok(()) } else { Err(Disabled(NO_TARGET.into())) }
}

/// The photos a slideshow plays, videos left out, and how many videos were.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Playlist {
    pub photos: Vec<PhotoId>,
    pub videos: usize,
}

/// Whether a file name is a video's (by its extension; the scan's facts say so too).
fn is_video(name: &str) -> bool {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    VIDEO_EXTENSIONS.contains(&ext.as_str())
}

/// The photos of `section` in its display order (hidden ones only while shown).
fn section_list(s: &mut Session, text: &str) -> Result<Vec<PhotoId>, CmdError> {
    let section: Section = text.parse().map_err(|e: dalica_library::Error| CmdError::BadParams(e.to_string()))?;
    let lib = s.library();
    match &section {
        Section::Album(AlbumRef::Search) => {
            return s.search_results_now()?.ok_or_else(|| CmdError::Disabled("There are no search results.".into()));
        }
        Section::Folder(f) if lib.folder(*f).is_none() => return Err(CmdError::NotFound(format!("no folder {f}"))),
        Section::Album(AlbumRef::User(a)) if lib.album(a).is_none() => return Err(CmdError::NotFound(format!("no album {a}"))),
        Section::Person(p) if lib.person(p).is_none() => return Err(CmdError::NotFound(format!("no person {p}"))),
        _ => {}
    }
    let options = s.search_tree_options();
    Ok(section_photos(s.library(), std::slice::from_ref(&section), &options).into_iter().flat_map(|(_, ids)| ids).collect())
}

/// What a slideshow of `section`, `ids` or `search` plays (exactly one of them).
pub fn playlist(s: &mut Session, section: Option<&str>, ids: Option<&[PhotoId]>, search: Option<&str>) -> Result<Playlist, CmdError> {
    let given = usize::from(section.is_some()) + usize::from(ids.is_some()) + usize::from(search.is_some());
    if given != 1 {
        return Err(CmdError::BadParams("name the photos by exactly one of section, ids or search".into()));
    }
    let hidden_pictures = s.search_tree_options().hidden_pictures;
    let found: Vec<PhotoId> = if let Some(text) = section {
        section_list(s, text)?
    } else if let Some(text) = search {
        check_text(text)?;
        let query = SearchQuery { text: text.to_string(), filters: Filters::default(), age: None };
        s.ask_search(query)?.photos().to_vec()
    } else {
        let ids = ids.unwrap_or_default();
        if ids.len() > MAX_IDS {
            return Err(CmdError::Limit(format!("at most {MAX_IDS} photos at once")));
        }
        let lib = s.library();
        let mut seen = HashSet::with_capacity(ids.len());
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let p = lib.photo(*id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
            let hidden = lib.is_hidden(p.id) || lib.is_folder_hidden(p.folder);
            if seen.insert(*id) && (hidden_pictures || !hidden) {
                out.push(*id);
            }
        }
        out
    };
    let lib = s.library();
    let mut list = Playlist::default();
    for id in found {
        let Some(p) = lib.photo(id) else { continue };
        if lib.facts(id).video || is_video(&p.name) {
            list.videos += 1;
        } else {
            list.photos.push(id);
        }
    }
    Ok(list)
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let ListParams { section, ids, search, start } = params(p)?;
    let list = playlist(s, section.as_deref(), ids.as_deref(), search.as_deref())?;
    let start = start.and_then(|id| list.photos.iter().position(|p| *p == id)).unwrap_or(0);
    Ok(json!({ "photos": list.photos, "start": start, "videos": list.videos }))
}

// --- The show and its previews --------------------------------------------------------------------

/// The show's settings from Options → Slideshow.
pub fn show_settings(settings: &SlideshowSettings) -> create::Settings {
    let transition = match settings.transition {
        Transition::Cut => create::Transition::Cut,
        Transition::PanAndZoom => create::Transition::PanAndZoom,
        Transition::Dissolve => create::Transition::Dissolve,
        Transition::Wipe => create::Transition::Wipe,
        Transition::Circle => create::Transition::Circle,
        Transition::Push => create::Transition::Push,
        Transition::Rectangle => create::Transition::Rectangle,
    };
    create::Settings { display_secs: settings.display_time, transition, looped: settings.looping }
}

/// What the UI draws at one moment of the show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The show's state ([`View::index`] is the photo's place in the list).
    pub view: View,
    /// The photo shown, and the preview pane holding it; `None` when the show has no photo left.
    pub photo: Option<(PhotoId, usize)>,
    /// While a change is under way: the outgoing photo and its pane.
    pub outgoing: Option<(PhotoId, usize)>,
    /// Whether the shown photo's screen-size preview is in its pane (else the UI draws the best
    /// thumbnail it has meanwhile).
    pub ready: bool,
}

/// Counts of the show's changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShowStats {
    /// Changes from one photo to another.
    pub changes: u64,
    /// Changes whose incoming photo's preview was not ready when they began.
    pub waits: u64,
}

/// A slideshow being played: the list, M11.4's [`Show`] on the session's clock, and the preview
/// panes it keeps (module docs). The UI owns it, sends the show its input through
/// [`Slideshow::show_mut`] and calls [`Slideshow::tick`] every frame; [`Slideshow::close`] gives
/// the panes back.
pub struct Slideshow {
    show: Show,
    photos: Vec<PhotoId>,
    /// The display's size in device pixels: every preview is made to cover it.
    fit: (u32, u32),
    /// The photo each pane was opened on.
    panes: [Option<PhotoId>; PANES],
    /// The photo shown and its pane.
    current: Option<(PhotoId, usize)>,
    /// The photo going out and its pane, while a change is under way.
    outgoing: Option<(PhotoId, usize)>,
    /// The place shown at the last tick.
    last_index: Option<usize>,
    /// What was prefetched last.
    prefetched: Vec<PhotoId>,
    stats: ShowStats,
}

impl std::fmt::Debug for Slideshow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slideshow").field("photos", &self.photos.len()).field("show", &self.show).field("stats", &self.stats).finish_non_exhaustive()
    }
}

impl Slideshow {
    /// A show of `photos` from place `start`, with Options → Slideshow's settings, its previews
    /// made to cover `fit` (the display, in device pixels). Its first photo is opened at once.
    pub fn start(s: &mut Session, photos: Vec<PhotoId>, start: usize, fit: (u32, u32)) -> Slideshow {
        let settings = show_settings(&s.settings().slideshow());
        Slideshow::with_settings(s, photos, start, settings, fit)
    }

    /// [`Slideshow::start`] with the settings given.
    pub fn with_settings(s: &mut Session, photos: Vec<PhotoId>, start: usize, settings: create::Settings, fit: (u32, u32)) -> Slideshow {
        let show = Show::new(Arc::clone(&s.clock), photos.len(), start, settings);
        let mut me = Slideshow {
            show,
            photos,
            fit,
            panes: [None; PANES],
            current: None,
            outgoing: None,
            last_index: None,
            prefetched: Vec::new(),
            stats: ShowStats::default(),
        };
        me.tick(s);
        me
    }

    /// The show, for its input (keys, mouse, the bar) and its settings.
    pub fn show_mut(&mut self) -> &mut Show {
        &mut self.show
    }

    pub fn show(&self) -> &Show {
        &self.show
    }

    /// The photos played.
    pub fn photos(&self) -> &[PhotoId] {
        &self.photos
    }

    pub fn stats(&self) -> ShowStats {
        self.stats
    }

    /// The display changed size: the previews are made again to cover `fit`.
    pub fn set_fit(&mut self, s: &mut Session, fit: (u32, u32)) {
        if self.fit == fit {
            return;
        }
        self.fit = fit;
        for pane in 0..PANES {
            if self.panes.get(pane).copied().flatten().is_some() {
                // A pane closed meanwhile is opened again by the next tick.
                let _ = s.set_preview_fit_in(pane, fit);
            }
        }
        self.prefetched.clear();
    }

    /// Ends the show: both panes are closed and the prefetch dropped.
    pub fn close(self, s: &mut Session) {
        s.prefetch_previews(&[]);
        s.close_preview();
    }

    fn present_at(&self, s: &Session, i: usize) -> bool {
        self.photos.get(i).is_some_and(|id| s.library().photo(*id).is_some())
    }

    /// The places of the next present photos after place `i`, at most `n` (wrapping with Loop).
    fn upcoming(&self, s: &Session, i: usize, n: usize) -> Vec<usize> {
        let len = self.photos.len();
        let looped = self.show.settings().looped;
        let mut out = Vec::with_capacity(n);
        let mut at = i;
        for _ in 0..len {
            at += 1;
            if at >= len {
                if !looped {
                    break;
                }
                at = 0;
            }
            if at == i || out.len() == n {
                break;
            }
            if self.present_at(s, at) {
                out.push(at);
            }
        }
        out
    }

    /// The show reached a photo that has left the library: it moves on to the next present one
    /// in the direction it was going (else the other way), at most once round the list.
    fn skip_gone(&mut self, s: &Session) {
        let Some(mut index) = self.show.index() else { return };
        if self.present_at(s, index) {
            return;
        }
        let back = self.last_index.is_some_and(|last| index < last && !(index == 0 && last + 1 == self.photos.len()));
        for backwards in [back, !back] {
            for _ in 0..self.photos.len() {
                let moved = if backwards { self.show.previous_photo() } else { self.show.next_photo() };
                match self.show.index() {
                    Some(i) if moved && i != index => index = i,
                    _ => break,
                }
                if self.present_at(s, index) {
                    return;
                }
            }
        }
    }

    /// Whether pane `pane` shows photo `id`'s preview.
    fn ready_in(&self, s: &Session, pane: usize, id: PhotoId) -> bool {
        s.preview_in(pane).is_some_and(|f| f.photo == id)
    }

    /// Opens photo `id` in pane `pane` (a prefetched preview shows at once).
    fn open_in(&mut self, s: &mut Session, pane: usize, id: PhotoId) {
        if s.open_preview_in(pane, id, self.fit).is_ok()
            && let Some(slot) = self.panes.get_mut(pane)
        {
            *slot = Some(id);
        }
    }

    /// Whether pane `pane` is open on photo `id`, as the show opened it.
    fn holds(&self, s: &Session, pane: usize, id: PhotoId) -> bool {
        self.panes.get(pane).copied().flatten() == Some(id) && s.preview_photo_in(pane) == Some(id)
    }

    /// Brings the show up to the clock's time and its previews up to the photo shown: answers what
    /// to draw.
    pub fn tick(&mut self, s: &mut Session) -> Frame {
        let mut view = self.show.tick();
        self.skip_gone(s);
        if self.show.index() != view.index {
            view = self.show.view();
        }
        let index = view.index.filter(|i| self.present_at(s, *i));
        let photo = index.and_then(|i| self.photos.get(i).copied());
        if photo != self.current.map(|(id, _)| id) {
            let previous = self.current.take();
            let pane = photo.map(|id| match (0..PANES).find(|p| self.holds(s, *p, id)) {
                Some(pane) => pane,
                None => {
                    // Not the outgoing photo's pane: it is drawn from until the change ends.
                    let pane = previous.map_or(0, |(_, p)| (p + 1) % PANES);
                    self.open_in(s, pane, id);
                    pane
                }
            });
            self.current = photo.zip(pane);
            if let (Some((id, pane)), Some(_)) = (self.current, previous) {
                self.stats.changes += 1;
                if !self.ready_in(s, pane, id) {
                    self.stats.waits += 1;
                }
            }
            self.outgoing = previous;
            if let Some((_, pane)) = self.current {
                // Its jobs go before the next photo's.
                let _ = s.set_preview_focus(pane);
            }
        }
        if view.change.is_none() {
            self.outgoing = None;
        }
        self.last_index = view.index;
        if let Some((id, pane)) = self.current {
            let places = index.map(|i| self.upcoming(s, i, 2)).unwrap_or_default();
            let upcoming: Vec<PhotoId> = places.iter().filter_map(|i| self.photos.get(*i).copied()).collect();
            if upcoming != self.prefetched {
                s.prefetch_previews(&upcoming);
                self.prefetched = upcoming.clone();
            }
            // The next photo goes into the other pane once nothing is drawn from it.
            let other = (pane + 1) % PANES;
            if self.outgoing.is_none()
                && let Some(next) = upcoming.first().copied().filter(|n| *n != id)
                && !self.holds(s, other, next)
            {
                self.open_in(s, other, next);
            }
        }
        let ready = self.current.is_some_and(|(id, pane)| self.ready_in(s, pane, id));
        Frame { view, photo: self.current, outgoing: self.outgoing, ready }
    }
}
