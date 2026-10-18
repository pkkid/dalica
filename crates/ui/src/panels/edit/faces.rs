//! Face boxes on the photo in the edit view (`organizing.md` §6.4), run after the canvas
//! (`panels/edit.rs`) while no tool is open and one photo is shown:
//!
//! - **The faces' boxes** show while the People pane is open (or Add a Person Manually's box is
//!   drawn): each face of the photo that is not ignored as ui-kit's `face_box` (`facebox:<PhotoId>.<n>`,
//!   labelled with its person's name), its rectangle taken from the stored frame
//!   (`data-model.md` §3.3) onto the photo as shown through the engine's one mapping ([`Shown`]:
//!   the photo's geometry, its rotations, flips, straighten and crop, frames left out; as
//!   `faces.list`'s `displayed`), so a box stays on its face however the photo is turned or
//!   cropped. A box is highlighted while its face is hovered in the People pane
//!   (`peoplePane.hovered`), and hovering a box highlights its face in the pane.
//! - **Add a Person Manually** (`peoplePane.manual`, `faces.addManually`): an adjustable box
//!   (`facebox:manual`, its eight handles `facebox:manual/nw` … `/w`, moved by a drag inside it)
//!   in the photo's own normalised coordinates as shown, and under it (above it when there is no
//!   room) a plate with "Drag the box to fit the face, then type a name.", the name field
//!   (`field:faces.manual`, autocomplete over the people), [Add] (`button:faces.manual.add`) and
//!   [Cancel] (`button:faces.manual.cancel`). Enter or Add stores the face
//!   ([`crate::commands::people_pane::store_manual`]): the box is taken back to the stored frame
//!   ([`stored_rect`]) and nothing is stored before a name is given.

use dalica_core::{PhotoId, Rect01};
use dalica_engine::cmd::faces::Shown;
use dalica_library::FacePerson;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::{self, Ending, FaceBox, ToastKind, face_box_rect};
use egui::{Align, CornerRadius, Layout, Rect, RichText, Ui, UiBuilder, pos2, vec2};

use super::canvas;
use crate::app::App;
use crate::commands::{edit_view, people_pane, tools};
use crate::panels::people_pane as pane;
use crate::state::{Compare, FaceRef, Pane, is_unit_rect};

/// Add a Person Manually's box and its plate's controls.
pub const MANUAL_ID: &str = "facebox:manual";
pub const NAME_ID: &str = "field:faces.manual";
pub const ADD_ID: &str = "button:faces.manual.add";
pub const CANCEL_ID: &str = "button:faces.manual.cancel";
/// What the plate says.
pub const INSTRUCTION: &str = "Drag the box to fit the face, then type a name.";
/// The name field's hint.
pub const NAME_HINT: &str = "Type a name";
/// The plate's width and its padding (points).
const PLATE_W: f32 = 300.0;
const PLATE_PAD: f32 = 8.0;
/// Between the box and the plate (points).
const PLATE_GAP: f32 = 6.0;

/// The automation id of face `face`'s box: `facebox:<PhotoId>.<n>`.
pub fn box_id(face: FaceRef) -> String {
    format!("facebox:{face}")
}

/// The photo as the edit view shows it: the engine's one mapping of face rectangles
/// ([`Shown`]), over the window's own recipe of `photo` (its variant's while one is shown); `None`
/// without a library, for a photo it lacks or one without a size.
pub fn shown(app: &App, photo: PhotoId) -> Option<Shown> {
    let p = app.session()?.library().photo(photo)?;
    Shown::new(&tools::recipe(app, photo), p.width, p.height)
}

/// Where the photo lies on the screen, the displayed picture drawn at `image`.
fn on_screen(shown: &Shown, image: Rect) -> Rect {
    let [x0, y0, x1, y1] = shown.area();
    let at = |x: f64, y: f64| pos2(image.min.x + x as f32 * image.width(), image.min.y + y as f32 * image.height());
    Rect::from_min_max(at(x0, y0), at(x1, y1))
}

/// `photo`'s stored rectangle under the box `shown` (`[x0, y0, x1, y1]` normalised to the photo as
/// the edit view shows it).
pub fn stored_rect(app: &App, photo: PhotoId, box_: [f32; 4]) -> Option<Rect01> {
    shown(app, photo)?.stored_rect(box_.map(f64::from))
}

/// The faces of `photo` that are not ignored: the face, its stored rectangle, its person's name.
fn faces_of(app: &App, photo: PhotoId) -> Vec<(FaceRef, Rect01, Option<String>)> {
    let Some(s) = app.session() else { return Vec::new() };
    let lib = s.library();
    lib.faces(photo)
        .iter()
        .enumerate()
        .filter(|(_, f)| f.counts())
        .filter_map(|(i, f)| {
            let face = FaceRef { photo, index: u16::try_from(i).ok()? };
            let name = match &f.person {
                FacePerson::Person(id) => lib.person(id).map(|p| p.name.clone()),
                _ => None,
            };
            Some((face, f.rect, name))
        })
        .collect()
}

