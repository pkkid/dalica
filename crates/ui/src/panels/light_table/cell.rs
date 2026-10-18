//! A thumbnail cell (`ui-layout.md` §5.3–§5.5, `design-system.md` §7): a square of the cell size
//! with the photo letterboxed in it (never cropped, never rounded) over its drop shadow; the
//! selection's `cell_selected` surround and 2 pt outline (`photo_focus` on the anchor,
//! `photo_selected` on the rest); the hover surround; the badges and video marks
//! ([`widgets::badges`], fed what the model knows); the caption line under the cell, middle-elided;
//! the `caution` "!" of a photo that cannot be shown; the inline caption editor under the cell it
//! is open for ([`caption_edit`]); and, after the pointer has rested on the cell
//! for [`TOOLTIP_SECONDS`], the tooltip (file name; date taken, pixel size, file size; or why the
//! photo cannot be shown, with its format's hint), unless Options ▸ Show tooltips is off (`interface.tooltips`, which
//! `crate::scale` turns into egui's infinite tooltip delay). A hidden photo is drawn at [`HIDDEN_OPACITY`] when View ▸ Hidden
//! Pictures shows it ([`opacity`]).
//!
//! **Boundary.** The parent calls [`show`] once per cell on screen, after the section's header,
//! with the frame, this part's state ([`Cells`]), the [`Thumbnails`] and a [`CellIn`]: the photo,
//! its section, the square on screen and (captions on) the caption line under it. The parent has
//! already left out what is not shown (small pictures unless View ▸ Small Pictures). The cell senses
//! clicks, registers `thumb:<PhotoId>` and its star badge `thumb:<PhotoId>/star` ([`thumb_id`]:
//! in an album section `thumb:<PhotoId>/in/<album>`, as a photo of an album is also shown in its
//! folder's section; the selection outline and the caption editor show on the copy in the
//! selection's section, [`is_primary`]), asks
//! [`Thumbnails::paint`] for the image and returns a [`CellOut`] (its response) for
//! [`super::input`] to act on. Once the cells are drawn, the parent calls [`apply`], which applies
//! what the caption editor did. What the last pass drew is kept for tests and inspection
//! ([`Cells::shown`], [`Cells::tooltip`]).
//!
//! **Zoom to Faces** (`organizing.md` §6.3): in a person's section zoomed to their faces
//! (`people.zoomToFaces`), a cell shows the person's face on the photo as a face tile filling the
//! square (`face:<PhotoId>.<n>`, from the shared face picture store) in place of the photo. The
//! tile takes the cell's input itself: a click selects the photo as a thumbnail's click does
//! (Ctrl toggles, Shift selects the range), a double-click opens it, a right-click selects it when
//! it was not selected and opens the face menu (`crate::panels::faces::menu`) for the person's
//! faces on the photos selected, and a drag carries those faces (onto a person in the tree:
//! `crate::drag::start_faces`). [`apply`] runs what the tiles asked for.

use std::collections::{HashMap, HashSet};

use dalica_core::{PersonId, PhotoId};
use dalica_library::{Library, Photo};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Badge, Badges, FACE_TILE_MIN, FACE_TILE_PAD, FaceTile, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Align2, Id, Layout, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, UiBuilder, vec2};
use serde_json::{Value, json};

use super::caption_edit::{self, Editor};
use super::header::face_on;
use super::{Frame, long_date};
use crate::app::App;
use crate::panels::people_pane::pictures;
use crate::panels::thumbnails::{Drawn, PhotoRef, Thumbnails};
use crate::state::{self, Caption, FaceRef};
use crate::status_text as status;

