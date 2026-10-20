//! What a slideshow plays, and its photos ready before their transitions (`viewing.md` §1.1–§1.2,
//! M11.11).
//!
//! - `slideshow.list {section?, ids?, search?, start?}` (a query): the photos to play, in the
//!   display order of a light-table section (a folder's visible photos in its sort, an album in its
//!   order, `album:search` the search mode's matches), of a search (`search`: text in the search
//!   box's grammar, its matches in light-table order) or as given (`ids`: the tray, its order kept,
//!   duplicates dropped); hidden photos only while the view shows hidden pictures; videos are
//!   played like photos (M15.27) and counted (`videos`). `withholdNames` leaves people's names and
//!   nicknames out of a `search`'s match, as `library.query`'s (automation without the people
//!   permission always sends it: `automation::NAME_SEARCHES`). And the place to start from: `start`'s place
//!   when it is in the list, else the first. For each photo with a face that shows (after its crop)
//!   it gives the face Pan and Zoom moves toward ([`focus_of`]): the largest named face, else the
//!   largest unnamed one (ignored faces never), and its centre on the picture shown. Automation
//!   without the people permission gets each photo's focus as its centre and no face
//!   (`automation::FACE_POINTS`); the window's own show still moves toward the face.
//! - [`Slideshow`]: the show the UI plays, M11.4's [`Show`] on the session's clock over that list,
//!   with the previews it needs made ahead through the preview's two panes at the display's size:
//!   the photo shown in one pane; the next photo opened in the other as soon as no change is under
//!   way (during a change that pane still holds the outgoing photo); the next two prefetched
//!   ([`Session::prefetch_previews`]) the moment a photo is shown. So the next photo's screen-size
//!   preview is made during the whole display time of the one before it, and a transition never
//!   waits on the disk. A photo that has left the library is skipped when the show reaches it, and
//!   the one after it is the one made ready. [`Slideshow::stats`] counts the changes and those
//!   whose incoming photo was not ready when they began. Each [`Frame`] carries the Pan and Zoom
//!   focus of the photo shown and of the outgoing one ([`focus_of`], worked out again only when the
//!   library changes).
//! - **Videos** (`viewing.md` §1.1, M15.27): a video Dalica plays ([`crate::video::playback`] by its
//!   facts) plays to its end (its trim respected) before the show moves on: the show holds it
//!   ([`Show::set_held`]) from the moment it is shown, its frames are decoded on a worker of its own
//!   ([`Clip`], at most [`crate::video::PLAY_SIDE`] on the long side) and timed by the session's
//!   clock, and when it reaches its end the show goes on to the next photo at once. Its preview
//!   pane holds its poster frame (M15.20), shown until its first frame is there; the frame due is
//!   [`clip_picture`] of the [`Frame::clip`]'s key. Pausing the show pauses the clip. It plays
//!   muted unless Play video sound is on; with it on, a clip whose sound Dalica decodes (AAC) plays
//!   it through the session's [`Sound`] at the video volume (the music, if any, pausing meanwhile),
//!   and one whose sound it cannot decode plays muted and says why ([`Slideshow::clip_state`]). A
//!   video Dalica cannot play, or one whose decoder fails, stays as its tile for the display time.
//! - **Music** (`viewing.md` §1.5, M15.27): with Play music during slideshows on and a music folder
//!   set, a [`MusicPlayer`] plays its files in name order, looping, from the show's start; it pauses
//!   while the show is paused and while a clip's sound plays, and fades out when the show ends (at
//!   its last photo without Loop, or closed). A folder's own music (Folder Properties) is not
//!   stored yet: the music folder of Options ▸ Slideshow plays for every show.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use dalica_core::{Clock, PhotoId, Timestamp};
use dalica_create::slideshow::{self as create, FaceBox, Point, Show, State, View};
use dalica_edit::{Geometry, Recipe};
use dalica_image::Limits;
use dalica_image::Orientation;
use dalica_image::formats::video::SoundPlay;
use dalica_image::formats::video::frames::{Frame as VideoFrame, FrameOptions, FrameSource};
use dalica_image::formats::video::sound::SoundSource;
use dalica_library::FaceRef;
use dalica_library::search::{Filters, People, Query as SearchQuery};
use dalica_library::{AlbumRef, Section, section_photos};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::crop::shown_rect;
use crate::cmd::faces::face_id;
use crate::cmd::search::check_text;
use crate::error::CmdError;
use crate::preview::PANES;
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;
use crate::settings::{SlideshowSettings, Transition};
use crate::sound::{Format, MusicPlayer, Sound};

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "slideshow.list",
        label: "Slideshow Photos",
        menu: "",
        keys: &[],
        params: r#"{"section"?:"folder:<id>"|"album:<id>"|"album:starred"|"album:recent"|"album:search"|"person:<id>" (its photos in display order) | "ids"?:[PhotoId] (the tray, in its order) | "search"?:string (the search box's grammar; its matches in light-table order) (exactly one of the three), "start"?:PhotoId (the photo to start from), "withholdNames"?:bool (default false: people's names and nicknames take no part in search's match; automation without --automation-allow-people always sends true)}: the photos a slideshow plays: {photos:[PhotoId] (videos among them, played to their end), start: the place of start in photos (0 when it is not there), videos: how many of photos are videos, focus: {"<PhotoId>": {face:"<PhotoId>.<n>", point:[x, y]}} (for each photo with a face that shows: the face Pan and Zoom moves toward, the largest named one else the largest, and its centre normalised to the picture shown; automation without --automation-allow-people gets each photo's focus as {point:[0.5, 0.5]}, its centre, and no face)}; hidden photos only while the view shows hidden pictures"#,
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
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ListParams {
    section: Option<String>,
    ids: Option<Vec<PhotoId>>,
    search: Option<String>,
    start: Option<PhotoId>,
    #[serde(default)]
    withhold_names: bool,
}

