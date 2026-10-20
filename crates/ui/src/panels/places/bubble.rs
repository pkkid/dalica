//! The map's bubble (`organizing.md` §7): what it says, its thumbnail stack and its buttons, for
//! the bubble open in the view state (`places.bubble`):
//!
//! - a drop's: "Put 3 photos here?" ("Move 3 photos here?" when some have a location), the
//!   dropped photos' stack, [OK] `button:places.put` [Cancel] `button:places.cancel`, pointing at
//!   the drop's point (drawn as the pin while it waits);
//! - the pin's: the same for the selection ("Select the photos to put here." and [Cancel] alone
//!   while nothing is selected);
//! - a marker's: "3 photos here", their stack, [Show These Photos] `button:places.showPhotos`
//!   [Remove Location] `button:places.removeLocation`; a marker the map draws as a group of places
//!   speaks for all of them (the count its badge shows), and its buttons act on all of them.
//!
//! The stack's pictures are the photos' thumbnails the session holds in memory (asked for at the
//! visible priority when missing, never read from the disk here), at most three textures kept for
//! the pane and replaced as the photos change.

use dalica_core::PhotoId;
use dalica_engine::thumbs::{Size, Thumb};
use dalica_ui_kit::widgets::{BUBBLE_STACK, BUBBLE_STACK_MAX, BubbleAnchor, BubbleButton, BubbleThumb, MarkerGroup};
use egui::{Context, Id, TextureHandle, vec2};

use super::markers::{Place, grouped};
use crate::app::App;
use crate::commands::places as cmd;
use crate::state::Bubble;
use crate::{tl, tln};

pub const OK_ID: &str = "button:places.put";
pub const CANCEL_ID: &str = "button:places.cancel";
pub const SHOW_ID: &str = "button:places.showPhotos";
pub const REMOVE_ID: &str = "button:places.removeLocation";
/// The pin's bubble while nothing is selected.
pub const SELECT_TEXT: &str = "Select the photos to put here.";

const OK: BubbleButton<'static> = BubbleButton { id: OK_ID, label: "OK", primary: true };
const CANCEL: BubbleButton<'static> = BubbleButton { id: CANCEL_ID, label: "Cancel", primary: false };
const SHOW: BubbleButton<'static> = BubbleButton { id: SHOW_ID, label: "Show These Photos", primary: true };
const REMOVE: BubbleButton<'static> = BubbleButton { id: REMOVE_ID, label: "Remove Location", primary: false };

/// The bubble to draw: where it points, its photos, its text and its buttons.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub anchor: BubbleAnchor,
    pub ids: Vec<PhotoId>,
    pub text: String,
    pub buttons: Vec<BubbleButton<'static>>,
}

/// A button's label in the language in effect (the buttons keep the English).
pub fn label(id: &str, english: &str) -> String {
    match id {
        OK_ID => tl!("OK"),
        CANCEL_ID => tl!("Cancel"),
        SHOW_ID => tl!("Show These Photos"),
        REMOVE_ID => tl!("Remove Location"),
        _ => crate::i18n::tr(english),
    }
}

/// "Put 3 photos here?", or "Move 3 photos here?" when `moving`.
pub fn put_text(n: usize, moving: bool) -> String {
    if moving { tln!(n, "Move {n} photo here?", "Move {n} photos here?") } else { tln!(n, "Put {n} photo here?", "Put {n} photos here?") }
}

/// "3 photos here".
pub fn here_text(n: usize) -> String {
    tln!(n, "{n} photo here", "{n} photos here")
}

