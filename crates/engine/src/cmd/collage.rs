//! The Collage room (`creating.md` §3, `roadmap.md` M14 → *Collage*; the state is
//! `crate::collage`): `create.collage {ids | section}`, `collage.inspect`, `collage.set`,
//! `collage.clips.add` / `.clips.remove`, `collage.place` / `.unplace`, `collage.move`,
//! `collage.swap`, `collage.order`, `collage.background`, `collage.frameCenter`,
//! `collage.scramble`, `collage.shuffle`, `collage.reset`, `collage.formats.remove`, `collage.undo` /
//! `.redo` (the room's own history) and `collage.close {discard?}`.
//!
//! One collage is open at a time; opening again adds the new pictures to its Clips. Every change
//! is one step of the room's own history (`collage.undo`, `collage.redo`: Ctrl+Z and Ctrl+Y while
//! the room is in front) and never a step of the library's undo stack, so these commands are
//! `History::None`; like the Import room's they change only the room (and the remembered settings),
//! never the library. A change is checked as a whole document (`Collage::check`) before it is
//! taken, so a refused change leaves the room as it was. `item` (and `a`, `b`) is a picture's
//! number among `collage.inspect`'s placements (bottom first), as the canvas's `item:<n>` is.
//!
//! The page formats are the Crop panel's ratios (`crop.ratios.list`, the custom ones shared) and
//! Current Display at the size the window gives (`display`); deleting a custom one here deletes it
//! from the Crop panel too. The style and settings chosen are remembered (`settings.json` →
//! `collage`) and the next collage starts from them. Nothing here touches the disk on the owner
//! thread: the settings are written by the I/O worker.

use std::collections::HashSet;

use dalica_core::PhotoId;
use dalica_create::collage::page::{CURRENT_DISPLAY, DEFAULT_FORMAT, Format, Orientation, Page, PhotoSizes, formats};
use dalica_create::collage::{Border, Collage as Doc, Move, Rng, Style, doc::MAX_LAYER};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Context, Disabled, Effect, History, always, params};
use crate::session::Session;
use crate::settings::{self, CollageSettings, CollageStyle, PageOrientation, PictureBorder};

