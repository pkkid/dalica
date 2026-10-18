//! Suggestions in a person's section (`organizing.md` §6.3; the copy they come from is
//! `crate::commands::suggestions`):
//!
//! - **The strip** ([`show`]), in the row a folder's description takes (a person's header has
//!   none), while the person has suggestions or Find More's faces waiting, or shows only them:
//!   "Suggestions:" with [Confirm All] (`section:<id>/confirmAll`), [Confirm] (`/confirm`) and
//!   [Reject] (`/reject`, on the faces of the photos selected in the section), [Find More]
//!   (`/findMore`), the checkbox "Show only suggestions" (`/onlySuggestions`) and the hint "Click ✓
//!   to confirm a match or × to reject it." while there is room for it.
//! - **The section's photos** ([`ForSections::photos`]): the photos of the person's suggestions
//!   first, then of Find More's faces, then (unless Show only suggestions is on) their own.
//! - **A suggested photo's cell** ([`cell`]) shows the suggested face as a face tile filling the
//!   square (`face:<PhotoId>.<n>/in/<PersonId>`, as a photo's cell is `thumb:<PhotoId>/in/…`, so
//!   the People pane's tile of the same face keeps its id), "Ana?" under it and ✓ / × (that id's
//!   `/confirm` and `/reject`): ✓ confirms it
//!   (the face joins the person's photos, one undo step), × rejects it for good. A click on the
//!   tile selects the photo as a thumbnail's click does; a double-click opens it.

use dalica_core::{PersonId, PhotoId};
use dalica_library::{Library, TreeOptions};
use dalica_ui_kit::fonts;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, FACE_LABEL_H, FACE_MARKS_H, FACE_TILE_MIN, FACE_TILE_PAD, FaceLabel, FaceTile, Target};
use egui::{Align, Align2, Id, Layout, Rect, Sense, Ui, UiBuilder, pos2};
use serde_json::{Value, json};

use super::Frame;
use super::cell::{CellIn, CellOut, Shown};
use crate::commands::suggestions::{self as cmd, Kind, Snapshot};
use crate::panels::people_pane::pictures;
use crate::panels::thumbnails::Drawn;
use crate::state::{self, FaceRef, PersonKey};

/// The strip's top in the header block (points): the row under the buttons.
pub const TOP: f32 = 70.0;
// Below the 22-high button row at 46.
const _: () = assert!(TOP >= 68.0);
/// What the strip says.
pub const LABEL: &str = "Suggestions:";
pub const HINT: &str = "Click ✓ to confirm a match or × to reject it.";
pub const ONLY: &str = "Show only suggestions";
/// The strip's buttons: their part of the header's id, label and command.
pub const BUTTONS: [(&str, &str, &str); 4] = [
    ("confirmAll", "Confirm All", "suggestions.confirmAll"),
    ("confirm", "Confirm", "suggestions.confirm"),
    ("reject", "Reject", "suggestions.reject"),
    ("findMore", "Find More", "suggestions.findMore"),
];
/// The checkbox's part of the header's id.
pub const ONLY_PART: &str = "onlySuggestions";

/// What the person sections take in from the suggestions.
#[derive(Debug, Clone, Copy, Default)]
pub struct ForSections<'a> {
    pub snapshot: Option<&'a Snapshot>,
    /// The people whose section shows only their suggestions.
    pub only: &'a [PersonId],
}

/// What the person sections were built from: the copy's generation and Show only suggestions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionsKey {
    generation: u64,
    only: Vec<PersonId>,
}

impl ForSections<'_> {
    pub fn key(&self) -> SectionsKey {
        SectionsKey { generation: self.snapshot.map_or(0, |s| s.generation), only: self.only.to_vec() }
    }

    /// The photos of `person`'s section: the suggested ones first (those the tree options show),
    /// then `own` (theirs, in the section's order) unless the section shows only its suggestions.
    pub fn photos(&self, library: &Library, options: &TreeOptions, person: PersonKey, own: Vec<PhotoId>) -> Vec<PhotoId> {
        let Some(id) = person.id() else { return own };
        let only = self.only.contains(&id);
        let Some(s) = self.snapshot.filter(|s| s.has(&id)) else { return if only { Vec::new() } else { own } };
        let shows = |p: &PhotoId| {
            library.photo(*p).is_some()
                && (options.hidden_pictures || !library.is_hidden(*p))
                && options.filter.as_ref().is_none_or(|f| f.contains(*p))
        };
        let mut out: Vec<PhotoId> = s.photos(&id).into_iter().filter(shows).collect();
        if !only {
            let seen: std::collections::HashSet<PhotoId> = out.iter().copied().collect();
            out.extend(own.into_iter().filter(|p| !seen.contains(p)));
        }
        out
    }
}

/// Whether `person`'s header shows the strip.
pub fn shows_strip(f: &Frame<'_>, person: &PersonId) -> bool {
    cmd::latest().has(person) || f.state.people.only_suggestions.contains(person)
}

