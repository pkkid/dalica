//! A section's header block (`ui-layout.md` §5.2, `design-system.md` §7): `folder_header` fill
//! across the light table, the 2 pt `accent` bar of the selected section, and four rows:
//!
//! ```text
//! [icon 32]  Title (SemiBold 15)                              3 edited  [Save]
//!            Thursday, June 4, 2026 · place (12.5 text_dim)
//! [▶] [▦] │ [★] │ [💾] [⇪]         (26 × 22 icon buttons)
//! Add a description
//! ```
//!
//! and its sticky form, the first line only, 32 high, pinned over the cells.
//!
//! **Boundary.** The parent calls [`show`] once per header on screen, in painting order, with the
//! frame, this part's state ([`Headers`]), the section, the rect the layout gives it (full width ×
//! [`Spacing::header`](super::layout::Spacing::header), or × [`STICKY`](super::layout::STICKY) for
//! [`Form::Sticky`], whose top may be above the view while the next header pushes it up) and the
//! form. It paints inside the rect, registers `section:<id>` and its parts, and returns what the
//! header asks for ([`HeaderOut`]); the parent hands that to [`apply`] once the library is no
//! longer borrowed.
//!
//! **Ids** (`control-protocol.md` §7): `section:<id>` is the header (its right-click opens the
//! section menu), with the parts `/icon`, `/title`, `/description`, `/play`, `/collage`,
//! `/selectStarred`, `/save`, `/export` and, with unsaved edits, `/saveEdited` (the text button at
//! the top right). While the section's description is being edited inline the field
//! [`DESCRIPTION_FIELD`] (`field:section.description`) takes the place of `/description`. While a
//! section's header is pinned, its block has scrolled partly above the view
//! and lies under the pinned form: the **pinned form owns the ids** (`section:<id>` and its
//! first-line parts) and the block registers, and takes input on, only the parts below the pinned
//! band, so every id stays unique and nothing hidden takes a click.
//!
//! **Date, place and description** (`organizing.md` §10): the date line is the folder's date (the
//! one set in Folder Properties, else its oldest photo's) and " · place" when one is set; the last
//! row is the description, or "Add a description". A click on it opens the inline editor
//! ([`UiState::description_edit`](crate::state::UiState::description_edit)), which takes the focus
//! with the description as its text: Enter (or a click elsewhere) commits it through
//! `folder.info.set` (one undo step), Esc closes it and changes nothing. A double-click on the
//! title opens Folder Properties (`section.properties`).
//!
//! **Buttons** are live only when their command is registered (a UI command or one the session
//! has) and its `enabled` accepts the parameters a click passes; Select Starred also needs a starred
//! photo in the section, Save an unsaved edit. A disabled button says why in its tooltip.
//!
//! **Save** (`data-model.md` §6): with unsaved edits in the section's folder (hidden photos
//! included, `Library::unsaved_count`) the first line ends with "%d edited" and the Save text button
//! (`/saveEdited`, in the pinned form too); it and the disk button (`/save`, the `ui-kit`
//! [`OutputIcon::Save`]) run `photo.save` with those photos' ids, whatever is selected. The
//! command's first-time explanation asks through the dialog host
//! (`crate::dialogs::save_explain`). Their ids are worked out once per library revision.

use std::collections::HashMap;
use std::sync::Arc;