/// How long the pointer rests on a cell before its tooltip shows (seconds, `ui-layout.md` §5.5).
pub const TOOLTIP_SECONDS: f64 = 0.6;
/// The opacity of a hidden photo shown by View ▸ Hidden Pictures (`ui-layout.md` §5.3).
pub const HIDDEN_OPACITY: f32 = 0.4;
/// Lists of ids (the tray's held photos, the selection) up to this long are searched as they are;
/// longer ones are put in a set once a pass, so a cell's lookup stays O(1).
const SCAN: usize = 32;
/// The most fitted captions kept between passes (a screenful is far fewer).
const FITTED_CAP: usize = 4096;

/// The tooltip a cell shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tip {
    /// The file name.
    pub title: String,
    /// Date taken, pixel size and file size; or why the photo cannot be shown.
    pub detail: String,
}

/// What one cell drew in the last pass that drew cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub photo: PhotoId,
    /// The caption line as drawn (middle-elided); `None` with captions off.
    pub caption: Option<String>,
    /// Drawn as selected (the surround and the outline).
    pub selected: bool,
    /// 1, or [`HIDDEN_OPACITY`] for a hidden photo.
    pub opacity: f32,
    /// What the badges were fed.
    pub badges: Badges,
    /// What [`Thumbnails::paint`] drew.
    pub drawn: Drawn,
    /// The tooltip, when it was showing.
    pub tooltip: Option<Tip>,
}

/// The cell under the pointer and since when.
#[derive(Debug, Clone, Copy)]
struct Hover {
    photo: PhotoId,
    /// The frame clock when the pointer came onto it (or last pressed it).
    since: f64,
    /// The pass that last saw it hovered.
    pass: u64,
}

/// What the cells keep between frames.
#[derive(Debug, Default)]
pub struct Cells {
    /// The egui pass the records below belong to.
    pass: Option<u64>,
    hover: Option<Hover>,
    shown: Vec<Shown>,
    /// The tray's held photos and the selection, as sets when they are long.
    held: IdSet,
    selected: IdSet,
    /// Fitted captions: photo → (full text, width bits, fitted text).
    fitted: HashMap<PhotoId, (String, u32, String)>,
    /// The inline caption editor.
    editor: Editor,
    /// What the Zoom to Faces tiles asked for this pass, for [`apply`].
    faces: FaceAsks,
}

/// What the Zoom to Faces tiles of a pass ask for.
#[derive(Debug, Default)]
struct FaceAsks {
    /// Commands to run, in order.
    commands: Vec<(&'static str, Value)>,
    /// A drag of faces starting from a tile: its widget id and the person.
    drag: Option<(String, FaceRef, PersonId)>,
    /// The tile whose face menu was asked for or is open, with the person.
    menu: Option<(Response, PersonId)>,
}

impl Cells {
    /// What each cell drew in the last pass that drew cells, in drawing order.
    pub fn shown(&self) -> &[Shown] {
        &self.shown
    }

    /// What `photo`'s cell drew in that pass.
    pub fn get(&self, photo: PhotoId) -> Option<&Shown> {
        self.shown.iter().find(|s| s.photo == photo)
    }

    /// Whether the inline caption editor was drawn in the last pass that drew cells.
    pub fn editor_shown(&self) -> bool {
        self.pass.is_some_and(|p| self.editor.drawn_in(p))
    }

    /// The tooltip showing in that pass, with its photo.
    pub fn tooltip(&self) -> Option<(PhotoId, &Tip)> {
        self.shown.iter().find_map(|s| s.tooltip.as_ref().map(|t| (s.photo, t)))
    }

    /// Starts a new pass on the first cell of one: forgets the last pass's records and refreshes
    /// the held and selected sets.
    fn begin(&mut self, pass: u64, held: &[PhotoId], selected: &[PhotoId]) {
        if self.pass == Some(pass) {
            return;
        }
        self.pass = Some(pass);
        self.shown.clear();
        self.held.refresh(held);
        self.selected.refresh(selected);
        if self.fitted.len() > FITTED_CAP {
            self.fitted.clear();
        }
    }