/// The photo area's part of the People pane: the faces' boxes and Add a Person Manually's box.
pub fn on_canvas(app: &mut App, ui: &mut Ui) {
    let Some(photo) = edit_view::open_photo(app) else { return };
    let st = app.state();
    if st.edit.tool.is_some() || st.edit.compare != Compare::One {
        return;
    }
    let pane_open = st.pane == Some(Pane::People);
    let manual = st.people_pane.manual.clone();
    let hovered = st.people_pane.hovered;
    if !pane_open && manual.is_none() {
        return;
    }
    let Some(view) = canvas::view(ui.ctx()).filter(|v| v.photo == photo) else { return };
    let Some(shown) = shown(app, photo) else { return };
    let rect = on_screen(&shown, view.image);
    if !(rect.width() > 0.0 && rect.height() > 0.0 && rect.is_finite()) {
        return;
    }
    let t = *app.tokens();
    let mut child = ui.new_child(UiBuilder::new().id_salt("edit.faces").max_rect(view.area));
    child.set_clip_rect(view.area.intersect(ui.clip_rect()));

    let mut over = None;
    for (face, stored, name) in faces_of(app, photo) {
        let Some(r) = shown.shown_rect(&stored) else { continue };
        // A face the crop leaves out has no box.
        if r[2] <= 0.0 || r[0] >= 1.0 || r[3] <= 0.0 || r[1] >= 1.0 {
            continue;
        }
        let b = FaceBox {
            rect: Rect::from_min_max(pos2(r[0] as f32, r[1] as f32), pos2(r[2] as f32, r[3] as f32)),
            label: name.as_deref(),
            highlighted: hovered == Some(face),
            edit: false,
        };
        if widgets::face_box(&mut child, &t, &box_id(face), rect, &b).hovered {
            over = Some(face);
        }
    }
    pane::track_hover(app, ui.ctx(), pane::HoverFrom::Canvas, over);

    if let Some(m) = manual {
        let b =
            FaceBox { rect: Rect::from_min_max(pos2(m.rect[0], m.rect[1]), pos2(m.rect[2], m.rect[3])), label: None, highlighted: true, edit: true };
        let out = widgets::face_box(&mut child, &t, MANUAL_ID, rect, &b);
        if let Some(r) = out.rect {
            let r = face_box_rect(r);
            let v = [r.min.x, r.min.y, r.max.x, r.max.y];
            if let Some(m) = app.state_mut().people_pane.manual.as_mut()
                && is_unit_rect(v)
            {
                m.rect = v;
            }
        }
        plate(app, &mut child, out.screen, view.area);
    }
}

/// Where the plate goes: centred under the box, above it when there is no room below, kept across
/// the photo area.
pub fn plate_rect(boxed: Rect, area: Rect, height: f32) -> Rect {
    let w = PLATE_W.min(area.width().max(1.0));
    let below = boxed.max.y + PLATE_GAP;
    let above = boxed.min.y - PLATE_GAP - height;
    let y = if below + height <= area.max.y {
        below
    } else if above >= area.min.y {
        above
    } else {
        (area.max.y - height).max(area.min.y)
    };
    let x = (boxed.center().x - w / 2.0).clamp(area.min.x, (area.max.x - w).max(area.min.x));
    Rect::from_min_size(pos2(x, y).round(), vec2(w, height))
}

/// The plate under Add a Person Manually's box: the instruction, the name field, Add and Cancel.
fn plate(app: &mut App, ui: &mut Ui, boxed: Rect, area: Rect) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let height = 2.0 * PLATE_PAD + 18.0 + 6.0 + t.metrics.button_h;
    let r = plate_rect(boxed, area, height);
    ui.painter().rect_filled(r, CornerRadius::same(t.radius), t.caption_plate);
    let mut col = ui.new_child(UiBuilder::new().id_salt("faces.manual.plate").max_rect(r.shrink(PLATE_PAD)).layout(Layout::top_down(Align::Min)));
    col.label(RichText::new(INSTRUCTION).font(fonts::regular(t.count)).color(t.overlay_text));
    col.add_space(6.0);
    let mut text = app.state().people_pane.manual.as_ref().map(|m| m.name.clone()).unwrap_or_default();
    let options = pane::people_names(app, &ctx);
    let (out, add, cancel) = col
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // Room for Add and Cancel at their own widths.
            let width = (ui.available_width() - 110.0).max(40.0);
            let out =
                widgets::autocomplete(ui, &t, NAME_ID, &mut text, &options, |ui, text| widgets::text_field(ui, &t, NAME_ID, text, NAME_HINT, width));
            let add = ui.add_enabled_ui(!text.trim().is_empty(), |ui| widgets::primary_button(ui, &t, ADD_ID, "Add")).inner;
            let cancel = widgets::text_button(ui, &t, CANCEL_ID, "Cancel", false);
            (out, add, cancel)
        })
        .inner;
    if let Some(m) = app.state_mut().people_pane.manual.as_mut()
        && m.name != text
    {
        m.name = text.chars().take(crate::state::MAX_DRAFT).collect();
    }
    if cancel.clicked() {
        app.state_mut().people_pane.manual = None;
        return;
    }
    let enter = out.field.ending == Some(Ending::Return) || out.picked.is_some();
    // A name picked from the list is in the field already.
    if (enter || add.clicked())
        && !text.trim().is_empty()
        && let Err(e) = people_pane::store_manual(app, &text)
    {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plate_goes_under_the_box_or_above_it_and_stays_on_the_area() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let under = plate_rect(Rect::from_min_max(pos2(300.0, 100.0), pos2(400.0, 200.0)), area, 60.0);
        assert_eq!((under.min.y, under.center().x), (206.0, 350.0));
        let over = plate_rect(Rect::from_min_max(pos2(300.0, 500.0), pos2(400.0, 590.0)), area, 60.0);
        assert_eq!(over.max.y, 494.0, "no room below: above");
        let left = plate_rect(Rect::from_min_max(pos2(0.0, 100.0), pos2(20.0, 120.0)), area, 60.0);
        assert_eq!(left.min.x, 0.0, "kept on the area");
        let tall = plate_rect(Rect::from_min_max(pos2(0.0, 0.0), pos2(20.0, 600.0)), area, 60.0);
        assert_eq!(tall.max.y, 600.0, "neither: at the bottom");
    }
}