/// Why a room command is refused with no collage open.
pub const NOT_OPEN: &str = "No collage is open.";
/// Why `create.collage` is refused without photos.
pub const NO_PHOTOS: &str = "Select the photos for the collage first.";
/// The question Close asks with unsaved changes.
pub const CLOSE_QUESTION: &str = "Save the collage as a draft before closing? Its changes are lost otherwise.";
/// The most ids one command takes.
pub const MAX_IDS: usize = 100_000;
/// The widest angle `collage.move` takes, in degrees either way (folded into ±180).
pub const MAX_TURN: f64 = 360.0;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "create.collage",
            label: "Picture Collage…",
            menu: "Create/Picture Collage…",
            keys: &[],
            params: r#"{"ids"?:[PhotoId] (the tray, in its order) | "section"?:"folder:<id>"|"album:<id>"|"album:starred"|"album:recent"|"album:search"|"person:<id>" (exactly one), "display"?:[w, h] (the display's size in pixels, for Current Display), "seed"?:string (1–16 hex digits: the arrangement's seed; else a new one)}: opens the Collage room with these photos (videos and hidden photos left out), every one in the Clips and on the page, in the last style and settings; with a collage open, adds the new ones to its Clips and page (one step of the room's history). At most 1000 clips. Replies {added, clips, left (already in or over the limit), opened}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: has_photos,
            run: create,
        },
        CommandSpec {
            id: "collage.inspect",
            label: "Inspect Collage",
            menu: "",
            keys: &[],
            params: r#"{}: the open collage, {open:false} with none: {open, document (style, settings, page, seed, shuffle, clips, background, frameCentre, moves, swaps, as in a .dalica-collage recipe), page: {width, height, ratio} (the size Create Collage writes), placements: [{item, clip, id, centre:[x,y], size:[w,h], angle, z, shown:[x0,y0,x1,y1]}] (bottom first, normalised to the page), clips: [{id, thumb (the PhotoId whose thumbnail draws it), name, onPage, aspect, missing}], formats: [{name, width, height, custom, current}], offers: {borders, spacing, captions, background, free}, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: inspect,
        },
        CommandSpec {
            id: "collage.set",
            label: "Collage Settings",
            menu: "",
            keys: &[],
            params: r##"{"style"?:"pile"|"mosaic"|"frameMosaic"|"grid"|"contactSheet"|"multipleExposure", "borders"?:"none"|"white"|"instant", "gridSpacing"?:number (percent of the page's short side, 0–5), "shadows"?:bool, "captions"?:bool, "pageFormat"?:string (a name from collage.inspect's formats; "Current Display" needs the display's size), "orientation"?:"landscape"|"portrait", "backgroundColor"?:"#rrggbb", "display"?:[w, h] (the display's size in pixels)}: the Settings tab; one step of the room's history; the style and settings are remembered for the next collage. Replies {changed, unsaved, undo, redo}"##,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: set,
        },
        CommandSpec {
            id: "collage.clips.add",
            label: "Add to Collage",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId]}: adds the photos to the Clips and the page (Get More's Back to Collage), those already in left out; videos and hidden photos left out. Replies {added, clips, left, changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: clips_add,
        },
        CommandSpec {
            id: "collage.clips.remove",
            label: "Remove from Clips",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId]}: takes clips out of the Clips (−), and with them their place, moves and swaps; a background or frame centre among them is unset. Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: clips_remove,
        },
        CommandSpec {
            id: "collage.place",
            label: "Put on Page",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId]}: puts clips on the page (+). Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: place,
        },
        CommandSpec {
            id: "collage.unplace",
            label: "Take off Page",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId]}: takes clips off the page (Remove, Delete); they stay in the Clips. Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: unplace,
        },
        CommandSpec {
            id: "collage.move",
            label: "Move Picture",
            menu: "",
            keys: &[],
            params: r#"{"item":int, "centre"?:[x, y] (normalised to the page, -0.5–1.5), "scale"?:number (over the layout's size, 0.05–20), "angle"?:number (degrees clockwise, -360–360, folded into ±180)} (at least one of the three): moves, scales and turns a picture of a Picture Pile. Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: move_item,
        },
        CommandSpec {
            id: "collage.swap",
            label: "Swap Pictures",
            menu: "",
            keys: &[],
            params: r#"{"a":int, "b":int}: exchanges two pictures' places (every style but Picture Pile). Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: swap,
        },
        CommandSpec {
            id: "collage.order",
            label: "Bring to Top / Send to Bottom",
            menu: "",
            keys: &[],
            params: r#"{"item":int, "to":"top"|"bottom"}: Bring to Top or Send to Bottom. Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: order,
        },
        CommandSpec {
            id: "collage.background",
            label: "Set as Background",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId|null}: a clip covers the page as its background (Use picture), or null for the solid colour. Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: background,
        },
        CommandSpec {
            id: "collage.frameCenter",
            label: "Set as Frame Center",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: the picture in the centre of a Frame Mosaic (a clip on the page). Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: frame_center,
        },
        CommandSpec {
            id: "collage.scramble",
            label: "Scramble Collage",
            menu: "",
            keys: &[],
            params: r#"{}: a new arrangement (a new seed; the pictures moved and swapped are laid out again). Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: scramble,
        },
        CommandSpec {
            id: "collage.shuffle",
            label: "Shuffle Pictures",
            menu: "",
            keys: &[],
            params: r#"{}: the same places, the pictures dealt to them again. Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: shuffle,
        },
        CommandSpec {
            id: "collage.reset",
            label: "Reset",
            menu: "",
            keys: &[],
            params: r#"{}: the Settings tab back to its defaults (style, borders, spacing, background, page format and orientation, shadows, captions), remembered for the next collage; the clips and the arrangement stay. Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: reset,
        },
        CommandSpec {
            id: "collage.formats.remove",
            label: "Delete Page Format",
            menu: "",
            keys: &[],
            params: r#"{"name":string}: deletes a custom ratio from the page formats and from the Crop panel's ratios (they are shared); an open collage on that format goes to the default one (a step of the room's history). Replies {formats}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: formats_remove,
        },
        CommandSpec {
            id: "collage.undo",
            label: "Undo",
            menu: "",
            keys: &[(Context::Collage, "Ctrl+Z")],
            params: r#"{}: undoes the room's last change (never the library's). Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: can_undo,
            run: undo,
        },
        CommandSpec {
            id: "collage.redo",
            label: "Redo",
            menu: "",
            keys: &[(Context::Collage, "win,linux:Ctrl+Y"), (Context::Collage, "mac:Ctrl+Shift+Z")],
            params: r#"{}: redoes the room's last undone change. Replies {changed, unsaved, undo, redo}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: can_redo,
            run: redo,
        },
        CommandSpec {
            id: "collage.close",
            label: "Close",
            menu: "",
            keys: &[],
            params: r#"{"discard"?:bool, "confirm"?:bool (the same as discard)}: closes the Collage room. With unsaved changes it asks first (Save Draft, Discard, Cancel): refused without "confirm": true (or "discard": true), which discards them. Replies {closed, discarded}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: room_open,
            run: close,
        },
    ]
}