    /// Whether the pointer has rested on `photo` for [`TOOLTIP_SECONDS`], counting from when it
    /// came onto the cell or last pressed it. Asks for a repaint when the tooltip is still to come.
    /// Never with Show tooltips off: the setting makes egui's tooltip delay infinite
    /// (`crate::scale`), and the cell's own timer follows it.
    fn rested(&mut self, ui: &Ui, photo: PhotoId, response: &Response, pass: u64, now: f64) -> bool {
        if !response.hovered() || !ui.style().interaction.tooltip_delay.is_finite() {
            return false;
        }
        let pressed = response.is_pointer_button_down_on() || response.clicked() || response.dragged();
        let since = match self.hover {
            Some(h) if h.photo == photo && h.pass.saturating_add(1) >= pass && !pressed => h.since,
            _ => now,
        };
        self.hover = Some(Hover { photo, since, pass });
        let waited = now - since;
        if waited.is_finite() && waited + 1e-9 >= TOOLTIP_SECONDS {
            return true;
        }
        let left = if waited.is_finite() { (TOOLTIP_SECONDS - waited).max(0.0) } else { TOOLTIP_SECONDS };
        ui.ctx().request_repaint_after_secs(left as f32);
        false
    }

    /// `text` fitted to `max` points in `font`, middle-elided, remembered per photo.
    fn fit(&mut self, painter: &Painter, photo: PhotoId, text: &str, max: f32, font: &egui::FontId) -> String {
        let bits = max.to_bits();
        if let Some((full, w, fitted)) = self.fitted.get(&photo)
            && full == text
            && *w == bits
        {
            return fitted.clone();
        }
        let fitted = fit_middle(text, max, |s| painter.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE).size().x);
        self.fitted.insert(photo, (text.to_owned(), bits, fitted.clone()));
        fitted
    }
}

/// A list of photo ids looked up per cell: searched as it is when short, else through a set
/// rebuilt once a pass when the list changed.
#[derive(Debug, Default)]
struct IdSet {
    list: Vec<PhotoId>,
    set: HashSet<PhotoId>,
}

impl IdSet {
    fn refresh(&mut self, list: &[PhotoId]) {
        if list.len() > SCAN && self.list != list {
            self.list = list.to_vec();
            self.set = list.iter().copied().collect();
        }
    }

    /// Whether `list` (the one last refreshed with) holds `id`.
    fn contains(&self, list: &[PhotoId], id: PhotoId) -> bool {
        if list.len() > SCAN { self.set.contains(&id) } else { list.contains(&id) }
    }
}

/// One cell to paint.
pub struct CellIn<'a> {
    pub photo: &'a Photo,
    pub section: state::Section,
    /// The square on screen.
    pub rect: Rect,
    /// The caption line under it, when captions are on.
    pub caption: Option<Rect>,
}

/// What a cell reports.
pub struct CellOut {
    pub photo: PhotoId,
    pub section: state::Section,
    /// The square on screen.
    pub rect: Rect,
    /// The letterboxed image inside it.
    pub image: Rect,
    pub response: Response,
    pub drawn: Drawn,
}

/// The image's rect inside a square `cell`: letterboxed to its aspect (`width × height`; unknown
/// sizes fill the square).
pub fn letterbox(cell: Rect, width: u32, height: u32) -> Rect {
    let aspect = if width > 0 && height > 0 { width as f32 / height as f32 } else { 1.0 };
    let size = if aspect >= 1.0 { vec2(cell.width(), cell.width() / aspect) } else { vec2(cell.height() * aspect, cell.height()) };
    Rect::from_center_size(cell.center(), size)
}

/// The hidden-photo rule (`ui-layout.md` §5.3): a photo that is not hidden is drawn opaque; a
/// hidden one at [`HIDDEN_OPACITY`] when View ▸ Hidden Pictures is on, and not at all when it is
/// off (`None`).
pub fn opacity(hidden: bool, hidden_pictures: bool) -> Option<f32> {
    match (hidden, hidden_pictures) {
        (false, _) => Some(1.0),
        (true, true) => Some(HIDDEN_OPACITY),
        (true, false) => None,
    }
}

