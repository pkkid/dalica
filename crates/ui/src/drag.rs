//! Dragging photos (`ui-layout.md` §4, §5.5 and §6.2, `organizing.md` §4.3 and §9,
//! `control-protocol.md` §6.6).
//!
//! - **Start.** A press on a thumbnail held and moved past egui's drag threshold starts a drag of
//!   the selected photos when the thumbnail is selected, else of that one photo
//!   (`crate::panels::light_table::input`); a press on a mini thumbnail of the Photo Tray dragged
//!   the same way carries the tray-selected photos when it is tray-selected, else that one, and a
//!   drag from the tray's token carries the selection it stands for ([`frame`]).
//! - **Targets.** While a drag lasts, the parts of the window under the pointer offer what they
//!   are ([`offer`], at most one a frame, so the work is bounded by what is on screen): a library
//!   folder's tree row moves the photos there (with Ctrl held, copies them; the pill shows +), a
//!   user album's tree row adds them, the tray holds them, and the gap before or after a cell of a
//!   user album's section adds them there or, when they are all the album's, reorders them (the
//!   insertion marker). A place where a drop would change nothing is no target: the photos' own
//!   folder, an album holding them all, a tray holding them all, the gap beside a dragged photo.
//!   The target under the pointer gets the `drop_target` highlight; the drag pill (ui-kit's
//!   `drag_pill`, id `drag:pill`) follows the pointer with the first photo's thumbnail and what a
//!   drop would do ("Move 3 photos to Garden", "Copy 1 photo to Garden", "Add 3 photos to Lisbon",
//!   "Hold 3 photos in the tray", "Reorder 3 photos"; "3 photos" over nothing).
//! - **Drop.** Releasing the button over a target runs what it offered ([`drop_at`]): a move or a
//!   copy through `crate::commands::files::{move_photos, copy_photos}` (with their questions: the
//!   first-time move question, "Copy %d files to %s?", the collision question), `album.add` or
//!   `album.reorder` (`before` / `after` the photo beside the gap) or `tray.hold`. Released over
//!   nothing, the drag ends with nothing changed; Esc ends it at once ([`escape`]).
//! - **Inspection.** `ui.inspect` → `drag` reports the drag ([`inspect`]): where it started, the
//!   photos, the target's widget id, the operation and the pill's text.
//!
//! - **Faces (M13).** A drag can carry faces instead of photos ([`start_faces`]: from the Unnamed
//!   grid, a person section's Zoom to Faces cells, the People pane): a person's tree row offers to
//!   name them ([`DropAt::Person`], `faces.moveToPerson`); photos dragged onto the Places pane's
//!   map offer to geotag them there ([`DropAt::Map`], `places.drop`, which opens the drop bubble).
//!   A drop of photos on a person, or of faces anywhere else, changes nothing. M13.4's stub: the
//!   payload, the targets and the drops are final; the pill of a drag of faces is reported
//!   ([`face_pill`]) but not painted yet.
//!
//! **Boundary.** [`start`] is called by the part a drag starts on; [`offer`] by the parts drawn
//! under the pointer (the tree's rows, the light table's album cells), which paint their own
//! highlight or marker; [`frame`] once a frame after the panels (it finds the tray, resolves the
//! target, paints the pill and drops on release); [`escape`] before the frame's shortcuts.

use std::collections::HashSet;

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Session};
use dalica_ui_kit::registry;
use dalica_ui_kit::widgets::{self, DragPill, PILL_THUMB};
use egui::{Context, CursorIcon, Id, LayerId, Order, Pos2, Rect, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::app::App;
use crate::panels::Panels;
use crate::panels::thumbnails::{PhotoRef, fit_within};
use crate::state::{AlbumKey, Drag, DragOp, DropAt, FaceRef, MAX_IDS, UiState, View};

/// The drag pill's automation id (role `other`, painted, labelled with its text).
pub const PILL_ID: &str = "drag:pill";
/// The tray box's automation id, which is a drop target.
pub const TRAY_ID: &str = "tray";
/// The egui id of the tray box's interaction (`crate::panels::bottom::tray`), whose drag started
/// on a mini thumbnail starts a drag of photos.
const TRAY_BOX: &str = "tray.box";
/// The room left of the pill's thumbnail (ui-kit's `drag_pill`).
const PILL_PAD: f32 = 4.0;

/// What a part of the window under the pointer offers this frame: the target, its widget id and
/// what a drop there does.
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub at: DropAt,
    pub widget: String,
    pub op: DragOp,
}