// --- enabled -------------------------------------------------------------------------------------

fn room_open(s: &Session, _: &Value) -> Result<(), Disabled> {
    if s.collage().is_open() { Ok(()) } else { Err(Disabled(NOT_OPEN.into())) }
}

fn has_photos(_: &Session, p: &Value) -> Result<(), Disabled> {
    let given = |k: &str| p.get(k).is_some_and(|v| !v.is_null());
    if given("ids") || given("section") { Ok(()) } else { Err(Disabled(NO_PHOTOS.into())) }
}

fn can_undo(s: &Session, p: &Value) -> Result<(), Disabled> {
    room_open(s, p)?;
    if s.collage().history_depth().0 > 0 { Ok(()) } else { Err(Disabled("Nothing to undo in the collage.".into())) }
}

fn can_redo(s: &Session, p: &Value) -> Result<(), Disabled> {
    room_open(s, p)?;
    if s.collage().history_depth().1 > 0 { Ok(()) } else { Err(Disabled("Nothing to redo in the collage.".into())) }
}

// --- the settings and the document ---------------------------------------------------------------

fn style_of(s: CollageStyle) -> Style {
    Style::from_id(s.id()).unwrap_or_default()
}

fn border_of(b: PictureBorder) -> Border {
    match b {
        PictureBorder::None => Border::None,
        PictureBorder::White => Border::White,
        PictureBorder::Instant => Border::Instant,
    }
}

fn picture_border(b: Border) -> PictureBorder {
    match b {
        Border::None => PictureBorder::None,
        Border::White => PictureBorder::White,
        Border::Instant => PictureBorder::Instant,
    }
}

fn orientation_of(o: PageOrientation) -> Orientation {
    match o {
        PageOrientation::Landscape => Orientation::Landscape,
        PageOrientation::Portrait => Orientation::Portrait,
    }
}

/// The Crop panel's custom ratios (name, width, height).
fn custom_ratios(s: &Session) -> Vec<(String, f64, f64)> {
    s.settings().edit().crop_ratios.into_iter().map(|r| (r.name, r.width, r.height)).collect()
}

/// The page formats offered: the Crop panel's ratios and, with the display's size known, Current
/// Display.
fn page_formats(s: &Session, display: Option<(u32, u32)>) -> Vec<Format> {
    let sizes = match s.settings().edit().photo_sizes() {
        settings::PhotoSizes::Us => PhotoSizes::Us,
        settings::PhotoSizes::Metric => PhotoSizes::Metric,
    };
    formats(sizes, &custom_ratios(s), display)
}

/// The format named `name`, if offered.
fn format_named(s: &Session, name: &str, display: Option<(u32, u32)>) -> Option<Format> {
    page_formats(s, display).into_iter().find(|f| f.name() == name)
}

/// The default page format (16:9, always offered).
fn default_format() -> Format {
    Page::default().format
}

/// `c` given to `doc`: the style, the settings, the page and the background colour.
fn apply_settings(s: &Session, doc: &mut Doc, c: &CollageSettings, display: Option<(u32, u32)>) {
    doc.style = style_of(c.style);
    doc.settings.border = border_of(c.borders);
    doc.settings.spacing = (c.grid_spacing / 100.0).clamp(0.0, dalica_create::collage::doc::MAX_SPACING);
    doc.settings.shadows = c.shadows;
    doc.settings.captions = c.captions;
    doc.page =
        Page { format: format_named(s, c.page_format.trim(), display).unwrap_or_else(default_format), orientation: orientation_of(c.orientation) };
    doc.background.colour = c.background_color;
}

/// The settings `doc` stands for, to remember for the next collage.
fn settings_of(doc: &Doc) -> CollageSettings {
    CollageSettings {
        style: CollageStyle::from_id(doc.style.id()).unwrap_or_default(),
        borders: picture_border(doc.settings.border),
        grid_spacing: (doc.settings.spacing * 100.0).clamp(0.0, 5.0),
        shadows: doc.settings.shadows,
        captions: doc.settings.captions,
        page_format: doc.page.format.name().to_string(),
        orientation: match doc.page.orientation {
            Orientation::Landscape => PageOrientation::Landscape,
            Orientation::Portrait => PageOrientation::Portrait,
        },
        background_color: doc.background.colour,
    }
}