/// Whether `photo` is hidden (Picture ▸ Hide; `data-model.md` §3).
pub fn is_hidden(library: &Library, photo: &Photo) -> bool {
    library.is_hidden(photo.id)
}

/// What `photo`'s badges show: what the model knows. Starred, held, edited when it has edits not
/// yet saved into its file, rotations and flips included (`edited`, [`Thumbnails::edited`], the
/// folder header's "n edited" test), and geotagged by its effective geotag
/// ([`Library::is_geotagged`], `organizing.md` §7: the library's word when it has one, a point
/// set in Dalica or a location removed, else its file's GPS as the scan read it; a lookup per cell
/// drawn, never the file); videos once the model carries them.
pub fn badges_of(photo: &Photo, library: &Library, edited: bool, held: bool) -> Badges {
    Badges { starred: photo.star, edited, geotagged: library.is_geotagged(photo.id), held, ..Badges::default() }
}

/// The caption line's text for `mode` (`ui-layout.md` §5.3), before fitting: the file name; the
/// effective date (the date set by the user, else the date taken, else the file's date); the pixel
/// size; the photo's caption; its tags joined with ", " in the order added. Empty for a size, date,
/// caption or tags the photo does not have. `None` with captions off. The pixel size is `shown`,
/// the size the photo shows at (its recipe's turns included,
/// [`crate::panels::thumbnails::Thumbnails::displayed`]).
pub fn caption_of(photo: &Photo, library: &Library, mode: Caption, shown: (u32, u32)) -> Option<String> {
    Some(match mode {
        Caption::Resolution => status::pixels(shown.0, shown.1).unwrap_or_default(),
        Caption::Caption => library.caption(photo.id).to_owned(),
        Caption::Tags => library.tags_of(photo.id).join(", "),
        Caption::Date => library.sort_date(photo).map(|d| long_date(d.date())).unwrap_or_default(),
        _ => return caption_text(photo, mode),
    })
}

/// [`caption_of`] for the modes that come from the photo's file alone (file name, date, size);
/// Caption and Tags, which the library holds, are empty here.
pub fn caption_text(photo: &Photo, mode: Caption) -> Option<String> {
    Some(match mode {
        Caption::None => return None,
        Caption::Filename => photo.name.clone(),
        Caption::Date => photo.sort_date().map(|d| long_date(d.date())).unwrap_or_default(),
        Caption::Resolution => resolution(photo).unwrap_or_default(),
        Caption::Caption | Caption::Tags => String::new(),
    })
}

/// `4000 × 3000`, when the size is known.
fn resolution(photo: &Photo) -> Option<String> {
    (photo.width > 0 && photo.height > 0).then(|| format!("{} × {}", photo.width, photo.height))
}

/// The hover tooltip (`ui-layout.md` §5.5): the file name over the date taken (else the file's
/// date), the pixel size and the file size; for a photo that cannot be shown, why, and on the next
/// line what to do about it when its format has a hint (`dalica_image::formats::hint`). The pixel size
/// is the file's; [`tooltip_shown`] gives the size the photo shows at.
pub fn tooltip_text(photo: &Photo, failure: Option<&str>) -> Tip {
    tooltip_dated(photo, photo.sort_date(), failure)
}

/// [`tooltip_text`] showing `date`: the cells pass the effective date ([`Library::sort_date`]).
pub fn tooltip_dated(photo: &Photo, date: Option<dalica_core::DateTime>, failure: Option<&str>) -> Tip {
    tooltip_shown(photo, date, (photo.width, photo.height), failure)
}