use dalica_core::{Date, FolderId, PhotoId};
use dalica_engine::Session;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::{self, Icon, OutputIcon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target, TextField};
use egui::text::{LayoutJob, TextWrapping};
use egui::{Align, Color32, CursorIcon, FontId, Galley, Id, Painter, Rect, Response, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::{Value, json};

use super::{Frame, Section};
use crate::app::App;
use crate::menus;
use crate::state;

/// The header's left (and right) padding (points).
pub const PADDING: f32 = 12.0;
/// The top of the first line in the block (points).
const TOP: f32 = 8.0;
/// Between the icon and the title.
const ICON_GAP: f32 = 8.0;
/// The title row's height, and the date line's top and height.
const TITLE_H: f32 = 20.0;
const DATE_TOP: f32 = 30.0;
const DATE_H: f32 = 16.0;
/// The button row's top; buttons are [`BUTTON_W`] × `section_button_h` (22), [`BUTTON_GAP`] apart,
/// groups [`GROUP_GAP`] apart with a 1 pt divider in the middle.
const BUTTONS_TOP: f32 = 46.0;
pub const BUTTON_W: f32 = 26.0;
const BUTTON_GAP: f32 = 2.0;
const GROUP_GAP: f32 = 12.0;
/// The glyph inside a header button.
const GLYPH: f32 = 14.0;
/// The description row's top and height.
const DESCRIPTION_TOP: f32 = 72.0;
const DESCRIPTION_H: f32 = 18.0;
/// The icon of the pinned form, which is one 32-high line.
pub const STICKY_ICON: f32 = 20.0;
/// What the description row says when the section has none.
pub const PLACEHOLDER: &str = "Add a description";
/// The label of the Save text button at the top right.
pub const SAVE_LABEL: &str = "Save";

/// What the first line says beside Save: "3 edited".
pub fn edited_text(count: usize) -> String {
    format!("{count} edited")
}
/// The inline description editor's automation id (`control-protocol.md` §7).
pub const DESCRIPTION_FIELD: &str = "field:section.description";
/// The widest the inline description editor gets (points).
const DESCRIPTION_FIELD_W: f32 = 720.0;
/// The most sections whose facts are kept; past it the cache starts over.
const MAX_FACTS: usize = 4096;

/// Which form of the header to paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Form {
    /// The full block above the section's grid.
    Block,
    /// Pinned at the top while the section scrolls under it: the first line only.
    Sticky,
}

/// A button of the header's button row, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Play,
    Collage,
    SelectStarred,
    Save,
    Export,
}

impl Button {
    /// The row, left to right.
    pub const ALL: [Button; 5] = [Button::Play, Button::Collage, Button::SelectStarred, Button::Save, Button::Export];

    /// Its part of the header's id: `section:<id>/<part>`.
    pub fn part(self) -> &'static str {
        match self {
            Button::Play => "play",
            Button::Collage => "collage",
            Button::SelectStarred => "selectStarred",
            Button::Save => "save",
            Button::Export => "export",
        }
    }

    /// The command a click runs (`menus-and-shortcuts.md`).
    pub fn command(self) -> &'static str {
        match self {
            Button::Play => "section.slideshow",
            Button::Collage => "create.collage",
            Button::SelectStarred => "select.starred",
            Button::Save => "photo.save",
            Button::Export => "output.export",
        }
    }

    /// Its tooltip (`ui-layout.md` §5.2).
    pub fn tooltip(self) -> &'static str {
        match self {
            Button::Play => "Play a slideshow of this folder",
            Button::Collage => "Make a collage from this folder",
            Button::SelectStarred => "Select the starred photos in this folder",
            Button::Save => "Save edited photos to disk",
            Button::Export => "Export these photos to a folder",
        }
    }

    /// The group it belongs to: groups are divided by a thin line.
    fn group(self) -> u8 {
        match self {
            Button::Play | Button::Collage => 0,
            Button::SelectStarred => 1,
            Button::Save | Button::Export => 2,
        }
    }
}

/// Whether a header button takes clicks: its command is `live` (registered, and its `enabled`
/// accepts the click's parameters), Select Starred also needs a starred photo, Save an unsaved
/// edit. Otherwise the reason, for its tooltip.
pub fn button_enabled(button: Button, live: bool, starred: bool, edited: usize) -> Result<(), &'static str> {
    if !live {
        return Err(NOT_YET);
    }
    match button {
        Button::SelectStarred if !starred => Err("No photo in this folder is starred."),
        Button::Save if edited == 0 => Err(NO_EDITS),
        _ => Ok(()),
    }
}

/// A button whose command is not there yet says so.
const NOT_YET: &str = "Not available yet.";
/// Save's tooltip when the folder has no unsaved edits.
pub const NO_EDITS: &str = "No photo in this folder has unsaved edits.";

/// The day of the week of `d` (proleptic Gregorian).
pub fn weekday(d: Date) -> &'static str {
    const NAMES: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    const OFFSETS: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let month = usize::from(d.month());
    let mut y = i32::from(d.year());
    if month < 3 {
        y -= 1;
    }
    let offset = OFFSETS.get(month.wrapping_sub(1)).copied().unwrap_or(0);
    let w = (y + y / 4 - y / 100 + y / 400 + offset + i32::from(d.day())).rem_euclid(7);
    NAMES.get(w as usize).copied().unwrap_or("")
}