fn has_target(_: &Session, p: &Value) -> Result<(), Disabled> {
    let given = |k: &str| p.get(k).is_some_and(|v| !v.is_null());
    if given("section") || given("ids") || given("search") { Ok(()) } else { Err(Disabled(NO_TARGET.into())) }
}

/// What a slideshow plays: its stills, everything in order (videos where they stand), and how many
/// videos there are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Playlist {
    /// The stills, in order (what a collage takes).
    pub photos: Vec<PhotoId>,
    /// The stills and the videos, in order: what the show plays.
    pub played: Vec<PhotoId>,
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

/// What a slideshow of `section`, `ids` or `search` plays (exactly one of them); with
/// `withhold_names` people's names take no part in the search's match.
pub fn playlist(
    s: &mut Session,
    section: Option<&str>,
    ids: Option<&[PhotoId]>,
    search: Option<&str>,
    withhold_names: bool,
) -> Result<Playlist, CmdError> {
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
        s.ask_search_with(query, People { withhold_names, ..People::default() })?.photos().to_vec()
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
        list.played.push(id);
    }
    Ok(list)
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let ListParams { section, ids, search, start, withhold_names } = params(p)?;
    let list = playlist(s, section.as_deref(), ids.as_deref(), search.as_deref(), withhold_names)?;
    let start = start.and_then(|id| list.played.iter().position(|p| *p == id)).unwrap_or(0);
    let mut focus = serde_json::Map::new();
    for id in &list.photos {
        if let Some((face, point)) = focus_of(s, *id) {
            // Six decimals, as every normalised value Dalica answers.
            let n = |v: f32| (f64::from(v) * 1e6).round() / 1e6;
            focus.insert(id.to_string(), json!({ "face": face_id(face), "point": [n(point.x), n(point.y)] }));
        }
    }
    Ok(json!({ "photos": list.played, "start": start, "videos": list.videos, "focus": focus }))
}