/// [`tooltip_dated`] with the pixel size `shown`: the size the photo shows at, its recipe's turns
/// included (what a cell shows).
pub fn tooltip_shown(photo: &Photo, date: Option<dalica_core::DateTime>, shown: (u32, u32), failure: Option<&str>) -> Tip {
    let detail = match failure {
        // What to do about it under the reason, where there is something (HEIC in a build
        // without its decoder, `formats.md` §3).
        Some(why) => match dalica_image::formats::hint_for(why) {
            Some(hint) => format!("Cannot show this photo: {why}\n{hint}"),
            None => format!("Cannot show this photo: {why}"),
        },
        None => {
            let mut parts: Vec<String> = Vec::with_capacity(3);
            parts.extend(date.map(status::date_time));
            parts.extend(status::pixels(shown.0, shown.1));
            parts.push(status::size(photo.size));
            parts.join(" · ")
        }
    };
    Tip { title: photo.name.clone(), detail }
}

/// `text` cut in the middle with "…" until `width` of it is at most `max` (the most characters
/// that fit); unchanged when it fits; empty when not even "…" fits.
pub fn fit_middle(text: &str, max: f32, width: impl Fn(&str) -> f32) -> String {
    if width(text) <= max {
        return text.to_owned();
    }
    let n = text.chars().count();
    if n == 0 || width("…") > max || !max.is_finite() {
        return String::new();
    }
    // The most characters kept that still fit, by bisection (width grows with what is kept).
    let (mut lo, mut hi) = (0usize, n - 1);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if width(&status::elide_middle(text, mid)) <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    status::elide_middle(text, lo)
}

/// How a cell looks, apart from its pixels and caption.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// The square on screen.
    pub cell: Rect,
    /// The letterboxed image in it.
    pub image: Rect,
    pub selected: bool,
    /// The selection's anchor (the focused photo).
    pub anchor: bool,
    pub hovered: bool,
    pub badges: Badges,
}

/// The widget id of photo `id`'s cell in `section` (`control-protocol.md` §7): `thumb:<PhotoId>`
/// in a folder's section, `thumb:<PhotoId>/in/<album>` (`a91c0e`, `starred`, `recent`, `search`) in
/// an album's, so the copies of a photo shown in two sections stay apart.
pub fn thumb_id(section: state::Section, id: PhotoId) -> String {
    match section {
        state::Section::Folder(_) => format!("thumb:{}", id.get()),
        // A person's section: `thumb:12/in/p7f3a2c`.
        state::Section::Album(state::AlbumSection::Person(k)) => format!("thumb:{}/in/{k}", id.get()),
        state::Section::Album(_) => {
            let album = section.to_string();
            format!("thumb:{}/in/{}", id.get(), album.trim_start_matches("album:"))
        }
    }
}

/// Whether the copy of a photo in `section` is the one the selection outline and the caption
/// editor show on: the copy in the selection's album section while the selection is in an album,
/// else the folder's copy.
pub fn is_primary(section: state::Section, selection: &state::Selection) -> bool {
    match selection.section {
        Some(s) if s.is_album() => s == section,
        _ => !section.is_album(),
    }
}