/// The header's date line: the long date ("Thursday, June 4, 2026"), then " · place" when the
/// section has one.
pub fn date_line(date: Option<Date>, place: Option<&str>) -> String {
    let place = place.map(str::trim).filter(|p| !p.is_empty());
    match (date, place) {
        (Some(d), Some(p)) => format!("{}, {} · {p}", weekday(d), super::long_date(d)),
        (Some(d), None) => format!("{}, {}", weekday(d), super::long_date(d)),
        (None, Some(p)) => p.to_string(),
        (None, None) => String::new(),
    }
}

/// What a header shows that takes a pass over the section's photos, kept per section until the
/// sections are rebuilt.
#[derive(Debug, Clone, Default, PartialEq)]
struct Facts {
    /// The folder's full path (the icon's tooltip).
    path: String,
    /// The date line: the folder's date and place ([`date_line`]).
    date: String,
    /// The folder's description, as set (`""`: none); shared, as the facts are copied each frame.
    description: Arc<str>,
    /// The description on one line: line breaks it may hold (from another application) as spaces.
    shown: Arc<str>,
    /// The section holds a starred photo.
    starred: bool,
}

/// What the headers keep between frames: each section's [`Facts`] for the sections' generation,
/// each section's photos with unsaved edits for the library's revision, and the section whose
/// inline description editor was given the focus.
#[derive(Debug, Default)]
pub struct Headers {
    generation: Option<u64>,
    facts: HashMap<state::Section, Facts>,
    revision: Option<u64>,
    unsaved: HashMap<state::Section, Arc<[PhotoId]>>,
    focused: Option<state::Section>,
}

impl Headers {
    /// What the header of `section` showed when it was last drawn: its date line and its
    /// description (`""`: none, the placeholder shows).
    pub fn shown(&self, section: state::Section) -> Option<(&str, &str)> {
        self.facts.get(&section).map(|f| (f.date.as_str(), &*f.description))
    }

    /// The count of photos with unsaved edits the header of `section` showed ("2 edited") when it
    /// was last drawn in this library revision; 0: no count and no Save text button.
    pub fn edited(&self, section: state::Section) -> Option<usize> {
        self.unsaved.get(&section).map(|ids| ids.len())
    }

    fn facts(&mut self, session: &Session, generation: u64, section: &Section) -> Facts {
        if self.generation != Some(generation) || self.facts.len() >= MAX_FACTS {
            self.facts.clear();
            self.generation = Some(generation);
        }
        self.facts
            .entry(section.id)
            .or_insert_with(|| {
                let library = session.library();
                let path = library.folder(section.folder).map(|f| f.path.display().to_string()).unwrap_or_default();
                let info = library.folder_info(section.folder);
                // The date set in Folder Properties, else the oldest photo's (its effective date).
                let date = info
                    .date
                    .or_else(|| section.photos.iter().filter_map(|&id| library.photo(id).and_then(|p| library.sort_date(p))).min().map(|d| d.date()));
                let starred = section.photos.iter().any(|&id| library.photo(id).is_some_and(|p| p.star));
                Facts {
                    path,
                    date: date_line(date, Some(&info.place)),
                    description: Arc::from(info.description.as_str()),
                    shown: Arc::from(info.description.replace(['\n', '\r', '\t'], " ")),
                    starred,
                }
            })
            .clone()
    }

    /// The photos of `section`'s folder with unsaved edits (hidden ones included), in display
    /// order: what its Save saves. Worked out once per library revision.
    fn unsaved(&mut self, session: &Session, section: &Section) -> Arc<[PhotoId]> {
        let library = session.library();
        if self.revision != Some(library.revision()) || self.unsaved.len() >= MAX_FACTS {
            self.unsaved.clear();
            self.revision = Some(library.revision());
        }
        self.unsaved.entry(section.id).or_insert_with(|| Arc::from(library.unsaved_in(section.folder))).clone()
    }
}

/// The parameters of the section's Save: the ids of its photos with unsaved edits.
pub fn save_params(ids: &[PhotoId]) -> Value {
    json!({ "ids": ids })
}

