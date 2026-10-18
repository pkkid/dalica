//! Face boxes on the photo in the edit view (`organizing.md` §6.4), run after the canvas
//! (`panels/edit.rs`) while no tool is open and one photo is shown:
//!
//! - **The faces' boxes** show while the People pane is open (or Add a Person Manually's box is
//!   drawn): each face of the photo that is not ignored as ui-kit's `face_box` (`facebox:<PhotoId>.<n>`,
//!   labelled with its person's name), its rectangle taken from the stored frame
//!   (`data-model.md` §3.3) through the photo's [`Geometry`] (its rotations, flips, straighten,
//!   crop and frames) onto the photo as shown, so a box stays on its face however the photo is
//!   turned or cropped. A box is highlighted while its face is hovered in the People pane
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
use dalica_edit::{Geometry, Point, Recipe, Rect as Area};
use dalica_image::Orientation;
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

fn unit(v: f64) -> f64 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.5 }
}

/// The photo as the edit view shows it: its geometry, and where the photo lies on the displayed
/// picture (normalised, ordered, not empty).
pub struct Shown {
    g: Geometry,
    area: Area,
}

impl Shown {
    /// The geometry of `photo`'s recipe as the edit view shows it; `None` without a library, for a
    /// photo it lacks or one without pixels.
    pub fn of(app: &App, photo: PhotoId) -> Option<Shown> {
        let s = app.session()?;
        let p = s.library().photo(photo)?;
        if p.width == 0 || p.height == 0 {
            return None;
        }
        let recipe = Recipe::parse(tools::recipe(app, photo)).unwrap_or_default();
        let g = Geometry::new(&recipe, Orientation::default(), p.width, p.height);
        let area = g.photo_area()?.to_display(g.orientation());
        let ok = [area.x0, area.y0, area.x1, area.y1].iter().all(|v| v.is_finite()) && area.x1 - area.x0 > 1e-9 && area.y1 - area.y0 > 1e-9;
        ok.then_some(Shown { g, area })
    }

    /// Where the stored point `p` shows, normalised to the photo as shown; outside 0..1 when a crop
    /// leaves it out.
    fn shown(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        let d = self.g.to_display(Point::new(p[0], p[1]))?;
        let a = &self.area;
        let s = [(d.x - a.x0) / (a.x1 - a.x0), (d.y - a.y0) / (a.y1 - a.y0)];
        s.iter().all(|v| v.is_finite()).then_some(s)
    }

    /// The stored point under `s` (normalised to the photo as shown, brought onto it).
    fn stored(&self, s: [f64; 2]) -> Option<[f64; 2]> {
        let a = &self.area;
        let d = Point::new(a.x0 + unit(s[0]) * (a.x1 - a.x0), a.y0 + unit(s[1]) * (a.y1 - a.y0));
        let p = self.g.to_stored(d)?;
        Some([unit(p.x), unit(p.y)])
    }

    /// The box round the corners of `r` (`[x0, y0, x1, y1]`) mapped by `map`.
    fn bounds(r: [f64; 4], map: impl Fn([f64; 2]) -> Option<[f64; 2]>) -> Option<[f64; 4]> {
        let corners = [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]];
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for c in corners {
            let p = map(c)?;
            b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
        }
        b.iter().all(|v| v.is_finite()).then_some(b)
    }

    /// The stored rectangle `r` as shown, normalised to the photo as shown (a turned photo's box
    /// turned with it; past 0..1 where a crop cuts it).
    pub fn shown_rect(&self, r: &Rect01) -> Option<[f64; 4]> {
        Shown::bounds([r.x0(), r.y0(), r.x1(), r.y1()], |p| self.shown(p))
    }

    /// The stored rectangle under `s` (normalised to the photo as shown), `None` when it is empty.
    pub fn stored_rect(&self, s: [f64; 4]) -> Option<Rect01> {
        let b = Shown::bounds(s, |p| self.stored(p))?;
        Rect01::new(b[0], b[1], b[2], b[3]).ok()
    }

    /// The photo as shown on the screen, the picture drawn at `image`.
    pub fn on_screen(&self, image: Rect) -> Rect {
        let at = |x: f64, y: f64| pos2(image.min.x + x as f32 * image.width(), image.min.y + y as f32 * image.height());
        Rect::from_min_max(at(self.area.x0, self.area.y0), at(self.area.x1, self.area.y1))
    }
}

/// `photo`'s stored rectangle under the box `shown` (`[x0, y0, x1, y1]` normalised to the photo as
/// the edit view shows it).
pub fn stored_rect(app: &App, photo: PhotoId, shown: [f32; 4]) -> Option<Rect01> {
    Shown::of(app, photo)?.stored_rect(shown.map(f64::from))
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
    let Some(shown) = Shown::of(app, photo) else { return };
    let rect = shown.on_screen(view.image);
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