/// Paints one cell (see the module's boundary).
pub fn show(ui: &mut Ui, f: &Frame<'_>, cells: &mut Cells, thumbs: &mut Thumbnails, c: &CellIn<'_>) -> CellOut {
    let t = f.t;
    let id = c.photo.id;
    let pass = ui.ctx().cumulative_pass_nr();
    let held = &f.state.tray.held;
    let selection = &f.state.selection;
    cells.begin(pass, held, &selection.ids);
    // Letterboxed to the size it shows at: a landscape turned a quarter is a portrait.
    let (width, height) = thumbs.displayed(PhotoRef { session: f.session, photo: c.photo });
    let image = letterbox(c.rect, width, height);
    let Some(alpha) = opacity(is_hidden(f.session.library(), c.photo), f.state.hidden_pictures) else {
        // Not shown: the parent leaves hidden photos out; this is the cell's own guard.
        let response = ui.interact(c.rect, Id::new(("thumb", id.get(), c.section)), Sense::hover());
        return CellOut { photo: id, section: c.section, rect: c.rect, image, response, drawn: Drawn::Placeholder };
    };
    let primary = is_primary(c.section, selection);
    let selected = cells.selected.contains(&selection.ids, id) && primary;
    // A person's section zoomed to their faces: the face tile in the photo's place.
    if let Some(person) = c.section.person().filter(|p| f.state.people.zoom_to_faces.contains(p))
        && let Some(face) = face_on(f.session.library(), id, &person)
    {
        let anchor = selection.anchor == Some(id) && primary;
        return face_cell(ui, f, cells, c, (face, person), (selected, anchor), alpha);
    }
    let response = ui.interact(c.rect, Id::new(("thumb", id.get(), c.section)), Sense::click());
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(alpha);
    let look = Look {
        cell: c.rect,
        image,
        selected,
        anchor: selection.anchor == Some(id) && primary,
        hovered: response.hovered(),
        badges: badges_of(
            c.photo,
            f.session.library(),
            thumbs.edited(PhotoRef { session: f.session, photo: c.photo }),
            cells.held.contains(held, id),
        ),
    };
    paint_under(&painter, t, &look);
    let device_px = image.width().max(image.height()) * f.pixels_per_point;
    let drawn = thumbs.paint(&painter, PhotoRef { session: f.session, photo: c.photo }, image, device_px);
    let marks = paint_over(&painter, t, &look, drawn == Drawn::Failed);
    let editing = f.state.caption_edit.as_ref().filter(|e| e.photo == id && primary);
    let caption = match (c.caption, caption_of(c.photo, f.session.library(), f.state.caption, (width, height))) {
        // The editor takes the caption line's place.
        (Some(_), Some(_)) if editing.is_some() => None,
        (Some(line), Some(text)) => {
            let font = fonts::regular(t.caption);
            let fitted = cells.fit(&painter, id, &text, (line.width() - 4.0).max(0.0), &font);
            painter.text(line.center(), Align2::CENTER_CENTER, &fitted, font, t.text_dim);
            Some(fitted)
        }
        _ => None,
    };
    if let Some(star) = marks.get(Badge::Starred) {
        let target = Target::Painted { rect: star, clip: painter.clip_rect() };
        widgets::register(ui.ctx(), &format!("{}/star", thumb_id(c.section, id)), Role::Badge, Badge::Starred.label(), target, true);
    }
    widgets::register(ui.ctx(), &thumb_id(c.section, id), Role::Thumbnail, &c.photo.name, Target::Widget(&response), selected);
    let tooltip = cells.rested(ui, id, &response, pass, f.now).then(|| {
        let date = f.session.library().sort_date(c.photo);
        let tip = tooltip_shown(c.photo, date, (width, height), if drawn == Drawn::Failed { thumbs.failure(id) } else { None });
        widgets::tooltip_now(&response, t, &tip.title, Some(&tip.detail));
        tip
    });
    if let Some(edit) = editing {
        caption_edit::show(ui, f, &mut cells.editor, edit, c.rect, pass);
    }
    cells.shown.push(Shown { photo: id, caption, selected, opacity: alpha, badges: look.badges, drawn, tooltip });
    CellOut { photo: id, section: c.section, rect: c.rect, image, response, drawn }
}