fn offer_id() -> Id {
    Id::new("dalica-drag/offer")
}

/// Starts a drag of `ids` from the widget `from` (`thumb:12`, `tray:item:12`). A drag already
/// running is replaced.
pub fn start(app: &mut App, from: String, mut ids: Vec<PhotoId>) {
    ids.truncate(MAX_IDS);
    let pill = pill(None, ids.len(), None);
    app.state_mut().drag = Some(Drag { from, ids, faces: Vec::new(), target: None, op: None, pill, at: None });
}

/// Starts a drag of `faces` from the widget `from` (`face:12.0`). A drag already running is
/// replaced.
pub fn start_faces(app: &mut App, from: String, mut faces: Vec<FaceRef>) {
    faces.truncate(MAX_IDS);
    let pill = face_pill(None, faces.len(), None);
    app.state_mut().drag = Some(Drag { from, ids: Vec::new(), faces, target: None, op: None, pill, at: None });
}

/// Ends the drag in progress, if any, without dropping anything.
pub fn end(app: &mut App) {
    app.state_mut().drag = None;
}

/// Whether a drag is in progress.
pub fn dragging(app: &App) -> bool {
    app.state().drag.is_some()
}

/// The pointer while a drag is in progress; `None` without one.
pub fn pointer(ctx: &Context, state: &UiState) -> Option<Pos2> {
    state.drag.as_ref()?;
    ctx.input(|i| i.pointer.latest_pos()).filter(|p| p.is_finite())
}

/// Whether a drop would copy rather than move: Ctrl (Cmd on macOS) is held.
pub fn copying(ctx: &Context) -> bool {
    ctx.input(|i| i.modifiers.command || i.modifiers.ctrl)
}

/// What dropping `ids` at `at` does, or `None` when it would change nothing (`copy`: Ctrl is
/// held). A folder moves or copies the photos not already in it; a user album adds those it does
/// not hold; the tray holds those it does not; a gap reorders the album's own photos, adds others,
/// and is no target beside a photo being dragged.
pub fn op_at(session: &Session, state: &UiState, ids: &[PhotoId], at: &DropAt, copy: bool) -> Option<DragOp> {
    let lib = session.library();
    let members = |key: &AlbumKey| -> Option<HashSet<PhotoId>> {
        let album = key.id()?;
        lib.album(&album)?;
        Some(lib.album_members(&album).iter().copied().collect())
    };
    match at {
        DropAt::Folder(f) => {
            lib.folder(*f)?;
            let foreign = ids.iter().any(|id| lib.photo(*id).is_some_and(|p| p.folder != *f));
            foreign.then_some(if copy { DragOp::Copy } else { DragOp::Move })
        }
        DropAt::Album(key) => {
            let members = members(key)?;
            ids.iter().any(|id| !members.contains(id)).then_some(DragOp::Add)
        }
        DropAt::Tray => ids.iter().any(|id| !state.tray.holds(*id)).then_some(DragOp::Hold),
        DropAt::Gap { album, anchor, .. } => {
            let members = members(album)?;
            if ids.contains(anchor) || !members.contains(anchor) {
                return None;
            }
            Some(if ids.iter().all(|id| members.contains(id)) { DragOp::Reorder } else { DragOp::Add })
        }
        // Photos hold no face to name; the map geotags them.
        DropAt::Person(_) => None,
        DropAt::Map { .. } => (!ids.is_empty()).then_some(DragOp::Geotag),
    }
}

/// What dropping `faces` at `at` does: a person's row names them; nothing else takes faces.
pub fn face_op_at(faces: &[FaceRef], at: &DropAt) -> Option<DragOp> {
    match at {
        DropAt::Person(_) if !faces.is_empty() => Some(DragOp::Name),
        _ => None,
    }
}