/// Whether the section's Save takes clicks: it has photos with unsaved edits and `photo.save`
/// accepts them (none offline, none holding an edit Dalica cannot render). Otherwise why not, for
/// the tooltip.
pub fn save_enabled(session: &Session, ids: &[PhotoId]) -> Result<(), String> {
    if ids.is_empty() {
        return Err(NO_EDITS.into());
    }
    let spec = session.spec(Button::Save.command()).ok_or_else(|| NOT_YET.to_string())?;
    (spec.enabled)(session, &save_params(ids)).map_err(|d| d.0)
}

/// What the description row asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescriptionOut {
    /// A click on the description: open the inline editor on `text`, the folder's description.
    Open { section: state::Section, folder: FolderId, text: String },
    /// The editor's text changed.
    Draft(String),
    /// Enter, or a click elsewhere: apply `text` to the folder and close the editor.
    Commit { section: state::Section, folder: FolderId, text: String },
    /// Esc: close the editor; nothing changes.
    Cancel(state::Section),
}

/// What a header asks for.
#[derive(Debug, Default)]
pub struct HeaderOut {
    /// What the description row asks for.
    pub description: Option<DescriptionOut>,
    /// Commands to run, in order.
    pub commands: Vec<(String, Value)>,
    /// The section's folder when the header was right-clicked: it becomes the selected section,
    /// which the section menu acts on.
    pub select: Option<FolderId>,
    /// The header's responses that open the section menu on a right-click.
    pub menus: Vec<Response>,
}

/// Whether command `id` is registered here and accepts `params`.
fn live(session: &Session, id: &str, params: impl FnOnce() -> Value) -> bool {
    if crate::commands::UI_COMMANDS.iter().any(|c| c.id == id) {
        return true;
    }
    session.spec(id).is_some_and(|spec| (spec.enabled)(session, &params()).is_ok())
}

/// The parameters Play, Collage, Save and Export pass: the section, and the selection when it lies
/// inside it (they use it, else the whole section). Select Starred passes the section alone.
fn params(f: &Frame<'_>, section: &Section) -> Value {
    let sel = &f.state.selection;
    if sel.section == Some(section.id) && !sel.ids.is_empty() {
        json!({ "section": section.id, "ids": sel.ids })
    } else {
        json!({ "section": section.id })
    }
}

/// `text` on one line in `font`, cut short with "…" past `max` points.
fn one_line(painter: &Painter, text: &str, font: FontId, color: Color32, max: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping::truncate_at_width(if max.is_finite() { max.max(1.0) } else { 1.0 });
    painter.layout_job(job)
}

/// Paints a header button's glyph: an icon of the set, the disk of Save.
fn paint_glyph(painter: &Painter, rect: Rect, b: Button, color: Color32) {
    let icon = match b {
        Button::Play => Icon::Slideshow,
        Button::Collage => Icon::Collage,
        Button::SelectStarred => Icon::Star,
        Button::Export => Icon::Export,
        Button::Save => return icons::paint_output(painter, rect, OutputIcon::Save, color),
    };
    icons::paint(painter, rect, icon, color);
}