/// Draws the strip of `person`'s section in `row` (the header's width less its padding,
/// `button_h` high); returns the commands it asks for.
pub fn show(ui: &mut Ui, f: &Frame<'_>, section: state::Section, person: &PersonId, row: Rect) -> Vec<(String, Value)> {
    let t = f.t;
    let mut out = Vec::new();
    let snapshot = cmd::latest();
    let selected = f.state.selection.section == Some(section) && f.state.selection.ids.iter().any(|id| snapshot.on_photo(person, *id).is_some());
    let mut strip = ui.new_child(UiBuilder::new().id_salt(("suggestions.strip", section)).max_rect(row).layout(Layout::left_to_right(Align::Center)));
    strip.set_clip_rect(row.intersect(ui.clip_rect()));
    strip.spacing_mut().item_spacing.x = 6.0;
    let painter = strip.painter().clone();
    let label = painter.layout_no_wrap(LABEL.to_owned(), fonts::regular(t.detail), t.text_label);
    let (_, at) = strip.allocate_space(label.size());
    painter.galley(at.min, label, t.text_label);
    for (part, text, command) in BUTTONS {
        let on = match part {
            "confirmAll" => snapshot.pending(person) > 0,
            "confirm" | "reject" => selected,
            _ => true,
        };
        let id = format!("section:{section}/{part}");
        let r = strip
            .scope(|ui| {
                if !on {
                    ui.disable();
                }
                widgets::text_button(ui, t, &id, text, false)
            })
            .inner;
        if r.clicked() {
            // Confirm and Reject act on the faces of the photos selected here, as the person's.
            out.push((command.to_string(), json!({ "person": person })));
        }
    }
    strip.add_space(8.0);
    let was = f.state.people.only_suggestions.contains(person);
    let mut now = was;
    widgets::checkbox(&mut strip, t, &format!("section:{section}/{ONLY_PART}"), ONLY, &mut now);
    if now != was {
        out.push(("suggestions.only".to_string(), json!({ "person": person, "on": now })));
    }
    // The hint, where there is room for it.
    let left = strip.cursor().min.x + 10.0;
    let hint = painter.layout_no_wrap(HINT.to_owned(), fonts::regular(t.detail), t.text_dim);
    if left + hint.size().x <= row.max.x {
        painter.galley(pos2(left, row.center().y - hint.size().y / 2.0).round(), hint, t.text_dim);
    }
    out
}

/// The id of `face`'s tile in `person`'s section (the tile registers `face:` and it).
pub fn tile_id(face: FaceRef, person: &PersonId) -> String {
    format!("{face}/in/{person}")
}

/// What a suggested photo's cell drew and asks for.
pub struct CellDrawn {
    pub out: CellOut,
    pub shown: Shown,
    pub commands: Vec<(&'static str, Value)>,
}

/// The cell of `c`'s photo in `person`'s section when a face on it is suggested (or Find More's)
/// for them: the face as a tile with "Name?" and ✓ / × (see the module's doc); `None` otherwise.
pub fn cell(ui: &mut Ui, f: &Frame<'_>, c: &CellIn<'_>, person: &PersonId, (selected, anchor): (bool, bool), alpha: f32) -> Option<CellDrawn> {
    let snapshot = cmd::latest();
    let (face, kind) = snapshot.on_photo(person, c.photo.id)?;
    let library = f.session.library();
    let rect = library.face(dalica_library::FaceRef { photo: face.photo, index: usize::from(face.index) })?.rect;
    let name = library.person(person).map(|p| p.name.clone()).unwrap_or_default();
    let t = f.t;
    let id = c.photo.id;
    let ctx = ui.ctx().clone();
    let response = ui.interact(c.rect, Id::new(("thumb", id.get(), c.section)), Sense::hover());
    widgets::register(&ctx, &super::cell::thumb_id(c.section, id), Role::Thumbnail, &c.photo.name, Target::Widget(&response), selected);
    let side = (c.rect.width() - 2.0 * FACE_TILE_PAD - FACE_LABEL_H - FACE_MARKS_H).max(FACE_TILE_MIN);
    let picture = pictures::picture(&ctx, face, rect, pictures::side_px(&ctx, side));
    let face_id = tile_id(face, person);
    let tile = FaceTile { side, picture, label: FaceLabel::Suggested(&name), selected, focused: anchor, marks: true, ..FaceTile::new(&face_id) };
    let size = widgets::face_tile_size(side, true, true);
    let at = Align2::CENTER_CENTER.align_size_within_rect(size, c.rect);
    let mut child =
        ui.new_child(UiBuilder::new().id_salt(("suggestionCell", face_id.as_str())).max_rect(at).layout(Layout::left_to_right(Align::Min)));
    child.multiply_opacity(alpha);
    let out = widgets::face_tile(&mut child, t, &tile);
    let mut commands = Vec::new();
    let mods = ui.input(|i| i.modifiers);
    let section = json!(c.section);
    if out.response.clicked() {
        let sel = &f.state.selection;
        let here = sel.ids.is_empty() || sel.section == Some(c.section);
        let mode = if mods.shift {
            "range"
        } else if mods.command && here {
            "toggle"
        } else {
            "replace"
        };
        commands.push(("select.set", json!({ "ids": [id.get()], "mode": mode, "section": section })));
    }
    if out.response.double_clicked() {
        commands.push(("view.edit", json!({ "id": id.get() })));
    }
    // A face Find More listed is named as (or rejected for) the person; a suggestion as suggested.
    let mut params = json!({ "faces": [face] });
    if kind == Kind::More {
        params["person"] = json!(person);
    }
    if out.confirm {
        commands.push(("suggestions.confirm", params.clone()));
    } else if out.reject {
        commands.push(("suggestions.reject", params));
    }
    let drawn = if picture.is_some() { Drawn::Image } else { Drawn::Placeholder };
    let shown = Shown { photo: id, caption: None, selected, opacity: alpha, badges: widgets::Badges::default(), drawn, tooltip: None };
    Some(CellDrawn { out: CellOut { photo: id, section: c.section, rect: c.rect, image: out.picture, response, drawn }, shown, commands })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_fits_under_the_buttons_inside_the_block() {
        let t = dalica_ui_kit::Tokens::default();
        assert!(TOP + t.metrics.button_h <= t.metrics.section_h, "inside the block");
    }
}