/// The face Pan and Zoom moves photo `id` toward and its centre, normalised to the picture shown
/// (the photo's recipe applied: crop, straighten, frames and turns): the largest named face that
/// shows, else the largest unnamed one; ignored faces never count. `None` for a photo without such
/// a face (Pan and Zoom then moves toward its centre) or whose size the library does not know.
pub fn focus_of(s: &Session, id: PhotoId) -> Option<(FaceRef, Point)> {
    let lib = s.library();
    let faces = lib.faces(id);
    if !faces.iter().any(|f| f.counts()) {
        return None;
    }
    let photo = lib.photo(id).filter(|p| p.width > 0 && p.height > 0)?;
    let recipe = Recipe::parse(&lib.info(id).edits).unwrap_or_default();
    let geometry = Geometry::new(&recipe, Orientation::default(), photo.width, photo.height);
    let mut boxes = Vec::with_capacity(faces.len());
    let mut places = Vec::with_capacity(faces.len());
    for (index, face) in faces.iter().enumerate().filter(|(_, f)| f.counts()) {
        if let Some(r) = shown_rect(&geometry, face.rect) {
            let rect = create::Rect::new(r.x0 as f32, r.y0 as f32, (r.x1 - r.x0) as f32, (r.y1 - r.y0) as f32);
            boxes.push(FaceBox { rect, named: face.person.person().is_some() });
            places.push(index);
        }
    }
    let chosen = create::focus_face(&boxes)?;
    let index = places.get(chosen).copied()?;
    Some((FaceRef { photo: id, index }, create::face_focus(&boxes)))
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
    /// The point Pan and Zoom moves the photo shown toward, normalised to it ([`focus_of`]; the
    /// centre without a face): the [`create::Slide`]'s `focus`.
    pub focus: Point,
    /// The same for the outgoing photo (the centre when there is none).
    pub outgoing_focus: Point,
    /// The video playing in place of the photo shown, while it is one Dalica plays.
    pub clip: Option<ClipView>,
}

/// A video of the show as the UI draws it: the frame due is [`clip_picture`]`(key)` (its poster in
/// the photo's pane stands in until there is one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipView {
    /// The clip's key for [`clip_picture`].
    pub key: u64,
    pub photo: PhotoId,
    /// Where it is, in milliseconds from the clip's start.
    pub position_ms: u64,
    /// The number of the frame due (`None` before the first is decoded).
    pub frame: Option<usize>,
    /// Whether it is playing (not paused, not yet opening).
    pub playing: bool,
    /// Whether its sound plays.
    pub sound: bool,
}

/// What the show says of the video shown, for inspection and tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipState {
    pub photo: PhotoId,
    pub ready: bool,
    pub playing: bool,
    pub position_ms: u64,
    /// Where it stops: its trim's end, else its length (0 until known).
    pub end_ms: u64,
    /// Whether its sound plays.
    pub sound: bool,
    /// Why it plays without sound while Play video sound is on ("This clip plays without sound
    /// (AC-3 sound can't be played)").
    pub muted: Option<String>,
    /// Why it stays as its tile (its decoder failed).
    pub failed: Option<String>,
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
    /// The Pan and Zoom focus of the photos drawn lately, with the library revision it was worked
    /// out at (at most [`FOCUS_KEPT`]).
    focus: Vec<(PhotoId, u64, Point)>,
    /// The video shown, while the photo shown is one Dalica plays.
    clip: Option<Clip>,
    /// Play video sound, as the show started.
    video_sound: bool,
    /// The music folder, while Play music is on.
    music_folder: Option<PathBuf>,
    music: Option<MusicPlayer>,
    /// The output the music and the clips' sound play through.
    sound: Arc<dyn Sound>,
}

/// How many photos' Pan and Zoom focus a show keeps: the photo shown, the outgoing one, and room.
const FOCUS_KEPT: usize = 4;