/// A 26 × 22 header button: `hover × 0.7` hovered, `pressed` held (radius 4), the glyph `icon` at
/// rest, `text` hovered, `text_disabled` disabled; the tooltip (and why, when disabled).
fn button(ui: &mut Ui, t: &Tokens, id: &str, egui_id: Id, rect: Rect, b: Button, enabled: Result<(), &str>) -> Response {
    let on = enabled.is_ok();
    let r = ui.interact(rect, egui_id, if on { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if on && r.is_pointer_button_down_on() {
        p.rect_filled(rect, t.radius, t.pressed);
    } else if on && r.hovered() {
        p.rect_filled(rect, t.radius, t.hover.gamma_multiply(0.7));
    }
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let color = if !on {
        t.text_disabled
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    paint_glyph(p, Rect::from_center_size(rect.center(), Vec2::splat(GLYPH)), b, color);
    widgets::register(ui.ctx(), id, Role::IconButton, b.tooltip(), Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, on, b.tooltip()));
    widgets::tooltip(r, t, b.tooltip(), enabled.err())
}

/// The rects of the button row of a block whose top-left is `origin` (buttons `height` high), left
/// to right, and the dividers between its groups.
pub fn button_rects(origin: egui::Pos2, height: f32) -> ([(Button, Rect); 5], [Rect; 2]) {
    let mut x = origin.x + PADDING;
    let y = origin.y + BUTTONS_TOP;
    let mut out = [(Button::Play, Rect::NOTHING); 5];
    let mut dividers = [Rect::NOTHING; 2];
    let mut group = 0;
    for (slot, b) in out.iter_mut().zip(Button::ALL) {
        if b.group() != group {
            // The divider sits in the middle of the group gap.
            let d = (x - BUTTON_GAP + GROUP_GAP / 2.0).round();
            if let Some(div) = dividers.get_mut(usize::from(group)) {
                *div = Rect::from_min_size(pos2(d, y + 4.0), vec2(1.0, (height - 8.0).max(1.0)));
            }
            x += GROUP_GAP - BUTTON_GAP;
            group = b.group();
        }
        *slot = (b, Rect::from_min_size(pos2(x, y), vec2(BUTTON_W, height)));
        x += BUTTON_W + BUTTON_GAP;
    }
    (out, dividers)
}

/// Paints `section`'s header in `rect` (see the module's boundary).
pub fn show(ui: &mut Ui, f: &Frame<'_>, headers: &mut Headers, section: &Section, rect: Rect, form: Form) -> HeaderOut {
    let t = f.t;
    let facts = headers.facts(f.session, f.sections.generation(), section);
    let unsaved = headers.unsaved(f.session, section);
    let save_state = if unsaved.is_empty() { Err(NO_EDITS.to_string()) } else { save_enabled(f.session, &unsaved) };
    // The editor closed (or moved to another section): it takes the focus again when it opens.
    if f.state.description_edit.as_ref().map(|e| e.section) != headers.focused {
        headers.focused = None;
    }
    let mut out = HeaderOut::default();
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0, t.folder_header);
    let selected = f.state.folder == Some(section.folder);
    if selected {
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0, t.accent);
    }
    // A block scrolled partly above the view lies under the pinned form, which owns the ids of
    // what it covers: only the parts below the pinned band register and take input.
    let band = f.area.min.y + f.layout.spacing().sticky;
    let covered = form == Form::Block && rect.min.y < f.area.min.y;
    let takes_input = |r: Rect| !covered || r.min.y >= band;
    let sid = format!("section:{}", section.id);
    let egui_id = |part: &str| Id::new(("lightTable.header", section.id, form, part.to_owned()));
    let part_id = |part: &str| format!("{sid}/{part}");
    let track = |r: &Response, out: &mut HeaderOut, menu: bool| {
        if r.secondary_clicked() {
            out.select = Some(section.folder);
        }
        if menu {
            out.menus.push(r.clone());
        }
    };

    // The header itself, first, so its parts sit above it and win their clicks.
    if !covered {
        let r = ui.interact(rect, egui_id(""), Sense::click());
        widgets::register(ui.ctx(), &sid, Role::Other, &section.title, Target::Widget(&r), selected);
        track(&r, &mut out, true);
    }

    // The first line: icon, title, and with unsaved edits "%d edited" and Save at the right.
    let (icon, line_top) = match form {
        Form::Block => (Rect::from_min_size(rect.min + vec2(PADDING, TOP), Vec2::splat(t.metrics.section_icon)), rect.min.y + TOP),
        Form::Sticky => {
            let icon = Rect::from_center_size(pos2(rect.min.x + PADDING + STICKY_ICON / 2.0, rect.center().y), Vec2::splat(STICKY_ICON));
            (icon, rect.center().y - TITLE_H / 2.0)
        }
    };
    let line_mid = line_top + TITLE_H / 2.0;
    let text_x = icon.max.x + ICON_GAP;
    let mut right = rect.max.x - PADDING;
    if !unsaved.is_empty() {
        let label = SAVE_LABEL;
        let w = (painter.layout_no_wrap(label.to_owned(), fonts::semibold(t.button_label), t.text).size().x + 18.0).max(37.0).round();
        let slot = Rect::from_min_size(pos2(right - w, (line_mid - t.metrics.button_h / 2.0).round()), vec2(w, t.metrics.button_h));
        if takes_input(slot) {
            let is_live = save_state.is_ok();
            let r = ui
                .scope_builder(UiBuilder::new().max_rect(slot).layout(egui::Layout::left_to_right(Align::Center)), |ui| {
                    if !is_live {
                        ui.disable();
                    }
                    widgets::text_button(ui, t, &part_id("saveEdited"), label, false)
                })
                .inner;
            let r = widgets::tooltip(r, t, Button::Save.tooltip(), save_state.as_ref().err().map(String::as_str));
            if r.clicked() {
                out.commands.push((Button::Save.command().to_string(), save_params(&unsaved)));
            }
            track(&r, &mut out, is_live);
        }
        let g = painter.layout_no_wrap(edited_text(unsaved.len()), fonts::regular(t.detail), t.text_dim);
        let x = slot.min.x - 8.0 - g.size().x;
        painter.galley(pos2(x, line_mid - g.size().y / 2.0).round(), g, t.text_dim);
        right = x - 12.0;
    }

    // The icon: Locate on Disk on a click (once the OS integration lands); its tooltip is the path.
    icons::paint(&painter, icon, Icon::Folder, t.icon);
    if takes_input(icon) {
        let section_params = || json!({ "section": section.id });
        let locate = live(f.session, "folder.locate", section_params);
        let r = ui.interact(icon, egui_id("icon"), if locate { Sense::click() } else { Sense::hover() });
        widgets::register(ui.ctx(), &part_id("icon"), Role::IconButton, &facts.path, Target::Widget(&r), false);
        let detail = if locate { "Locate on Disk" } else { "Locate on Disk is not available yet." };
        let r = widgets::tooltip(r, t, &facts.path, Some(detail));
        if r.clicked() {
            out.commands.push(("folder.locate".to_string(), section_params()));
        }
        track(&r, &mut out, locate);
    }

    // The title: double-click = Properties.
    let g = one_line(&painter, &section.title, fonts::semibold(t.title), t.text, right - text_x);
    let size = g.size();
    let title = Rect::from_min_size(pos2(text_x, line_top), vec2(size.x.max(1.0), TITLE_H));
    painter.galley(pos2(text_x, line_mid - size.y / 2.0).round(), g, t.text);
    if takes_input(title) {
        // Folder Properties for this section's folder, whatever is selected.
        let folder_params = || json!({ "folder": section.folder });
        let props = live(f.session, "section.properties", folder_params);
        let r = ui.interact(title, egui_id("title"), if props { Sense::click() } else { Sense::hover() });
        widgets::register(ui.ctx(), &part_id("title"), Role::Other, &section.title, Target::Widget(&r), false);
        if r.double_clicked() {
            out.commands.push(("section.properties".to_string(), folder_params()));
        }
        track(&r, &mut out, props);
    }
    if form == Form::Sticky {
        return out;
    }

    // The date line.
    if !facts.date.is_empty() {
        let g = one_line(&painter, &facts.date, fonts::regular(t.detail), t.text_dim, rect.max.x - PADDING - text_x);
        let y = rect.min.y + DATE_TOP + (DATE_H - g.size().y) / 2.0;
        painter.galley(pos2(text_x, y).round(), g, t.text_dim);
    }

    // The button row.
    let (buttons, dividers) = button_rects(rect.min, t.metrics.section_button_h);
    for d in dividers {
        painter.rect_filled(d, 0, t.button_border);
    }
    let mut row_params: Option<Value> = None;
    for (b, r) in buttons {
        if !takes_input(r) {
            paint_glyph(&painter, Rect::from_center_size(r.center(), Vec2::splat(GLYPH)), b, t.text_disabled);
            continue;
        }
        // Select Starred acts on the section alone, Save on its photos with unsaved edits; the
        // others also on a selection inside it.
        let p = match b {
            Button::SelectStarred => json!({ "section": section.id }),
            Button::Save => save_params(&unsaved),
            _ => row_params.get_or_insert_with(|| params(f, section)).clone(),
        };
        let enabled = match b {
            Button::Save => save_state.as_ref().map(|_| ()).map_err(String::as_str),
            _ => button_enabled(b, live(f.session, b.command(), || p.clone()), facts.starred, 0),
        };
        let resp = button(ui, t, &part_id(b.part()), egui_id(b.part()), r, b, enabled);
        if resp.clicked() {
            out.commands.push((b.command().to_string(), p));
        }
        track(&resp, &mut out, enabled.is_ok());
    }

    // The description line, or its inline editor.
    let editing = f.state.description_edit.as_ref().filter(|e| e.section == section.id);
    let x = rect.min.x + PADDING;
    let top = rect.min.y + DESCRIPTION_TOP;
    if let Some(e) = editing {
        let row = Rect::from_min_max(pos2(x, top), pos2(rect.max.x - PADDING, top + DESCRIPTION_H));
        out.description = description_editor(ui, f, headers, section, &e.draft, row, &takes_input);
        return out;
    }
    let (text, color) = if facts.shown.is_empty() { (PLACEHOLDER, t.text_dim) } else { (&*facts.shown, t.text_label) };
    let g = one_line(&painter, text, fonts::regular(t.detail), color, rect.max.x - PADDING - x);
    let d = Rect::from_min_size(pos2(x, top), vec2(g.size().x.max(1.0), DESCRIPTION_H));
    painter.galley(pos2(x, top + (DESCRIPTION_H - g.size().y) / 2.0).round(), g, color);
    if takes_input(d) {
        let r = ui.interact(d, egui_id("description"), Sense::click()).on_hover_cursor(CursorIcon::Text);
        widgets::register(ui.ctx(), &part_id("description"), Role::Other, text, Target::Widget(&r), false);
        if r.clicked() {
            out.description = Some(DescriptionOut::Open { section: section.id, folder: section.folder, text: facts.description.to_string() });
        }
        track(&r, &mut out, true);
    }
    out
}

