//! The Properties pane (`ui-layout.md` §7): what is known about the selection, in the side pane.
//! It follows the selection: its heading names what it describes ("Properties of IMG_0361.jpg",
//! "Properties of 12 photos", the folder's name for a section selected as a unit; "No selection",
//! with nothing under it), then [Show All Metadata] (`button:properties.showAll`,
//! [`crate::state::UiState::properties_show_all`], `ui.set {propertiesShowAll}`), then the rows in a
//! list that scrolls with the wheel ([`paint`]).
//!
//! - **One photo** ([`answer`]): the rows of `photo.metadata` in its order (Path, File size,
//!   Dimensions as displayed after the recipe, Date taken, Date digitized, File modified, Camera,
//!   Lens, Exposure, Aperture, ISO, Focal length, Flash, Orientation, Color space, GPS, Caption,
//!   Tags, and the file's rating when it has one). What the library knows shows at once; while the
//!   engine's workers read the file a "Reading the file…" row follows, and the rest arrive a few
//!   frames later without any frame waiting for the file. A field another application changed
//!   carries the note "Changed in another application; Dalica's value kept". Show All Metadata
//!   adds every field of the file below the rows, grouped as the file holds them (EXIF, GPS, IPTC,
//!   XMP); "Show Fewer" takes them away.
//! - **Several photos or a folder** ([`summary`]): what the library knows of them (Pictures,
//!   Dates, Size on disk, Tags; a folder adds its Path, Place and Description). Show All Metadata
//!   asks for one photo.
//!
//! What the last frame showed is kept for tests and inspection ([`shown`]).

pub mod answer;
pub mod paint;
pub mod summary;

use std::sync::Arc;

use dalica_core::{FolderId, PhotoId};
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets;
use egui::{Align, Id, Layout, Rect, Sense, Ui, UiBuilder, vec2};

use self::answer::{Answer, Group, Row};
use self::paint::{Item, Painted, Tone};
use crate::app::App;
use crate::state::{Section, UiState};

/// The Show All switch's widget id.
pub const SHOW_ALL_ID: &str = "button:properties.showAll";
/// The row shown while the file is being read.
pub const LOADING: &str = "Reading the file…";
/// What Show All says for several photos.
pub const ONE_PHOTO: &str = "Select one photo to see all of its metadata.";
/// What Show All says for a file with no other metadata.
pub const NOTHING_MORE: &str = "The file holds no other metadata.";
/// What Show All says when the engine listed only the first fields.
pub const TRUNCATED: &str = "Only the first fields are listed.";

/// The pane's heading for `state`: the photo's name, the number of photos, the section's folder,
/// or "No selection".
pub fn heading(app: &App, state: &UiState) -> String {
    let sel = &state.selection;
    let library = app.session().map(dalica_engine::Session::library);
    let folder_name = |s: &Section| {
        let Section::Folder(f) = s;
        library
            .and_then(|l| l.folder(*f))
            .map(|f| f.path.file_name().map_or_else(|| f.path.display().to_string(), |n| n.to_string_lossy().into_owned()))
    };
    if sel.whole
        && let Some(name) = sel.section.as_ref().and_then(folder_name)
    {
        return name;
    }
    match sel.ids.as_slice() {
        [] => "No selection".into(),
        [one] => match library.and_then(|l| l.photo(*one)) {
            Some(p) => format!("Properties of {}", p.name),
            None => "Properties of 1 photo".into(),
        },
        many => format!("Properties of {} photos", many.len()),
    }
}

/// What the pane describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subject {
    Nothing,
    Photo(PhotoId),
    /// Several photos, or a folder selected as a unit (`Some`).
    Photos(Option<FolderId>),
}

/// What the pane describes for `state`.
pub fn subject(state: &UiState) -> Subject {
    let sel = &state.selection;
    let folder = sel.section.as_ref().filter(|_| sel.whole).map(|Section::Folder(f)| *f);
    match (sel.ids.as_slice(), folder) {
        ([], None) => Subject::Nothing,
        ([one], None) => Subject::Photo(*one),
        (_, folder) => Subject::Photos(folder),
    }
}