/// The bubble the view state has open, among `places` drawn as `groups`; `None` for none, or for
/// a marker that is not on the map.
pub fn shown(app: &App, places: &[Place], groups: &[MarkerGroup]) -> Option<Shown> {
    let state = &app.state().places;
    match state.bubble? {
        Bubble::Drop => {
            let ids = state.drop.as_ref()?.ids.clone();
            let text = put_text(ids.len(), cmd::some_located(app, &ids));
            Some(Shown { anchor: BubbleAnchor::Pin, ids, text, buttons: vec![OK, CANCEL] })
        }
        Bubble::Pin => {
            state.pin.as_ref()?;
            let ids = app.state().selection.ids.clone();
            if ids.is_empty() {
                return Some(Shown { anchor: BubbleAnchor::Pin, ids, text: tl!("Select the photos to put here."), buttons: vec![CANCEL] });
            }
            let text = put_text(ids.len(), cmd::some_located(app, &ids));
            Some(Shown { anchor: BubbleAnchor::Pin, ids, text, buttons: vec![OK, CANCEL] })
        }
        Bubble::Marker(n) => {
            let i = usize::try_from(n).ok()?;
            let place = grouped(places, groups, i)?;
            Some(Shown { anchor: BubbleAnchor::Marker(i), ids: place.ids.clone(), text: here_text(place.ids.len()), buttons: vec![SHOW, REMOVE] })
        }
    }
}

/// A picture of the stack: the photo, the ladder size, and its texture.
type Slot = (PhotoId, Size, TextureHandle);

/// The stack of `ids`: the first [`BUBBLE_STACK_MAX`] photos whose thumbnails the session has in
/// memory now, each shaped as the photo shows.
pub fn thumbs(app: &mut App, ctx: &Context, ids: &[PhotoId]) -> Vec<BubbleThumb> {
    let key = Id::new("places.bubble.thumbs");
    let kept: Vec<Slot> = ctx.data(|d| d.get_temp(key)).unwrap_or_default();
    let need = (BUBBLE_STACK * ctx.pixels_per_point()).ceil().max(1.0) as u32;
    let Some(session) = app.session_mut() else { return Vec::new() };
    let (mut slots, mut out) = (Vec::new(), Vec::new());
    for &id in ids.iter().take(BUBBLE_STACK_MAX) {
        let Thumb::Image { size, image, .. } = session.thumbnail(id, need) else { continue };
        let Some(photo) = session.library().photo(id) else { continue };
        let (w, h) = dalica_edit::shown_size(&session.library().info(id).edits, photo.width, photo.height, dalica_edit::Cropping::Ignored);
        let handle = match kept.iter().find(|(p, s, _)| *p == id && *s == size) {
            Some((_, _, h)) => h.clone(),
            None => {
                let dims = [image.width as usize, image.height as usize];
                if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
                    continue;
                }
                let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
                ctx.load_texture(format!("places.bubble:{id}"), pixels, egui::TextureOptions::LINEAR)
            }
        };
        out.push(BubbleThumb { texture: handle.id(), size: vec2(w as f32, h as f32) });
        slots.push((id, size, handle));
    }
    ctx.data_mut(|d| d.insert_temp(key, slots));
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::panels::places::markers;

    #[test]
    fn the_bubble_says_put_move_or_how_many_are_here() {
        assert_eq!((put_text(1, false).as_str(), put_text(3, true).as_str()), ("Put 1 photo here?", "Move 3 photos here?"));
        assert_eq!(here_text(9), "9 photos here");
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        assert_eq!(shown(&app, &[], &[]), None);
        app.run("places.drop", &json!({ "ids": [ids[0], ids[1]], "at": [1, 2] })).unwrap();
        let b = shown(&app, &[], &[]).unwrap();
        assert_eq!((b.text.as_str(), b.anchor, b.buttons.len()), ("Put 2 photos here?", BubbleAnchor::Pin, 2));
        app.run("photo.geo.set", &json!({ "ids": [ids[1]], "geo": [3, 4] })).unwrap();
        assert_eq!(shown(&app, &[], &[]).unwrap().text, "Move 2 photos here?");
        app.run("places.pin", &json!({ "at": [5, 6] })).unwrap();
        assert_eq!(shown(&app, &[], &[]).unwrap().text, SELECT_TEXT);
        app.run("select.set", &json!({ "ids": ids })).unwrap();
        let places = markers::places(&app);
        app.run("places.bubble", &json!({ "marker": 0 })).unwrap();
        let b = shown(&app, &places, &[]).unwrap();
        assert_eq!((b.text.as_str(), b.anchor, b.buttons[0].id), ("1 photo here", BubbleAnchor::Marker(0), SHOW_ID));
        assert_eq!(shown(&app, &[], &[]), None, "a marker no longer on the map");
    }
}