/// The drag pill's text for `n` faces: "2 faces", and over a person "Name 2 faces Ana".
pub fn face_pill(op: Option<DragOp>, n: usize, target: Option<&str>) -> String {
    let faces = if n == 1 { "1 face".to_string() } else { format!("{n} faces") };
    match (op, target) {
        (Some(DragOp::Name), Some(name)) => format!("Name {faces} {name}"),
        (Some(DragOp::Name), None) => format!("Name {faces}"),
        _ => faces,
    }
}

/// [`op_at`] as last worked out, so a pointer resting on a target costs nothing more: what it was
/// worked out for (the photos, the target, Ctrl, the library's revision, the tray's held photos).
#[derive(Debug, Clone, PartialEq)]
struct Known {
    ids: Vec<PhotoId>,
    at: DropAt,
    copy: bool,
    revision: u64,
    held: usize,
    op: Option<DragOp>,
}

fn known_id() -> Id {
    Id::new("dalica-drag/known")
}

/// Offers target `at` (drawn as widget `widget`) under the pointer this frame. Returns what a drop
/// there would do, for the caller to paint its highlight; `None` (and no offer) when there is no
/// drag or a drop would change nothing. Worked out again only when the target, Ctrl, the library
/// or the tray changed.
pub fn offer(ctx: &Context, session: &Session, state: &UiState, at: DropAt, widget: &str) -> Option<DragOp> {
    let drag = state.drag.as_ref()?;
    if !drag.faces.is_empty() {
        let op = face_op_at(&drag.faces, &at)?;
        ctx.data_mut(|d| d.insert_temp(offer_id(), Offer { at, widget: widget.to_string(), op }));
        return Some(op);
    }
    let (copy, revision, held) = (copying(ctx), session.library().revision(), state.tray.held.len());
    let known: Option<Known> = ctx.data(|d| d.get_temp(known_id()));
    let op = match known {
        Some(k) if k.at == at && k.copy == copy && k.revision == revision && k.held == held && k.ids == drag.ids => k.op,
        _ => {
            let op = op_at(session, state, &drag.ids, &at, copy);
            ctx.data_mut(|d| d.insert_temp(known_id(), Known { ids: drag.ids.clone(), at, copy, revision, held, op }));
            op
        }
    }?;
    let offer = Offer { at, widget: widget.to_string(), op };
    ctx.data_mut(|d| d.insert_temp(offer_id(), offer));
    Some(op)
}

/// The name a drop target goes by in the pill: the folder's, or the album's when photos join it.
fn target_name(session: &Session, at: &DropAt, op: DragOp) -> Option<String> {
    let lib = session.library();
    match (at, op) {
        (DropAt::Folder(f), _) => {
            let f = lib.folder(*f)?;
            Some(f.path.file_name().map_or_else(|| f.path.display().to_string(), |n| n.to_string_lossy().into_owned()))
        }
        (DropAt::Album(a) | DropAt::Gap { album: a, .. }, DragOp::Add) => a.id().and_then(|id| lib.album(&id).map(|i| i.name.clone())),
        _ => None,
    }
}

/// ui-kit's pill operation for a drag's.
fn pill_op(op: Option<DragOp>) -> widgets::DragOp {
    match op {
        None => widgets::DragOp::None,
        Some(DragOp::Move) => widgets::DragOp::Move,
        Some(DragOp::Copy) => widgets::DragOp::Copy,
        Some(DragOp::Add) => widgets::DragOp::AddToAlbum,
        Some(DragOp::Reorder) => widgets::DragOp::Reorder,
        Some(DragOp::Hold) => widgets::DragOp::Hold,
        // ui-kit's pill has no words for these yet: the count alone.
        Some(DragOp::Name | DragOp::Geotag) => widgets::DragOp::None,
    }
}