impl std::fmt::Debug for Slideshow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slideshow")
            .field("photos", &self.photos.len())
            .field("show", &self.show)
            .field("stats", &self.stats)
            .field("clip", &self.clip.as_ref().map(|c| c.id))
            .finish_non_exhaustive()
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
        let sound = s.settings().slideshow();
        let music_folder = sound.music_folder.filter(|_| sound.music);
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
            focus: Vec::new(),
            clip: None,
            video_sound: sound.video_sound,
            music_folder,
            music: None,
            sound: s.sound(),
        };
        me.start_music(s);
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

    /// Ends the show: both panes are closed and the prefetch dropped, the clip stopped and the
    /// music faded out.
    pub fn close(mut self, s: &mut Session) {
        self.stop_clip(s);
        if let Some(music) = self.music.take() {
            music.fade_out();
        }
        s.prefetch_previews(&[]);
        s.close_preview();
    }

    /// The music playing, if any (for inspection and tests).
    pub fn music(&self) -> Option<&MusicPlayer> {
        self.music.as_ref()
    }

    /// The video shown, if the photo shown is one Dalica plays.
    pub fn clip_state(&self, s: &Session) -> Option<ClipState> {
        self.clip.as_ref().map(|c| c.state(s.clock.as_ref()))
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

    /// Brings the show up to the clock's time and its previews up to the photo shown, its video
    /// and its music up to the show: answers what to draw.
    pub fn tick(&mut self, s: &mut Session) -> Frame {
        let mut frame = self.tick_photos(s);
        if self.tick_clip(s, &frame) {
            // The video ended and the show moved on.
            frame = self.tick_photos(s);
            self.tick_clip(s, &frame);
        }
        self.tick_music();
        frame.clip = self.clip.as_mut().map(|c| c.view(s.clock.as_ref()));
        if frame.clip.is_some() {
            frame.view.motion = 0.0;
        }
        frame
    }

    fn tick_photos(&mut self, s: &mut Session) -> Frame {
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
        let focus = self.current.map_or(Point::CENTRE, |(id, _)| self.focus(s, id));
        let outgoing_focus = self.outgoing.map_or(Point::CENTRE, |(id, _)| self.focus(s, id));
        Frame { view, photo: self.current, outgoing: self.outgoing, ready, focus, outgoing_focus, clip: None }
    }

    /// The clip of the photo shown: started when a video Dalica plays is shown (the show holding
    /// it), played while the show plays, and at its end the show moves on. Answers whether it
    /// moved on.
    fn tick_clip(&mut self, s: &mut Session, frame: &Frame) -> bool {
        let shown = frame.photo.map(|(id, _)| id);
        if self.clip.as_ref().map(|c| c.id) != shown {
            self.stop_clip(s);
            if let Some(id) = shown
                && let Some(clip) = Clip::start(s, id, self.video_sound)
            {
                self.show.set_held(true);
                self.clip = Some(clip);
            }
        }
        let clock = Arc::clone(&s.clock);
        let Some(clip) = self.clip.as_mut() else { return false };
        clip.sync(clock.as_ref());
        if clip.failed().is_some() {
            if self.show.held() {
                // Its tile stays for the rest of the display time.
                self.show.set_held(false);
            }
            return false;
        }
        let should_play = self.show.state() == State::Playing;
        if should_play && !clip.is_playing() && clip.is_ready() && !clip.finished {
            clip.play(s, self.music.as_ref());
        } else if !should_play && clip.is_playing() {
            clip.pause(s, self.music.as_ref());
        }
        if clip.is_playing() && clip.at_end(clock.as_ref()) {
            clip.finished = true;
            clip.pause(s, self.music.as_ref());
            self.show.set_held(false);
            self.show.next_photo();
            return true;
        }
        false
    }

    /// Stops the clip shown (its sound closed, the music resumed).
    fn stop_clip(&mut self, s: &mut Session) {
        if let Some(mut clip) = self.clip.take() {
            clip.pause(s, self.music.as_ref());
        }
    }

    /// Starts the music from the start of its folder, when Play music is on.
    fn start_music(&mut self, _: &Session) {
        self.music = self.music_folder.clone().and_then(|folder| MusicPlayer::start(folder, Arc::clone(&self.sound)));
    }

    /// The music follows the show: paused while it is paused (or a clip's sound plays), faded out
    /// at its end, started again when Play starts the show over.
    fn tick_music(&mut self) {
        let state = self.show.state();
        let clip_sound = self.clip.as_ref().is_some_and(|c| c.sound_open.is_some());
        let Some(music) = self.music.as_ref() else { return };
        if state == State::Ended {
            if !music.is_done() {
                music.fade_out();
            }
            return;
        }
        if music.is_done() && music.ended().is_none() {
            // Faded at the end, and the show plays again.
            self.music = None;
            if let Some(folder) = self.music_folder.clone() {
                self.music = MusicPlayer::start(folder, Arc::clone(&self.sound));
            }
            return;
        }
        let paused = state != State::Playing || clip_sound;
        if paused && !music.is_paused() {
            music.pause();
        } else if !paused && music.is_paused() {
            music.resume();
        }
    }

    /// Photo `id`'s Pan and Zoom focus ([`focus_of`]), worked out again only when the library has
    /// changed since.
    fn focus(&mut self, s: &Session, id: PhotoId) -> Point {
        let revision = s.library().revision();
        if let Some((_, _, p)) = self.focus.iter().find(|(f, r, _)| *f == id && *r == revision) {
            return *p;
        }
        let point = focus_of(s, id).map_or(Point::CENTRE, |(_, p)| p);
        self.focus.retain(|(f, r, _)| *f != id && *r == revision);
        if self.focus.len() >= FOCUS_KEPT {
            self.focus.remove(0);
        }
        self.focus.push((id, revision, point));
        point
    }
}