fn remember(s: &mut Session, doc: &Doc) {
    let c = settings_of(doc);
    if s.settings().collage() != c {
        s.change_settings(|st| st.set_collage(&c));
    }
}

/// What a change answers.
fn summary(s: &Session, changed: bool) -> Value {
    let (undo, redo) = s.collage().history_depth();
    json!({ "changed": changed, "unsaved": s.collage().is_changed(), "undo": undo, "redo": redo })
}

/// The open document, for a change.
fn current(s: &Session) -> Result<Doc, CmdError> {
    s.collage().document().cloned().ok_or_else(|| CmdError::Disabled(NOT_OPEN.into()))
}

/// Takes `doc` as one step of the room's history once it passes the document's checks.
fn commit(s: &mut Session, doc: Doc) -> Result<bool, CmdError> {
    doc.check().map_err(|e| CmdError::BadParams(e.to_string()))?;
    Ok(s.collage.change(doc))
}

/// Runs `f` on a copy of the open document and takes the result as one step.
fn edit(s: &mut Session, f: impl FnOnce(&mut Doc) -> Result<(), CmdError>) -> Result<Value, CmdError> {
    let mut doc = current(s)?;
    f(&mut doc)?;
    let changed = commit(s, doc)?;
    Ok(summary(s, changed))
}

fn check_ids(ids: &[PhotoId]) -> Result<(), CmdError> {
    if ids.len() > MAX_IDS { Err(CmdError::Limit(format!("at most {MAX_IDS} photos at once"))) } else { Ok(()) }
}

/// The positions in the Clips of `ids`, each a clip.
fn clip_positions(doc: &Doc, ids: &[PhotoId]) -> Result<Vec<usize>, CmdError> {
    check_ids(ids)?;
    ids.iter().map(|id| doc.clip_index(*id).ok_or_else(|| CmdError::NotFound(format!("picture {id} is not in the collage's clips")))).collect()
}

/// The photo of the picture numbered `item` among the placements.
fn item_id(s: &mut Session, item: usize) -> Result<PhotoId, CmdError> {
    let placements = s.collage_placements();
    placements.get(item).map(|p| p.id).ok_or_else(|| CmdError::NotFound(format!("no picture {item} on the page ({} there)", placements.len())))
}

fn display_param(display: Option<[u32; 2]>) -> Result<Option<(u32, u32)>, CmdError> {
    match display {
        None => Ok(None),
        Some([w, h]) if Format::display(w, h).is_some() => Ok(Some((w, h))),
        Some(_) => Err(CmdError::BadParams(format!("display: [width, height] in pixels, each 1 to {}", dalica_create::collage::page::MAX_PIXELS))),
    }
}

// --- create.collage, clips -----------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateParams {
    ids: Option<Vec<PhotoId>>,
    section: Option<String>,
    display: Option<[u32; 2]>,
    seed: Option<String>,
}

fn parse_seed(text: &str) -> Result<u64, CmdError> {
    let bad = || CmdError::BadParams("seed: 1 to 16 hex digits".into());
    if text.is_empty() || text.len() > 16 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    u64::from_str_radix(text, 16).map_err(|_| bad())
}

/// A new collage's seed: from the clock and the pictures, so two collages rarely start alike.
fn new_seed(s: &Session, ids: &[PhotoId]) -> u64 {
    let mut h = s.clock().now().as_millis() as u64;
    for id in ids {
        h = h.rotate_left(7) ^ id.get();
    }
    Rng::stream(h, 0x00c0_11a6).next_u64()
}

/// The photos of `ids` or `section` a collage takes (videos and hidden photos left out), and how
/// many videos it left out.
fn photos(s: &mut Session, section: Option<&str>, ids: Option<&[PhotoId]>) -> Result<(Vec<PhotoId>, usize), CmdError> {
    if let Some(ids) = ids {
        check_ids(ids)?;
    }
    let list = crate::cmd::slideshow::playlist(s, section, ids, None, false)?;
    Ok((list.photos, list.videos))
}

/// Adds `ids` to the open document as one step; answers how many were added.
fn add_to_room(s: &mut Session, ids: &[PhotoId]) -> Result<(usize, bool), CmdError> {
    let mut doc = current(s)?;
    let added = doc.add_clips(ids.iter().copied());
    let changed = commit(s, doc)?;
    Ok((added, changed))
}