/// The drag pill's text for `n` photos, what a drop would do and the target's name (ui-kit's
/// `drag_pill_text`): "Move 3 photos to Garden", "Add 1 photo to Lisbon", "Hold 3 photos in the
/// tray", "Reorder 3 photos"; with no target the count alone.
pub fn pill(op: Option<DragOp>, n: usize, target: Option<&str>) -> String {
    widgets::drag_pill_text(pill_op(op), n, target)
}

/// Esc during a drag ends it with nothing dropped; its key press is taken, so no shortcut sees it.
/// Call before the frame's shortcuts.
pub fn escape(app: &mut App, ctx: &Context) {
    if app.state().drag.is_none() {
        return;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        end(app);
    }
}

/// Once a frame, after the panels: starts a drag from the tray, finds the target under the pointer
/// (what a part offered, else the tray), paints the tray's highlight and the pill, and on release
/// drops the photos there.
pub fn frame(p: &mut Panels, app: &mut App, ctx: &Context) {
    let offered: Option<Offer> = ctx.data_mut(|d| {
        let o = d.get_temp(offer_id());
        d.remove::<Offer>(offer_id());
        o
    });
    if app.state().view != View::Library {
        end(app);
        return;
    }
    if app.state().drag.is_none() {
        start_from_tray(app, ctx);
    }
    let (Some(session), Some(drag)) = (app.session(), app.state().drag.as_ref()) else { return };
    let pos = ctx.input(|i| i.pointer.latest_pos()).filter(|p| p.is_finite());
    let tray = registry::find(ctx, TRAY_ID).map(|w| Rect::from_min_size(pos2(w.rect[0], w.rect[1]), vec2(w.rect[2], w.rect[3])));
    let offered = offered.or_else(|| {
        let over = tray.zip(pos).is_some_and(|(r, p)| r.contains(p));
        let op = over.then(|| op_at(session, app.state(), &drag.ids, &DropAt::Tray, false)).flatten()?;
        Some(Offer { at: DropAt::Tray, widget: TRAY_ID.into(), op })
    });
    let name = offered.as_ref().and_then(|o| target_name(session, &o.at, o.op));
    let faces = drag.faces.len();
    let text = if faces > 0 {
        face_pill(offered.as_ref().map(|o| o.op), faces, name.as_deref())
    } else {
        pill(offered.as_ref().map(|o| o.op), drag.ids.len(), name.as_deref())
    };
    let (count, first) = (drag.ids.len(), drag.ids.first().copied());
    if let Some(d) = app.state_mut().drag.as_mut() {
        d.target = offered.as_ref().map(|o| o.widget.clone());
        d.op = offered.as_ref().map(|o| o.op);
        d.at = offered.as_ref().map(|o| o.at);
        d.pill = text;
    }
    if !ctx.input(|i| i.pointer.primary_down()) {
        // Released: drop where the pointer is, or nowhere.
        if let Some(d) = app.state_mut().drag.take()
            && let (Some(at), Some(op)) = (d.at, d.op)
        {
            // A refusal's reason is on the status line.
            let _ = if d.faces.is_empty() { drop_at(app, at, op, d.ids) } else { drop_faces_at(app, at, d.faces) };
        }
        return;
    }
    // A drag of faces: its pill is ui-kit's to paint once it has words for faces.
    if faces > 0 {
        return;
    }
    let t = *app.tokens();
    let op = offered.as_ref().map(|o| o.op);
    if let (Some(DropAt::Tray), Some(r)) = (offered.as_ref().map(|o| o.at), tray) {
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("dalica-drag/tray")));
        widgets::drop_target(&painter, &t, r);
    }
    ctx.set_cursor_icon(if op == Some(DragOp::Copy) { CursorIcon::Copy } else { CursorIcon::Grabbing });
    if let Some(pos) = pos {
        let pill = DragPill { op: pill_op(op), count, target: name.as_deref(), thumb: None };
        let rect = widgets::drag_pill(ctx, &t, PILL_ID, pos, &pill);
        if let Some(id) = first {
            paint_thumb(p, app, ctx, rect, id, op == Some(DragOp::Copy));
        }
    }
}