// --- A video in the show ------------------------------------------------------------------------------

/// Frames decoded ahead of the one due.
const CLIP_AHEAD: usize = 8;
/// How long a clip's worker waits for a request while it plays, and while it does not.
const CLIP_PUMP: Duration = Duration::from_millis(4);
const CLIP_IDLE: Duration = Duration::from_millis(100);

/// What a clip's owner and its worker share.
#[derive(Debug, Default)]
struct ClipShared {
    /// Bumped by every play and pause: the worker's frames of an older one are dropped.
    generation: u64,
    ready: bool,
    failed: Option<String>,
    /// The clip's sound format, when its sound plays.
    format: Option<Format>,
    /// Why it plays muted while video sound is on.
    muted: Option<String>,
    /// Its length from its frames, in milliseconds.
    frames_ms: u64,
    shown: Option<Arc<VideoFrame>>,
    ahead: VecDeque<(u64, Arc<VideoFrame>)>,
    /// While playing: the position play started from and when (the session's clock).
    play: Option<(u64, Timestamp)>,
    /// The stream its sound is queued into, while it plays with sound.
    stream: Option<u64>,
    end_ms: u64,
    volume: f32,
}

/// The position of a clip: where play started plus the time since, up to its end; where it is
/// paused otherwise.
fn clip_position(c: &ClipShared, anchor: u64, clock: &dyn Clock) -> u64 {
    match c.play {
        Some((from, since)) => {
            from.saturating_add(u64::try_from(clock.now().as_millis().saturating_sub(since.as_millis())).unwrap_or(0)).min(c.end_ms)
        }
        None => anchor,
    }
}

fn lock_clip(shared: &Mutex<ClipShared>) -> MutexGuard<'_, ClipShared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a clip's worker is asked.
#[derive(Debug)]
enum ClipRequest {
    /// Play from a position.
    Play { generation: u64, from_ms: u64 },
}

/// The clips of the shows playing, by key, for [`clip_picture`].
static CLIPS: Mutex<Vec<(u64, Weak<Mutex<ClipShared>>)>> = Mutex::new(Vec::new());
static NEXT_KEY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The frame due of the clip `key` names ([`ClipView::key`]), while that clip is shown.
pub fn clip_picture(key: u64) -> Option<Arc<VideoFrame>> {
    let clips = CLIPS.lock().unwrap_or_else(PoisonError::into_inner);
    let shared = clips.iter().find(|(k, _)| *k == key)?.1.upgrade()?;
    let c = lock_clip(&shared);
    c.shown.clone()
}

/// A video shown by the slideshow (module docs).
struct Clip {
    id: PhotoId,
    key: u64,
    shared: Arc<Mutex<ClipShared>>,
    /// To the worker; dropping it ends the worker.
    tx: Sender<ClipRequest>,
    sound: Arc<dyn Sound>,
    /// Where it starts (its trim's start) and where it is while paused.
    anchor_ms: u64,
    /// Whether its sound is to play.
    with_sound: bool,
    /// The stream it opened for its sound, while it plays with sound.
    sound_open: Option<u64>,
    /// It reached its end.
    finished: bool,
}