/// A cell of a section zoomed to its person's faces: `face`, theirs on the photo, as a face tile
/// filling the square, which takes the cell's input (see the module's doc).
fn face_cell(
    ui: &mut Ui,
    f: &Frame<'_>,
    cells: &mut Cells,
    c: &CellIn<'_>,
    (face, person): (FaceRef, PersonId),
    (selected, anchor): (bool, bool),
    alpha: f32,
) -> CellOut {
    let t = f.t;
    let id = c.photo.id;
    let ctx = ui.ctx().clone();
    // The cell itself takes no input: the tile over it does.
    let response = ui.interact(c.rect, Id::new(("thumb", id.get(), c.section)), Sense::hover());
    widgets::register(&ctx, &thumb_id(c.section, id), Role::Thumbnail, &c.photo.name, Target::Widget(&response), selected);
    let side = (c.rect.width() - 2.0 * FACE_TILE_PAD).max(FACE_TILE_MIN);
    let library = f.session.library();
    let rect = library.face(dalica_library::FaceRef { photo: face.photo, index: usize::from(face.index) }).map(|x| x.rect);
    let picture = rect.and_then(|r| pictures::picture(&ctx, face, r, pictures::side_px(&ctx, side)));
    let dragged = f.state.drag.as_ref().is_some_and(|d| d.faces.contains(&face));
    let face_id = face.to_string();
    let tile = FaceTile { side, picture, selected, focused: anchor, dragged, ..FaceTile::new(&face_id) };
    let mut child = ui.new_child(UiBuilder::new().id_salt(("faceCell", face_id.as_str())).max_rect(c.rect).layout(Layout::left_to_right(Align::Min)));
    child.multiply_opacity(alpha);
    let out = widgets::face_tile(&mut child, t, &tile);
    let r = &out.response;
    let mods = ui.input(|i| i.modifiers);
    let sel = &f.state.selection;
    let section = json!(c.section);
    if r.clicked() {
        let here = sel.ids.is_empty() || sel.section == Some(c.section);
        let mode = if mods.shift {
            "range"
        } else if mods.command && here {
            "toggle"
        } else {
            "replace"
        };
        cells.faces.commands.push(("select.set", json!({ "ids": [id.get()], "mode": mode, "section": section })));
    } else if r.secondary_clicked() && !selected {
        cells.faces.commands.push(("select.set", json!({ "ids": [id.get()], "mode": "replace", "section": section })));
    }
    if r.double_clicked() {
        cells.faces.commands.push(("view.edit", json!({ "id": id.get() })));
    }
    if r.drag_started() {
        cells.faces.drag = Some((face.widget(), face, person.clone()));
    }
    if r.secondary_clicked() || r.context_menu_opened() {
        cells.faces.menu = Some((r.clone(), person));
    }
    let drawn = if picture.is_some() { Drawn::Image } else { Drawn::Placeholder };
    cells.shown.push(Shown { photo: id, caption: None, selected, opacity: alpha, badges: Badges::default(), drawn, tooltip: None });
    CellOut { photo: id, section: c.section, rect: c.rect, image: out.picture, response, drawn }
}

/// The faces of `person` on the photos selected in their section (at most [`state::MAX_IDS`]):
/// what the face menu and a drag from a Zoom to Faces cell act on.
fn selected_faces(app: &App, person: &PersonId) -> Vec<FaceRef> {
    let Some(session) = app.session() else { return Vec::new() };
    let sel = &app.state().selection;
    if sel.section != Some(state::Section::Album(state::AlbumSection::Person(state::PersonKey::of(person)))) {
        return Vec::new();
    }
    sel.ids.iter().filter_map(|id| face_on(session.library(), *id, person)).take(state::MAX_IDS).collect()
}

/// Applies what the cells' parts did this frame, once the cells are drawn and the library is no
/// longer borrowed: the caption editor's typing, commit or cancel ([`caption_edit::apply`]), and
/// what the Zoom to Faces tiles asked for (the selection, a drag of faces, the face menu).
pub fn apply(app: &mut App, cells: &mut Cells) {
    caption_edit::apply(app, &mut cells.editor, cells.pass);
    let asks = std::mem::take(&mut cells.faces);
    for (id, params) in asks.commands {
        let _ = app.run(id, &params);
    }
    if let Some((from, face, person)) = asks.drag {
        // The faces selected when the face is among them, else that face alone.
        let selected = selected_faces(app, &person);
        let faces = if selected.contains(&face) { selected } else { vec![face] };
        crate::drag::start_faces(app, from, faces);
    }
    if let Some((r, person)) = asks.menu {
        // The face menu acts on the faces selected: the person's on the photos selected.
        let faces = selected_faces(app, &person);
        app.state_mut().unnamed.selection = faces;
        crate::panels::faces::menu::show(&r, app);
    }
}