/// The inline description editor of `section` over the description `row` (its text starts where
/// the description's does), as wide as the row allows up to [`DESCRIPTION_FIELD_W`]: the field
/// gets the focus the first frame it is shown; what its edit asks for comes back.
fn description_editor(
    ui: &mut Ui,
    f: &Frame<'_>,
    headers: &mut Headers,
    section: &Section,
    draft: &str,
    row: Rect,
    takes_input: &dyn Fn(Rect) -> bool,
) -> Option<DescriptionOut> {
    let t = f.t;
    let h = t.metrics.text_field_h;
    // The field's frame starts 8 before its text.
    let left = row.min.x - 8.0;
    let w = (row.max.x - left).clamp(24.0, DESCRIPTION_FIELD_W);
    let slot = Rect::from_min_size(pos2(left, (row.center().y - h / 2.0).round()), vec2(w, h));
    if !takes_input(slot) {
        return None;
    }
    if headers.focused != Some(section.id) {
        widgets::focus_field(ui.ctx(), DESCRIPTION_FIELD);
        headers.focused = Some(section.id);
    }
    let mut text = draft.to_owned();
    let field = ui
        .scope_builder(UiBuilder::new().max_rect(slot).layout(egui::Layout::left_to_right(Align::Center)), |ui| {
            TextField::new(DESCRIPTION_FIELD, PLACEHOLDER, w).show(ui, t, &mut text)
        })
        .inner;
    if field.cancelled() {
        Some(DescriptionOut::Cancel(section.id))
    } else if field.committed() {
        Some(DescriptionOut::Commit { section: section.id, folder: section.folder, text })
    } else if text != draft {
        Some(DescriptionOut::Draft(text))
    } else {
        None
    }
}