/// One row as the last frame laid it out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShownRow {
    pub key: String,
    pub label: String,
    /// The whole value (what the tooltip shows when it was cut).
    pub value: String,
    pub note: Option<String>,
    /// Where it was painted and whether its value was cut; `None` when scrolled out of view.
    pub painted: Option<(Rect, bool)>,
}

/// What the pane showed in the last frame it was drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    pub heading: String,
    pub rows: Vec<ShownRow>,
    /// The "Reading the file…" row was shown.
    pub loading: bool,
    /// Why the file could not be read.
    pub error: Option<String>,
    /// A note about the pane rather than one row (a changed field no row shows).
    pub note: Option<String>,
    /// Show All Metadata's groups, each with its fields.
    pub groups: Vec<(String, Vec<ShownRow>)>,
    /// A Show All message ([`ONE_PHOTO`], [`NOTHING_MORE`], [`TRUNCATED`]).
    pub message: Option<String>,
    /// The frame (egui pass) it was drawn in.
    pub pass: u64,
}

fn drawn_key() -> Id {
    Id::new("properties.drawn")
}

/// What a frame drew, kept by reference (nothing copied per frame): [`shown`] reads it out.
#[derive(Clone, Default)]
struct Drawn {
    heading: String,
    answer: Option<Arc<Answer>>,
    summary: Option<Arc<Vec<Row>>>,
    all: bool,
    message: Option<&'static str>,
    /// Where the rows and Show All's groups start among the painted items.
    rows_at: usize,
    groups_at: usize,
    painted: Vec<Option<Painted>>,
    pass: u64,
}

fn shown_row(r: &Row, painted: Option<Painted>) -> ShownRow {
    ShownRow {
        key: r.key.clone(),
        label: r.label.clone(),
        value: r.value.clone(),
        note: r.note.clone(),
        painted: painted.map(|p| (p.rect, p.elided)),
    }
}

/// What the pane showed in the last frame it was drawn, if it was ever drawn.
pub fn shown(ctx: &egui::Context) -> Option<Shown> {
    let d = ctx.data(|data| data.get_temp::<Drawn>(drawn_key()))?;
    let at = |n: usize| d.painted.get(n).copied().flatten();
    let rows: &[Row] = match (&d.answer, &d.summary) {
        (Some(a), _) => &a.rows,
        (None, Some(s)) => s,
        (None, None) => &[],
    };
    let mut groups = Vec::new();
    if d.all {
        let mut n = d.groups_at;
        for g in d.answer.as_ref().and_then(|a| a.groups.as_ref()).into_iter().flatten() {
            n += 1;
            let mut fields = Vec::with_capacity(g.rows.len());
            for r in &g.rows {
                fields.push(shown_row(r, at(n)));
                n += 1;
            }
            groups.push((g.name.clone(), fields));
        }
    }
    Some(Shown {
        heading: d.heading.clone(),
        rows: rows.iter().enumerate().map(|(n, r)| shown_row(r, at(d.rows_at + n))).collect(),
        loading: d.answer.as_ref().is_some_and(|a| a.pending),
        error: d.answer.as_ref().and_then(|a| a.error.clone()),
        note: d.answer.as_ref().and_then(|a| a.note.clone()),
        groups,
        message: d.message.map(str::to_owned),
        pass: d.pass,
    })
}