impl Clip {
    /// Starts showing video `id` when it is one Dalica plays (else `None`: it is shown as its
    /// tile): its worker opens it.
    fn start(s: &Session, id: PhotoId, video_sound: bool) -> Option<Clip> {
        let lib = s.library();
        if !crate::video::is_video(lib, id) {
            return None;
        }
        let facts = lib.video_facts(id).cloned().unwrap_or_default();
        let play = crate::video::playback(&facts);
        if play.video.is_err() {
            return None;
        }
        let photo = lib.photo(id)?;
        let path = lib.folder(photo.folder)?.path.join(&photo.name);
        let (start, end) = crate::video::trim_of(&lib.info(id).edits).unwrap_or((0, facts.duration_ms));
        let muted = if video_sound { play.muted().map(str::to_string) } else { None };
        let sound_plays = video_sound && matches!(play.sound, SoundPlay::Plays);
        let shared =
            Arc::new(Mutex::new(ClipShared { muted, end_ms: end, volume: f32::from(s.settings().video().volume) / 100.0, ..ClipShared::default() }));
        let (tx, rx) = mpsc::channel();
        let worker = ClipWorker { path, shared: Arc::clone(&shared), sound: s.sound(), clock: Arc::clone(&s.clock), rx };
        if let Err(e) = std::thread::Builder::new().name("dalica-slideshow-video".into()).spawn(move || worker.run(sound_plays, start)) {
            log::warn!("the slideshow's video could not open: {e}");
            return None;
        }
        let key = NEXT_KEY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut clips = CLIPS.lock().unwrap_or_else(PoisonError::into_inner);
        clips.retain(|(_, w)| w.strong_count() > 0);
        clips.push((key, Arc::downgrade(&shared)));
        Some(Clip { id, key, shared, tx, sound: s.sound(), anchor_ms: start, with_sound: sound_plays, sound_open: None, finished: false })
    }

    fn is_ready(&self) -> bool {
        lock_clip(&self.shared).ready
    }

    fn is_playing(&self) -> bool {
        lock_clip(&self.shared).play.is_some()
    }

    fn failed(&self) -> Option<String> {
        lock_clip(&self.shared).failed.clone()
    }

    /// Whether play has reached the clip's end.
    fn at_end(&self, clock: &dyn Clock) -> bool {
        let c = lock_clip(&self.shared);
        c.play.is_some() && c.end_ms > 0 && clip_position(&c, self.anchor_ms, clock) >= c.end_ms
    }

    /// Plays from where it is (its sound opened first when it plays; the music pauses for it).
    fn play(&mut self, s: &Session, music: Option<&MusicPlayer>) {
        let format = lock_clip(&self.shared).format;
        let mut stream = None;
        if let (true, Some(format)) = (self.with_sound, format) {
            if let Some(m) = music {
                m.pause();
            }
            match self.sound.open(format) {
                Ok(()) => stream = Some(self.sound.stream()),
                Err(e) => log::info!("the slideshow's video plays without sound: {e}"),
            }
        }
        self.sound_open = stream;
        let mut c = lock_clip(&self.shared);
        c.generation += 1;
        c.ahead.clear();
        c.stream = stream;
        c.play = Some((self.anchor_ms, s.clock.now()));
        let generation = c.generation;
        drop(c);
        let _ = self.tx.send(ClipRequest::Play { generation, from_ms: self.anchor_ms });
    }

    /// Pauses where it is: its sound closed (when it is still its stream) and the music resumed.
    fn pause(&mut self, s: &Session, music: Option<&MusicPlayer>) {
        let mut c = lock_clip(&self.shared);
        if c.play.is_some() {
            self.anchor_ms = clip_position(&c, self.anchor_ms, s.clock.as_ref());
        }
        c.play = None;
        c.stream = None;
        c.generation += 1;
        c.ahead.clear();
        drop(c);
        if let Some(stream) = self.sound_open.take() {
            if self.sound.stream() == stream {
                self.sound.stop();
            }
            if let Some(m) = music {
                m.resume();
            }
        }
    }

    /// Takes the frames due from those decoded ahead (all but the last of them dropped).
    fn sync(&mut self, clock: &dyn Clock) {
        let mut c = lock_clip(&self.shared);
        let pos = clip_position(&c, self.anchor_ms, clock);
        while let Some((generation, f)) = c.ahead.front().cloned() {
            if generation != c.generation {
                c.ahead.pop_front();
                continue;
            }
            if f.time_ms > pos {
                break;
            }
            c.ahead.pop_front();
            c.shown = Some(f);
        }
    }

