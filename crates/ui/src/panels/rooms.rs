//! The room tab strip (`ui-layout.md` §8): 28 high under the menu bar while a room besides the
//! Library is open, drawn with `ui-kit`'s room tabs (`design-system.md` §6: 190 × 28, glyph and
//! label, the tab in front joined to the content below). Each room open has a tab
//! (`tab:room/<room>`, active for the room in front; a click runs `room.select`), every tab but the
//! Library's a close × (`tab:room/<room>/close`, `room.close`, which asks "Stop importing?" while
//! the Import room copies, and the Collage room asks Save Draft, Discard or Cancel with unsaved
//! changes). The tabs shrink, then overflow into a menu, in a narrow window.

use dalica_ui_kit::icons::{Glyph, Icon, ViewingIcon};
use dalica_ui_kit::widgets::{self, RoomTab};
use egui::{Layout, Ui, UiBuilder, vec2};
use serde_json::json;

use crate::app::App;
use crate::state::Room;

/// The strip's automation id: its tabs are `tab:room/<room>`.
pub const STRIP: &str = "tab:room";
/// The space before the first tab (points).
pub const INSET: f32 = 8.0;

/// A room's tab label.
pub fn label(room: Room) -> &'static str {
    match room {
        Room::Library => "Library",
        Room::Import => "Import",
        Room::Collage => "Collage",
    }
}

/// A room's tab glyph: the Library's four thumbnails, Import's camera, Collage's three pictures.
pub fn glyph(room: Room) -> Glyph {
    match room {
        Room::Library => ViewingIcon::Library.into(),
        Room::Import => Icon::Import.into(),
        Room::Collage => Icon::Collage.into(),
    }
}

/// Draws the strip into `ui`'s rect (the `tabStrip` region).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0, t.canvas);
    let rect = full.with_min_x(full.min.x + INSET);
    let mut row = ui.new_child(UiBuilder::new().id_salt("roomTabs").max_rect(rect).layout(Layout::left_to_right(egui::Align::Min)));
    row.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let (rooms, front) = (app.state().rooms.clone(), app.state().room);
    let items: Vec<RoomTab<'_>> =
        rooms.iter().map(|&r| RoomTab { name: r.name(), icon: glyph(r), label: label(r), closable: r != Room::Library }).collect();
    let active = rooms.iter().position(|&r| r == front).unwrap_or(0);
    let out = widgets::room_tabs(&mut row, &t, STRIP, &items, active);
    let chosen = if let Some(room) = out.closed.and_then(|i| rooms.get(i)) {
        Some(("room.close", *room))
    } else {
        out.clicked.and_then(|i| rooms.get(i)).filter(|&&r| r != front).map(|&r| ("room.select", r))
    };
    if let Some((id, room)) = chosen
        && let Err(e) = app.run(id, &json!({ "room": room.name() }))
    {
        app.toast(&e.to_string(), widgets::ToastKind::Caution);
    }
}