fn create(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let CreateParams { ids, section, display, seed } = params(p)?;
    let display = display_param(display)?;
    let seed = seed.as_deref().map(parse_seed).transpose()?;
    let (found, videos) = photos(s, section.as_deref(), ids.as_deref())?;
    if s.collage().is_open() {
        if let Some(d) = display {
            s.collage.set_display(d);
        }
        let (added, _) = add_to_room(s, &found)?;
        let clips = s.collage().document().map_or(0, |d| d.clips.len());
        let mut reply = json!({ "added": added, "clips": clips, "left": found.len().saturating_sub(added), "opened": false });
        crate::video::note_collage_videos(&mut reply, videos);
        return Ok(reply);
    }
    if found.is_empty() {
        let why = if videos > 0 { crate::video::COLLAGE_REFUSED } else { "None of these photos can go in a collage." };
        return Err(CmdError::Disabled(why.into()));
    }
    let seed = seed.unwrap_or_else(|| new_seed(s, &found));
    let mut doc = Doc::new(found.iter().copied(), seed);
    let c = s.settings().collage();
    apply_settings(s, &mut doc, &c, display);
    doc.check().map_err(|e| CmdError::BadParams(e.to_string()))?;
    let added = doc.clips.len();
    s.collage.open(doc, display);
    let mut reply = json!({ "added": added, "clips": added, "left": found.len().saturating_sub(added), "opened": true });
    crate::video::note_collage_videos(&mut reply, videos);
    Ok(reply)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ids {
    ids: Vec<PhotoId>,
}

fn clips_add(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Ids { ids } = params(p)?;
    let (found, videos) = photos(s, None, Some(&ids))?;
    let (added, changed) = add_to_room(s, &found)?;
    let mut reply = summary(s, changed);
    let clips = s.collage().document().map_or(0, |d| d.clips.len());
    if let Some(o) = reply.as_object_mut() {
        o.insert("added".into(), json!(added));
        o.insert("clips".into(), json!(clips));
        o.insert("left".into(), json!(found.len().saturating_sub(added)));
    }
    crate::video::note_collage_videos(&mut reply, videos);
    Ok(reply)
}

fn clips_remove(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Ids { ids } = params(p)?;
    edit(s, |doc| {
        clip_positions(doc, &ids)?;
        let gone: HashSet<PhotoId> = ids.iter().copied().collect();
        doc.clips.retain(|c| !gone.contains(&c.id));
        doc.moves.retain(|m| !gone.contains(&m.id));
        doc.swaps.retain(|[a, b]| !gone.contains(a) && !gone.contains(b));
        if doc.background.picture.is_some_and(|id| gone.contains(&id)) {
            doc.background.picture = None;
        }
        if doc.frame_centre.is_some_and(|id| gone.contains(&id)) {
            doc.frame_centre = None;
        }
        Ok(())
    })
}

fn set_on_page(s: &mut Session, p: &Value, on: bool) -> Result<Value, CmdError> {
    let Ids { ids } = params(p)?;
    edit(s, |doc| {
        for i in clip_positions(doc, &ids)? {
            if let Some(c) = doc.clips.get_mut(i) {
                c.on_page = on;
            }
        }
        Ok(())
    })
}

fn place(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    set_on_page(s, p, true)
}

fn unplace(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    set_on_page(s, p, false)
}

// --- the canvas ----------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveParams {
    item: usize,
    centre: Option<[f64; 2]>,
    scale: Option<f64>,
    angle: Option<f64>,
}

/// `angle` folded into −180 … 180 degrees.
fn fold_angle(angle: f64) -> f64 {
    let a = (angle + 180.0).rem_euclid(360.0) - 180.0;
    if a.is_finite() { a } else { 0.0 }
}

/// The user's move of `id` in `doc`, made when there is none.
fn move_mut(doc: &mut Doc, id: PhotoId) -> Option<&mut Move> {
    if doc.move_of(id).is_none() {
        doc.moves.push(Move { id, centre: None, scale: None, angle: None, layer: 0 });
    }
    doc.moves.iter_mut().find(|m| m.id == id)
}