/// The first photo's thumbnail on the pill (`rect`), as the light table shows it, letterboxed in
/// ui-kit's thumbnail square; a copy's + over it.
fn paint_thumb(p: &mut Panels, app: &App, ctx: &Context, rect: Rect, id: PhotoId, copy: bool) {
    let (Some(session), true) = (app.session(), rect.is_positive()) else { return };
    let Some(photo) = session.library().photo(id) else { return };
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new(PILL_ID))).with_clip_rect(rect);
    let square = Rect::from_min_size(pos2(rect.min.x + PILL_PAD, rect.center().y - PILL_THUMB / 2.0), vec2(PILL_THUMB, PILL_THUMB));
    let photo = PhotoRef { session, photo };
    let (w, h) = p.thumbnails.displayed(photo);
    p.thumbnails.paint(&painter, photo, fit_within(square, w, h), PILL_THUMB * ctx.pixels_per_point());
    if copy {
        // ui-kit's copy mark, drawn again over the thumbnail now on it.
        let t = app.tokens();
        let c = square.max - vec2(1.0, 5.0);
        painter.circle_filled(c, 6.0, t.accent);
        let stroke = Stroke::new(1.5, t.on_accent);
        painter.line_segment([c - vec2(3.0, 0.0), c + vec2(3.0, 0.0)], stroke);
        painter.line_segment([c - vec2(0.0, 3.0), c + vec2(0.0, 3.0)], stroke);
    }
}

/// A drag from the tray: its box's drag started this frame on a mini thumbnail (which senses
/// clicks only) or on the token.
fn start_from_tray(app: &mut App, ctx: &Context) {
    if ctx.drag_started_id() != Some(Id::new(TRAY_BOX)) {
        return;
    }
    let Some(origin) = ctx.input(|i| i.pointer.press_origin()) else { return };
    let hit = registry::widgets(ctx).into_iter().rev().find(|w| {
        let item = w.id.strip_prefix("tray:item:").is_some_and(|rest| !rest.contains('/'));
        (item || w.id == crate::tray::TOKEN_ID) && Rect::from_min_size(pos2(w.rect[0], w.rect[1]), vec2(w.rect[2], w.rect[3])).contains(origin)
    });
    let Some(hit) = hit else { return };
    let state = app.state();
    let ids = match hit.id.strip_prefix("tray:item:").and_then(|n| n.parse::<u64>().ok()).map(PhotoId::new) {
        Some(id) if state.tray.selected.contains(&id) => state.tray.selected.clone(),
        Some(id) => vec![id],
        None => state.selection.ids.clone(),
    };
    if !ids.is_empty() {
        start(app, hit.id, ids);
    }
}

/// Runs a drop of `ids` at `at` (what [`op_at`] said it does): a move or copy into a folder (the
/// photos already there left out) with the UI's questions, `album.add`, `album.reorder` or
/// `tray.hold`.
pub fn drop_at(app: &mut App, at: DropAt, op: DragOp, ids: Vec<PhotoId>) -> Result<Value, CmdError> {
    match at {
        DropAt::Folder(folder) => {
            let lib = app.session().map(Session::library);
            let ids: Vec<PhotoId> = ids.into_iter().filter(|id| lib.and_then(|l| l.photo(*id)).is_some_and(|p| p.folder != folder)).collect();
            if op == DragOp::Copy {
                crate::commands::files::copy_photos(app, ids, folder)
            } else {
                crate::commands::files::move_photos(app, ids, folder)
            }
        }
        DropAt::Album(key) => app.run("album.add", &json!({ "album": key.to_string(), "ids": ids })),
        DropAt::Tray => app.run("tray.hold", &json!({ "ids": ids })),
        DropAt::Gap { album, anchor, after } => {
            let command = if op == DragOp::Reorder { "album.reorder" } else { "album.add" };
            let side = if after { "after" } else { "before" };
            app.run(command, &json!({ "album": album.to_string(), "ids": ids, side: anchor }))
        }
        DropAt::Person(_) => Err(CmdError::Disabled("Photos hold no face to name; drag faces onto a person.".into())),
        DropAt::Map { lat_e6, lon_e6 } => {
            let at = [f64::from(lat_e6) / 1e6, f64::from(lon_e6) / 1e6];
            app.run("places.drop", &json!({ "ids": ids, "at": at }))
        }
    }
}