/// `text` cut to [`state::MAX_DRAFT`] characters, the most an inline editor keeps.
fn cut_draft(mut text: String) -> String {
    if let Some((at, _)) = text.char_indices().nth(state::MAX_DRAFT) {
        text.truncate(at);
    }
    text
}

/// Does what the description row asked for: opens the editor (a description longer than an inline
/// editor keeps opens Folder Properties instead, so nothing is cut), keeps its text, commits it
/// through `folder.info.set` or closes it.
fn apply_description(app: &mut App, d: DescriptionOut) {
    match d {
        DescriptionOut::Open { section, folder, text } => {
            if text.chars().count() > state::MAX_DRAFT {
                if let Err(e) = app.run("section.properties", &json!({ "folder": folder })) {
                    app.state_mut().status = e.to_string();
                }
                return;
            }
            app.state_mut().description_edit = Some(state::DescriptionEdit { section, draft: text });
        }
        DescriptionOut::Draft(text) => {
            if let Some(e) = app.state_mut().description_edit.as_mut() {
                e.draft = cut_draft(text);
            }
        }
        DescriptionOut::Commit { section, folder, text } => {
            let state = app.state_mut();
            if state.description_edit.as_ref().is_some_and(|e| e.section == section) {
                state.description_edit = None;
            }
            if let Err(e) = app.run("folder.info.set", &json!({ "id": folder, "description": text })) {
                app.state_mut().status = e.to_string();
            }
        }
        DescriptionOut::Cancel(section) => {
            let state = app.state_mut();
            if state.description_edit.as_ref().is_some_and(|e| e.section == section) {
                state.description_edit = None;
            }
        }
    }
}