    fn view(&mut self, clock: &dyn Clock) -> ClipView {
        self.sync(clock);
        let c = lock_clip(&self.shared);
        ClipView {
            key: self.key,
            photo: self.id,
            position_ms: clip_position(&c, self.anchor_ms, clock),
            frame: c.shown.as_ref().map(|f| f.index),
            playing: c.play.is_some(),
            sound: self.sound_open.is_some(),
        }
    }

    fn state(&self, clock: &dyn Clock) -> ClipState {
        let c = lock_clip(&self.shared);
        ClipState {
            photo: self.id,
            ready: c.ready,
            playing: c.play.is_some(),
            position_ms: clip_position(&c, self.anchor_ms, clock),
            end_ms: c.end_ms,
            sound: self.sound_open.is_some(),
            muted: c.muted.clone(),
            failed: c.failed.clone(),
        }
    }
}

impl Drop for Clip {
    fn drop(&mut self) {
        if let Some(stream) = self.sound_open.take()
            && self.sound.stream() == stream
        {
            self.sound.stop();
        }
    }
}

/// A clip's worker: opens the clip, decodes its frames ahead of the one due and queues its sound
/// while it plays.
struct ClipWorker {
    path: PathBuf,
    shared: Arc<Mutex<ClipShared>>,
    sound: Arc<dyn Sound>,
    clock: Arc<dyn Clock>,
    rx: Receiver<ClipRequest>,
}

/// One play's progress.
struct ClipPlay {
    generation: u64,
    from_ms: u64,
    /// Sound frames queued since play started.
    queued: u64,
    sound_done: bool,
    frames_done: bool,
}