fn move_item(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let MoveParams { item, centre, scale, angle } = params(p)?;
    if centre.is_none() && scale.is_none() && angle.is_none() {
        return Err(CmdError::BadParams("give at least one of centre, scale and angle".into()));
    }
    if let Some(a) = angle
        && !(a.is_finite() && (-MAX_TURN..=MAX_TURN).contains(&a))
    {
        return Err(CmdError::BadParams(format!("angle: degrees, -{MAX_TURN} to {MAX_TURN}")));
    }
    if current(s)?.style != Style::Pile {
        return Err(CmdError::Disabled("Only a Picture Pile's pictures move; drag one onto another to swap them.".into()));
    }
    let id = item_id(s, item)?;
    edit(s, |doc| {
        if let Some(m) = move_mut(doc, id) {
            m.centre = centre.or(m.centre);
            m.scale = scale.or(m.scale);
            m.angle = angle.map(fold_angle).or(m.angle);
        }
        Ok(())
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SwapParams {
    a: usize,
    b: usize,
}

/// `doc.swaps` rewritten as at most one swap fewer than the pictures on the page, giving the same
/// assignment (and so the same layout: the slots are made for the pictures before every swap), so
/// any number of drags fits the document's limit.
fn compact_swaps(doc: &mut Doc) {
    let target = doc.assignment();
    let mut plain = doc.clone();
    plain.swaps.clear();
    let mut order = plain.assignment();
    let id = |doc: &Doc, i: usize| doc.clips.get(i).map(|c| c.id);
    let mut swaps = Vec::new();
    for i in 0..order.len() {
        let (Some(&want), Some(&have)) = (target.get(i), order.get(i)) else { break };
        if want == have {
            continue;
        }
        let Some(j) = order.iter().position(|&k| k == want) else { break };
        if let (Some(x), Some(y)) = (id(doc, have), id(doc, want)) {
            swaps.push([x, y]);
        }
        order.swap(i, j);
    }
    doc.swaps = swaps;
}

fn swap(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SwapParams { a, b } = params(p)?;
    if current(s)?.style.is_free() {
        return Err(CmdError::Disabled("A Picture Pile's pictures move freely; swapping is for the other styles.".into()));
    }
    let (x, y) = (item_id(s, a)?, item_id(s, b)?);
    if x == y {
        return Err(CmdError::BadParams("a and b: two different pictures".into()));
    }
    edit(s, |doc| {
        doc.swaps.push([x, y]);
        if doc.swaps.len() > dalica_create::collage::MAX_CLIPS {
            compact_swaps(doc);
        }
        Ok(())
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum To {
    Top,
    Bottom,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderParams {
    item: usize,
    to: To,
}

fn order(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let OrderParams { item, to } = params(p)?;
    let id = item_id(s, item)?;
    edit(s, |doc| {
        let others = doc.moves.iter().filter(|m| m.id != id).map(|m| m.layer);
        let layer = match to {
            To::Top => others.max().unwrap_or(0).max(0).saturating_add(1).min(MAX_LAYER),
            To::Bottom => others.min().unwrap_or(0).min(0).saturating_sub(1).max(-MAX_LAYER),
        };
        if let Some(m) = move_mut(doc, id) {
            m.layer = layer;
        }
        Ok(())
    })
}

/// A key that must be present, its value possibly `null`.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BackgroundParams {
    #[serde(default, deserialize_with = "present")]
    id: Option<Option<PhotoId>>,
}

fn background(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let BackgroundParams { id } = params(p)?;
    let id = id.ok_or_else(|| CmdError::BadParams("id: a clip's PhotoId, or null for the solid colour".into()))?;
    edit(s, |doc| {
        if let Some(id) = id {
            clip_positions(doc, &[id])?;
        }
        doc.background.picture = id;
        Ok(())
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdParams {
    id: PhotoId,
}

fn frame_center(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let IdParams { id } = params(p)?;
    if current(s)?.style != Style::FrameMosaic {
        return Err(CmdError::Disabled("Set as Frame Center is for a Frame Mosaic.".into()));
    }
    edit(s, |doc| {
        let i = clip_positions(doc, &[id])?;
        if !i.iter().all(|&i| doc.clips.get(i).is_some_and(|c| c.on_page)) {
            return Err(CmdError::BadParams(format!("picture {id} is not on the page")));
        }
        doc.frame_centre = Some(id);
        Ok(())
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn scramble(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Empty {} = params(p)?;
    edit(s, |doc| {
        doc.scramble();
        Ok(())
    })
}

fn shuffle(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Empty {} = params(p)?;
    edit(s, |doc| {
        doc.shuffle_pictures();
        Ok(())
    })
}

// --- settings ------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SetParams {
    style: Option<String>,
    borders: Option<String>,
    grid_spacing: Option<f64>,
    shadows: Option<bool>,
    captions: Option<bool>,
    page_format: Option<String>,
    orientation: Option<String>,
    background_color: Option<String>,
    display: Option<[u32; 2]>,
}

fn set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let q: SetParams = params(p)?;
    let display = display_param(q.display)?;
    let given = [q.style.is_some(), q.borders.is_some(), q.grid_spacing.is_some(), q.shadows.is_some(), q.captions.is_some()]
        .into_iter()
        .chain([q.page_format.is_some(), q.orientation.is_some(), q.background_color.is_some(), display.is_some()])
        .any(|g| g);
    if !given {
        return Err(CmdError::BadParams("give at least one setting".into()));
    }
    let mut doc = current(s)?;
    if let Some(style) = &q.style {
        doc.style = Style::from_id(style).ok_or_else(|| CmdError::BadParams(format!("style: one of {}", ids(Style::ALL.map(Style::id)))))?;
    }
    if let Some(b) = &q.borders {
        let b = PictureBorder::from_id(b)
            .ok_or_else(|| CmdError::BadParams(format!("borders: one of {}", ids(PictureBorder::ALL.map(PictureBorder::id)))))?;
        doc.settings.border = border_of(b);
    }
    if let Some(g) = q.grid_spacing {
        if !(g.is_finite() && settings::GRID_SPACING_RANGE.contains(&g)) {
            return Err(CmdError::BadParams("gridSpacing: percent of the page's short side, 0 to 5".into()));
        }
        doc.settings.spacing = g / 100.0;
    }
    if let Some(v) = q.shadows {
        doc.settings.shadows = v;
    }
    if let Some(v) = q.captions {
        doc.settings.captions = v;
    }
    if let Some(o) = &q.orientation {
        let o = PageOrientation::from_id(o)
            .ok_or_else(|| CmdError::BadParams(format!("orientation: one of {}", ids(PageOrientation::ALL.map(PageOrientation::id)))))?;
        doc.page.orientation = orientation_of(o);
    }
    if let Some(c) = &q.background_color {
        doc.background.colour = settings::parse_color_hex(c).ok_or_else(|| CmdError::BadParams("backgroundColor: \"#rrggbb\"".into()))?;
    }
    if let Some(d) = display {
        s.collage.set_display(d);
    }
    if let Some(name) = &q.page_format {
        let known = s.collage().display();
        doc.page.format = match format_named(s, name.trim(), known) {
            Some(f) => f,
            None if name.trim() == CURRENT_DISPLAY => {
                return Err(CmdError::BadParams("pageFormat: Current Display needs the display's size (display: [w, h])".into()));
            }
            None => {
                let shown: String = name.chars().take(64).collect();
                return Err(CmdError::NotFound(format!("pageFormat: no page format named {shown:?} (collage.inspect lists them)")));
            }
        };
    }
    let changed = commit(s, doc.clone())?;
    remember(s, &doc);
    Ok(summary(s, changed))
}

/// `a`, `b` or `c`, for a message.
fn ids<const N: usize>(all: [&str; N]) -> String {
    all.map(|i| format!("{i:?}")).join(", ")
}

fn reset(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Empty {} = params(p)?;
    let mut doc = current(s)?;
    let defaults = CollageSettings::default();
    let display = s.collage().display();
    apply_settings(s, &mut doc, &defaults, display);
    doc.background.picture = None;
    let changed = commit(s, doc.clone())?;
    remember(s, &doc);
    Ok(summary(s, changed))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameParams {
    name: String,
}

fn formats_json(s: &Session) -> Value {
    let custom: HashSet<String> = custom_ratios(s).into_iter().map(|(n, _, _)| n).collect();
    let current = s.collage().document().map(|d| d.page.format.name().to_string());
    let list: Vec<Value> = page_formats(s, s.collage().display())
        .iter()
        .map(|f| {
            let (width, height) = match *f {
                Format::Ratio { width, height, .. } => (width, height),
                Format::Display { width, height } => (f64::from(width), f64::from(height)),
            };
            let name = f.name();
            json!({ "name": name, "width": width, "height": height, "custom": matches!(f, Format::Ratio { .. }) && custom.contains(name), "current": current.as_deref() == Some(name) })
        })
        .collect();
    Value::Array(list)
}

fn formats_remove(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let NameParams { name } = params(p)?;
    let name = name.trim().to_string();
    let mut e = s.settings().edit();
    let before = e.crop_ratios.len();
    e.crop_ratios.retain(|r| r.name != name);
    if e.crop_ratios.len() == before {
        let shown: String = name.chars().take(64).collect();
        return Err(CmdError::NotFound(format!("no custom page format named {shown:?}")));
    }
    s.change_settings(|st| st.set_edit(&e));
    if let Some(mut doc) = s.collage().document().cloned()
        && doc.page.format.name() == name
        && matches!(doc.page.format, Format::Ratio { .. })
    {
        doc.page.format = format_named(s, DEFAULT_FORMAT, None).unwrap_or_else(default_format);
        commit(s, doc.clone())?;
        remember(s, &doc);
    }
    let mut c = s.settings().collage();
    if c.page_format == name {
        c.page_format = settings::COLLAGE_PAGE_FORMAT_DEFAULT.into();
        s.change_settings(|st| st.set_collage(&c));
    }
    Ok(json!({ "formats": formats_json(s) }))
}

// --- inspect, history, close ---------------------------------------------------------------------

fn inspect(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Empty {} = params(p)?;
    let Some(doc) = s.collage().document().cloned() else { return Ok(json!({ "open": false })) };
    let placements = s.collage_placements();
    let aspects = s.collage_aspects();
    let lib = s.library();
    let clips: Vec<Value> = doc
        .clips
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let photo = lib.photo(c.id);
            let aspect = aspects.get(i).copied().filter(|a| a.is_finite());
            json!({ "id": c.id, "thumb": c.id, "name": photo.map(|p| p.name.as_str()), "onPage": c.on_page, "aspect": aspect, "missing": photo.is_none() })
        })
        .collect();
    let placed: Vec<Value> = placements
        .iter()
        .enumerate()
        .map(|(item, p)| {
            let shown: [f64; 4] = p.shown.into();
            json!({ "item": item, "clip": p.clip, "id": p.id, "centre": p.centre, "size": p.size, "angle": p.angle, "z": p.z, "shown": shown })
        })
        .collect();
    let (width, height) = doc.page.output_size();
    let (undo, redo) = s.collage().history_depth();
    let style = doc.style;
    Ok(json!({
        "open": true,
        "document": serde_json::to_value(&doc).map_err(|e| CmdError::Internal(e.to_string()))?,
        "page": { "width": width, "height": height, "ratio": doc.page.ratio() },
        "placements": placed,
        "clips": clips,
        "formats": formats_json(s),
        "offers": { "borders": style.has_borders(), "spacing": style.has_spacing(), "captions": style.has_captions(), "background": style.has_background(), "free": style.is_free() },
        "unsaved": s.collage().is_changed(),
        "undo": undo,
        "redo": redo,
    }))
}

fn undo(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Empty {} = params(p)?;
    let changed = s.collage.undo();
    Ok(summary(s, changed))
}

fn redo(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Empty {} = params(p)?;
    let changed = s.collage.redo();
    Ok(summary(s, changed))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseParams {
    #[serde(default)]
    discard: bool,
    #[serde(default)]
    confirm: bool,
}

fn close(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let CloseParams { discard, confirm } = params(p)?;
    let changed = s.collage().is_changed();
    if changed && !(discard || confirm) {
        return Err(CmdError::Disabled(format!("Close Collage: {CLOSE_QUESTION} Send \"confirm\": true to discard the changes.")));
    }
    s.collage.close();
    Ok(json!({ "closed": true, "discarded": changed }))
}

#[cfg(test)]
mod tests {
    use dalica_core::PhotoId;
    use dalica_create::collage::{Collage as Doc, MAX_CLIPS, Style, layout};

    /// However many drags a mosaic takes, its swaps are rewritten within the document's limit
    /// and lay out exactly as before.
    #[test]
    fn compacted_swaps_lay_out_as_the_swaps_they_replace() {
        let mut doc = Doc::new((1..=7).map(PhotoId::new), 42);
        doc.style = Style::Mosaic;
        doc.shuffle = 9;
        let aspects = [1.5, 0.7, 1.0, 2.0, 0.5, 1.3, 0.8];
        for i in 0..MAX_CLIPS as u64 {
            let a = i % 7 + 1;
            let b = (a + i % 5) % 7 + 1;
            if a != b {
                doc.swaps.push([PhotoId::new(a), PhotoId::new(b)]);
            }
        }
        let before = layout(&doc, &aspects);
        super::compact_swaps(&mut doc);
        assert!(doc.swaps.len() < 7, "{} swaps", doc.swaps.len());
        assert!(doc.check().is_ok());
        assert_eq!(layout(&doc, &aspects), before);
    }
}