/// What goes under the image: the selected or hovered surround, and the photo's drop shadow.
pub fn paint_under(painter: &Painter, t: &Tokens, look: &Look) {
    if look.selected {
        painter.rect_filled(look.cell, 0, t.cell_selected);
    } else if look.hovered {
        painter.rect_filled(look.cell, 0, t.hover.gamma_multiply(0.5));
    }
    widgets::photo_shadow(painter, look.image);
}

/// What goes over the image: the selection outline, the "!" of a photo that cannot be shown, the
/// badges and video marks. Returns where the marks went.
pub fn paint_over(painter: &Painter, t: &Tokens, look: &Look, failed: bool) -> widgets::BadgeRects {
    if look.selected {
        let color = if look.anchor { t.photo_focus } else { t.photo_selected };
        painter.rect_stroke(look.image, 0, Stroke::new(2.0, color), StrokeKind::Outside);
    }
    if failed {
        widgets::failed_mark(painter, t, look.image);
    }
    widgets::badges(painter, t, look.image, &look.badges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn photos_are_letterboxed_never_cropped() {
        let cell = Rect::from_min_size(pos2(0.0, 0.0), vec2(144.0, 144.0));
        assert_eq!(letterbox(cell, 480, 320).size(), vec2(144.0, 96.0));
        assert_eq!(letterbox(cell, 320, 480).size(), vec2(96.0, 144.0));
        assert_eq!(letterbox(cell, 0, 0).size(), vec2(144.0, 144.0));
    }

    #[test]
    fn middle_elision_keeps_the_most_that_fits() {
        // One point per character.
        let w = |s: &str| s.chars().count() as f32;
        assert_eq!(fit_middle("IMG_0361.jpg", 12.0, w), "IMG_0361.jpg");
        assert_eq!(fit_middle("IMG_0361.jpg", 8.0, w), "IMG_…jpg");
        assert_eq!(fit_middle("IMG_0361.jpg", 1.0, w), "…");
        assert_eq!(fit_middle("IMG_0361.jpg", 0.5, w), "");
        assert_eq!(fit_middle("", 0.0, w), "");
        assert_eq!(fit_middle("ñandú.jpg", 5.0, w), "ña…pg", "characters, never bytes");
        assert_eq!(fit_middle("abc", f32::NAN, w), "");
    }

    /// The Resolution caption and the tooltip give the size the photo shows at: a landscape
    /// turned a quarter by its recipe reads as a portrait.
    #[test]
    fn a_turned_photo_shows_its_displayed_size() {
        let p = Photo {
            id: PhotoId::new(1),
            folder: dalica_core::FolderId::new(1),
            name: "IMG_0361.jpg".into(),
            size: 3_100_000,
            mtime: Default::default(),
            width: 4000,
            height: 3000,
            date: None,
            star: false,
        };
        let lib = Library::new();
        assert_eq!(caption_of(&p, &lib, Caption::Resolution, (3000, 4000)).as_deref(), Some("3000 × 4000"));
        assert_eq!(caption_of(&p, &lib, Caption::Resolution, (0, 0)).as_deref(), Some(""));
        assert_eq!(caption_of(&p, &lib, Caption::Filename, (3000, 4000)).as_deref(), Some("IMG_0361.jpg"));
        let tip = tooltip_shown(&p, None, (3000, 4000), None);
        assert!(tip.detail.contains("3000 × 4000") && !tip.detail.contains("4000 × 3000"), "{}", tip.detail);
        assert!(tooltip_text(&p, None).detail.contains("4000 × 3000"), "the file's size");
    }
}