/// Runs what a header asked for: the right-clicked section becomes the selected one, the clicked
/// commands run, and a right-click opens the section menu (`menus-and-shortcuts.md` §1.4) at the
/// header; items whose commands are not there yet are disabled.
pub fn apply(app: &mut App, out: HeaderOut) {
    if let Some(folder) = out.select {
        app.state_mut().folder = Some(folder);
    }
    if let Some(d) = out.description {
        apply_description(app, d);
    }
    for (id, params) in out.commands {
        let _ = app.run(&id, &params);
    }
    if let Some(r) = out.menus.iter().find(|r| r.secondary_clicked() || r.context_menu_opened()) {
        let nodes = menus::section_menu(app, menus::Platform::current());
        let _ = menus::context_menu(r, app, &nodes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: u16, m: u8, d: u8) -> Date {
        Date::new(y, m, d).unwrap()
    }

    #[test]
    fn an_inline_draft_is_cut_to_its_cap_on_a_char_boundary() {
        assert_eq!(cut_draft("trams".into()), "trams");
        let long = "é".repeat(state::MAX_DRAFT + 3);
        let cut = cut_draft(long);
        assert_eq!(cut.chars().count(), state::MAX_DRAFT);
        assert!(cut.chars().all(|c| c == 'é'));
    }

    #[test]
    fn weekdays_are_right_across_centuries_and_leap_days() {
        assert_eq!(weekday(date(2026, 6, 4)), "Thursday");
        assert_eq!(weekday(date(2000, 1, 1)), "Saturday");
        assert_eq!(weekday(date(2000, 2, 29)), "Tuesday");
        assert_eq!(weekday(date(1900, 3, 1)), "Thursday");
        assert_eq!(weekday(date(1970, 1, 1)), "Thursday");
        assert_eq!(weekday(date(2024, 12, 31)), "Tuesday");
    }

    #[test]
    fn the_date_line_is_long_with_the_place_when_set() {
        assert_eq!(date_line(Some(date(2026, 6, 4)), None), "Thursday, June 4, 2026");
        assert_eq!(date_line(Some(date(2026, 6, 4)), Some("Lisbon, Portugal")), "Thursday, June 4, 2026 · Lisbon, Portugal");
        assert_eq!(date_line(Some(date(2026, 6, 4)), Some("  ")), "Thursday, June 4, 2026");
        assert_eq!(date_line(None, Some("Lisbon")), "Lisbon");
        assert_eq!(date_line(None, None), "");
    }

    #[test]
    fn buttons_are_live_only_with_their_command_and_what_they_act_on() {
        for b in Button::ALL {
            assert!(button_enabled(b, false, true, 3).is_err(), "{b:?}: no command, no button");
        }
        assert!(button_enabled(Button::Play, true, false, 0).is_ok());
        assert!(button_enabled(Button::Collage, true, false, 0).is_ok());
        assert!(button_enabled(Button::Export, true, false, 0).is_ok());
        assert!(button_enabled(Button::SelectStarred, true, false, 0).is_err());
        assert!(button_enabled(Button::SelectStarred, true, true, 0).is_ok());
        assert!(button_enabled(Button::Save, true, true, 0).is_err());
        assert!(button_enabled(Button::Save, true, false, 2).is_ok());
    }

    #[test]
    fn the_button_row_is_26_by_22_in_three_groups() {
        let (row, dividers) = button_rects(pos2(0.0, 0.0), 22.0);
        assert_eq!(row.map(|(b, _)| b), Button::ALL);
        assert!(row.iter().all(|(_, r)| r.size() == vec2(26.0, 22.0) && r.min.y == BUTTONS_TOP));
        assert_eq!(row[0].1.min.x, PADDING);
        for w in row.windows(2) {
            assert!(w[1].1.min.x > w[0].1.max.x, "{w:?}: in order, never overlapping");
        }
        for (d, (before, after)) in dividers.iter().zip([(1, 2), (2, 3)]) {
            assert!(d.min.x > row[before].1.max.x && d.max.x < row[after].1.min.x, "{d:?} between the groups");
        }
        assert!(row[4].1.max.y <= DESCRIPTION_TOP && DESCRIPTION_TOP + DESCRIPTION_H <= 96.0, "the rows fit the 96 block");
    }
}