/// Draws the pane's body into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let rect = ui.max_rect().shrink(8.0);
    let mut col = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
    let mut drawn = Drawn { heading: heading(app, app.state()), pass: ctx.cumulative_pass_nr(), ..Drawn::default() };
    // The heading, cut with "…" when long, in full in a tooltip.
    let font = fonts::semibold(t.body);
    let (head, cut) = paint::fit(col.painter(), &drawn.heading, &font, t.text, rect.width(), false);
    let (head_rect, response) = col.allocate_exact_size(vec2(rect.width(), head.size().y), Sense::hover());
    col.painter().galley(head_rect.min, head, t.text);
    if cut {
        widgets::tooltip(response, &t, &drawn.heading, None);
    }
    let subject = subject(app.state());
    if subject == Subject::Nothing {
        ctx.data_mut(|d| d.insert_temp(drawn_key(), drawn));
        return;
    }
    col.add_space(8.0);
    let all = app.state().properties_show_all;
    let label = if all { "Show Fewer" } else { "Show All Metadata" };
    if widgets::text_button(&mut col, &t, SHOW_ALL_ID, label, all).clicked() {
        app.state_mut().properties_show_all = !all;
    }
    col.add_space(4.0);

    let (answer, summary) = match subject {
        Subject::Photo(id) => (Some(answer::answer(app, &ctx, id, all)), None),
        Subject::Photos(folder) => (None, Some(summary::kept(app, &ctx, folder))),
        Subject::Nothing => (None, None),
    };
    let rows: &[Row] = match (&answer, &summary) {
        (Some(a), _) => &a.rows,
        (None, Some(s)) => s,
        (None, None) => &[],
    };
    let error = answer.as_ref().and_then(|a| a.error.as_ref()).map(|e| format!("Could not read the file: {e}"));
    let mut items: Vec<Item<'_>> = Vec::with_capacity(rows.len() + 4);
    if let Some(note) = answer.as_ref().and_then(|a| a.note.as_deref()) {
        items.push(Item::Line(note, Tone::Caution));
    }
    drawn.rows_at = items.len();
    items.extend(rows.iter().map(Item::Row));
    if answer.as_ref().is_some_and(|a| a.pending) {
        items.push(Item::Line(LOADING, Tone::Dim));
    }
    if let Some(e) = &error {
        items.push(Item::Line(e, Tone::Caution));
    }
    let groups: &[Group] = answer.as_ref().and_then(|a| a.groups.as_deref()).unwrap_or(&[]);
    drawn.message = match (&answer, all) {
        (_, false) => None,
        (None, true) => Some(ONE_PHOTO),
        (Some(a), true) if a.groups.as_ref().is_some_and(Vec::is_empty) => Some(NOTHING_MORE),
        (Some(a), true) if a.truncated => Some(TRUNCATED),
        (Some(_), true) => None,
    };
    drawn.groups_at = items.len();
    if all {
        for g in groups {
            items.push(Item::Group(&g.name));
            items.extend(g.rows.iter().map(Item::Field));
        }
    }
    if let Some(m) = drawn.message {
        items.push(Item::Line(m, Tone::Dim));
    }
    drawn.painted = paint::list(&mut col, &t, &items);
    drop(items);
    drawn.all = all;
    drawn.answer = answer;
    drawn.summary = summary;
    ctx.data_mut(|d| d.insert_temp(drawn_key(), drawn));
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn the_heading_names_the_selection() {
        let (_dir, s, ids) = session_with_photos(3);
        let name = s.library().photo(ids[0]).unwrap().name.clone();
        let mut app = App::new(Some(s));
        assert_eq!(heading(&app, app.state()), "No selection");
        app.run("select.set", &json!({ "ids": [ids[0].get()] })).unwrap();
        assert_eq!(heading(&app, app.state()), format!("Properties of {name}"));
        app.run("select.set", &json!({ "ids": [ids[0].get(), ids[1].get()] })).unwrap();
        assert_eq!(heading(&app, app.state()), "Properties of 2 photos");
        let bare = App::new(None);
        let mut state = UiState::default();
        state.selection.set_ids(vec![ids[0]]);
        assert_eq!(heading(&bare, &state), "Properties of 1 photo");
    }

    #[test]
    fn the_subject_follows_the_selection() {
        let mut state = UiState::default();
        assert_eq!(subject(&state), Subject::Nothing);
        let (a, b) = (PhotoId::new(1), PhotoId::new(2));
        state.selection.set_ids(vec![a]);
        assert_eq!(subject(&state), Subject::Photo(a));
        state.selection.set_ids(vec![a, b]);
        assert_eq!(subject(&state), Subject::Photos(None));
        let f = FolderId::new(7);
        state.selection = crate::state::Selection::new(vec![a], None, Some(Section::Folder(f)), true);
        assert_eq!(subject(&state), Subject::Photos(Some(f)), "a folder of one photo is still the folder");
        state.selection = crate::state::Selection::new(vec![], None, Some(Section::Folder(f)), true);
        assert_eq!(subject(&state), Subject::Photos(Some(f)), "an empty folder too");
        state.selection = crate::state::Selection::new(vec![a], None, Some(Section::Folder(f)), false);
        assert_eq!(subject(&state), Subject::Photo(a));
    }
}