/// Runs a drop of `faces` at `at`: on a person's row, `faces.moveToPerson` names them.
pub fn drop_faces_at(app: &mut App, at: DropAt, faces: Vec<FaceRef>) -> Result<Value, CmdError> {
    match at {
        DropAt::Person(key) => app.run("faces.moveToPerson", &json!({ "person": key.to_string(), "faces": faces })),
        _ => Err(CmdError::Disabled("Faces can be dropped on a person only.".into())),
    }
}

/// `ui.inspect` → `drag`: the drag in progress, `null` when none.
pub fn inspect(app: &App) -> Value {
    app.state().drag.as_ref().and_then(|d| serde_json::to_value(d).ok()).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_drag_starts_reports_itself_and_ends() {
        let mut app = App::new(None);
        assert_eq!(inspect(&app), Value::Null);
        start(&mut app, "thumb:3".into(), vec![PhotoId::new(3), PhotoId::new(4)]);
        assert!(dragging(&app));
        assert_eq!(inspect(&app), json!({ "from": "thumb:3", "ids": [3, 4], "target": null, "op": null, "pill": "2 photos" }));
        end(&mut app);
        assert!(!dragging(&app));
        // Not part of ui.state.
        start(&mut app, "thumb:1".into(), vec![PhotoId::new(1)]);
        assert!(serde_json::to_value(app.state()).unwrap().get("drag").is_none());
    }

    #[test]
    fn the_pill_says_what_a_drop_does() {
        assert_eq!(pill(Some(DragOp::Move), 3, Some("Garden")), "Move 3 photos to Garden");
        assert_eq!(pill(Some(DragOp::Copy), 1, Some("Garden")), "Copy 1 photo to Garden");
        assert_eq!(pill(Some(DragOp::Add), 3, Some("Lisbon")), "Add 3 photos to Lisbon");
        assert_eq!(pill(Some(DragOp::Hold), 3, None), "Hold 3 photos in the tray");
        assert_eq!(pill(Some(DragOp::Reorder), 3, None), "Reorder 3 photos");
        assert_eq!(pill(None, 1204, None), "1,204 photos");
        assert_eq!(face_pill(None, 1, None), "1 face");
        assert_eq!(face_pill(Some(DragOp::Name), 2, Some("Ana")), "Name 2 faces Ana");
    }

    #[test]
    fn a_drag_of_faces_carries_faces_and_only_a_person_takes_them() {
        let mut app = App::new(None);
        let face = FaceRef { photo: PhotoId::new(3), index: 1 };
        start_faces(&mut app, face.widget(), vec![face]);
        assert_eq!(inspect(&app), json!({ "from": "face:3.1", "ids": [], "faces": ["3.1"], "target": null, "op": null, "pill": "1 face" }));
        let ana = crate::state::PersonKey::of(&dalica_core::PersonId::parse("p00a1").unwrap());
        let map = DropAt::Map { lat_e6: 38_711_600, lon_e6: -9_138_900 };
        assert_eq!(face_op_at(&[face], &DropAt::Person(ana)), Some(DragOp::Name));
        assert_eq!(face_op_at(&[face], &map), None);
        assert_eq!(face_op_at(&[face], &DropAt::Tray), None);
        assert_eq!(face_op_at(&[], &DropAt::Person(ana)), None);
        // Stubs until their tasks: the drops say why they did nothing, and change nothing.
        app.state_mut().drag = None;
        assert!(drop_faces_at(&mut app, DropAt::Person(ana), vec![face]).is_err());
        assert!(drop_faces_at(&mut app, map, vec![face]).is_err());
        assert!(drop_at(&mut app, DropAt::Person(ana), DragOp::Name, vec![PhotoId::new(3)]).is_err());
        let mut after = app.state().clone();
        after.status.clear();
        assert_eq!(after, App::new(None).state().clone());
    }
}
