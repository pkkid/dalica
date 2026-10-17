//! The room tab strip (`ui-layout.md` §8): under the menu bar while a room besides the Library is
//! open, a tab per room open (`tab:room.<room>`, the one in front active; a click runs
//! `room.select`) and a close button on every tab but the Library's (`button:room.close.<room>`,
//! `room.close`).
//!
//! M11.2's stub: plain text buttons in the strip's region. M11.13 draws the room tabs of
//! `design-system.md` (190 × 28, icon and label, the active tab joined to the content).

use dalica_ui_kit::icons::Icon;
use dalica_ui_kit::widgets;
use egui::{Align, Layout, Ui, UiBuilder, vec2};
use serde_json::json;

use crate::app::App;
use crate::state::Room;

/// A room's tab label.
pub fn label(room: Room) -> &'static str {
    match room {
        Room::Library => "Library",
        Room::Import => "Import",
    }
}

/// Draws the strip into `ui`'s rect (the `tabStrip` region).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect().shrink2(vec2(8.0, 0.0));
    let mut row = ui.new_child(UiBuilder::new().id_salt("roomTabs").max_rect(rect).layout(Layout::left_to_right(Align::Center)));
    let (rooms, front) = (app.state().rooms.clone(), app.state().room);
    let mut chosen = None;
    for room in rooms {
        if widgets::text_button(&mut row, &t, &format!("tab:room.{}", room.name()), label(room), room == front).clicked() {
            chosen = Some(("room.select", room));
        }
        if room != Room::Library {
            let id = format!("button:room.close.{}", room.name());
            if widgets::icon_button(&mut row, &t, &id, Icon::ClearSearch, 20.0, false, true, "Close").clicked() {
                chosen = Some(("room.close", room));
            }
        }
        row.add_space(4.0);
    }
    if let Some((id, room)) = chosen {
        let _ = app.run(id, &json!({ "room": room.name() }));
    }
}