impl ClipWorker {
    fn run(self, sound_plays: bool, start_ms: u64) {
        let limits = Limits::default();
        let threads = std::thread::available_parallelism().map_or(0, |n| (n.get() / 2).min(4));
        let options = FrameOptions { max_side: Some(crate::video::PLAY_SIDE), threads };
        let mut frames = match FrameSource::open(&self.path, &limits, options) {
            Ok(f) => f,
            Err(e) => {
                lock_clip(&self.shared).failed = Some(e.reason());
                return;
            }
        };
        let mut sound = None;
        if sound_plays {
            match SoundSource::open(&self.path, &limits) {
                Ok(s) => sound = Some(s),
                Err(m) => lock_clip(&self.shared).muted = Some(m.reason),
            }
        }
        let format = sound.as_ref().and_then(|s| Format { rate: s.rate(), channels: s.channels() }.check().ok());
        if format.is_none() {
            sound = None;
        }
        let count = frames.frame_count();
        let last = count.checked_sub(1).and_then(|i| frames.time_of(i)).unwrap_or(0);
        let step = if count > 1 { last / (count as u64 - 1).max(1) } else { 0 };
        let first = frames.frame_at(start_ms);
        {
            let mut c = lock_clip(&self.shared);
            c.format = format;
            c.frames_ms = last.saturating_add(step);
            if c.end_ms == 0 || c.end_ms > c.frames_ms {
                c.end_ms = c.frames_ms;
            }
            match first {
                Ok(f) => {
                    c.shown = Some(Arc::new(f));
                    c.ready = true;
                }
                Err(e) => c.failed = Some(e.reason()),
            }
        }
        let mut play: Option<ClipPlay> = None;
        loop {
            let wait = if play.is_some() { CLIP_PUMP } else { CLIP_IDLE };
            match self.rx.recv_timeout(wait) {
                Ok(r) => self.handle(r, &mut frames, sound.as_mut(), &mut play),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            while let Ok(r) = self.rx.try_recv() {
                self.handle(r, &mut frames, sound.as_mut(), &mut play);
            }
            if let Some(p) = play.as_mut() {
                let current = {
                    let c = lock_clip(&self.shared);
                    c.play.is_some() && c.generation == p.generation
                };
                if !current {
                    play = None;
                    continue;
                }
                self.pump(p, &mut frames, sound.as_mut(), format);
            }
        }
    }

    fn handle(&self, r: ClipRequest, frames: &mut FrameSource, sound: Option<&mut SoundSource>, play: &mut Option<ClipPlay>) {
        let ClipRequest::Play { generation, from_ms } = r;
        if lock_clip(&self.shared).generation != generation {
            return;
        }
        if let Some(s) = sound {
            s.seek(from_ms);
        }
        match frames.frame_at(from_ms) {
            Ok(f) => {
                let mut c = lock_clip(&self.shared);
                if c.generation == generation {
                    c.shown = Some(Arc::new(f));
                }
            }
            Err(e) => {
                lock_clip(&self.shared).failed = Some(e.reason());
                *play = None;
                return;
            }
        }
        *play = Some(ClipPlay { generation, from_ms, queued: 0, sound_done: false, frames_done: false });
    }

    /// Queues the sound ahead of the position and decodes the frames ahead of the one due.
    fn pump(&self, p: &mut ClipPlay, frames: &mut FrameSource, mut sound: Option<&mut SoundSource>, format: Option<Format>) {
        let (end, volume, stream, pos) = {
            let c = lock_clip(&self.shared);
            (c.end_ms, c.volume, c.stream, clip_position(&c, p.from_ms, self.clock.as_ref()))
        };
        if let (Some(format), Some(stream)) = (format, stream) {
            let rate = u64::from(format.rate.max(1));
            let channels = usize::from(format.channels.max(1));
            let ms_queued = |p: &ClipPlay| p.queued.saturating_mul(1000) / rate;
            let room = end.saturating_sub(p.from_ms);
            let heard = pos.saturating_sub(p.from_ms);
            let mut rounds = 0;
            while ms_queued(p) < heard.saturating_add(crate::video::SOUND_AHEAD_MS) && ms_queued(p) < room && rounds < 64 {
                rounds += 1;
                let left = usize::try_from(room.saturating_sub(ms_queued(p)).saturating_mul(rate) / 1000).unwrap_or(usize::MAX);
                let mut samples = match (p.sound_done, sound.as_deref_mut()) {
                    (false, Some(s)) => match s.next_chunk() {
                        Ok(Some(chunk)) => {
                            let skip = p.from_ms.saturating_sub(chunk.time_ms).saturating_mul(rate) / 1000;
                            let skip = usize::try_from(skip).unwrap_or(usize::MAX).saturating_mul(channels);
                            chunk.samples.get(skip.min(chunk.samples.len())..).unwrap_or_default().to_vec()
                        }
                        Ok(None) | Err(_) => {
                            p.sound_done = true;
                            continue;
                        }
                    },
                    _ => vec![0.0; usize::try_from(rate / 10).unwrap_or(0) * channels],
                };
                samples.truncate(left.saturating_mul(channels));
                samples.truncate(samples.len() / channels * channels);
                if samples.is_empty() {
                    break;
                }
                samples.iter_mut().for_each(|v| *v *= volume);
                if self.sound.stream() != stream || self.sound.queue(&samples).is_err() {
                    // The output went to another player, or away: the clip plays on silent.
                    lock_clip(&self.shared).stream = None;
                    break;
                }
                p.queued = p.queued.saturating_add((samples.len() / channels) as u64);
            }
        }
        let mut decoded = 0;
        while !p.frames_done && decoded < CLIP_AHEAD {
            let (pos, room) = {
                let c = lock_clip(&self.shared);
                if c.generation != p.generation {
                    return;
                }
                (clip_position(&c, p.from_ms, self.clock.as_ref()), c.ahead.len() < CLIP_AHEAD)
            };
            if !room {
                break;
            }
            decoded += 1;
            let f = match frames.next_frame() {
                Ok(Some(f)) => f,
                Ok(None) => {
                    p.frames_done = true;
                    break;
                }
                Err(e) => {
                    log::warn!("{}: a frame could not be decoded: {}", self.path.display(), e.reason());
                    p.frames_done = true;
                    break;
                }
            };
            if f.time_ms >= end {
                p.frames_done = true;
                break;
            }
            let next = frames.time_of(f.index + 1).unwrap_or(u64::MAX);
            let mut c = lock_clip(&self.shared);
            if c.generation != p.generation {
                return;
            }
            if next <= pos {
                continue;
            }
            c.ahead.push_back((p.generation, Arc::new(f)));
        }
    }
}
