//! The Properties pane (`ui-layout.md` §7): what is known about the selection, in the side pane.
//! Its heading names what it describes ("Properties of IMG_0361.jpg", "Properties of 12 photos", the
//! folder's name for a section selected as a unit); [Show All Metadata]
//! (`button:properties.showAll`) switches between the main rows and every field
//! ([`crate::state::UiState::properties_show_all`], `ui.set {propertiesShowAll}`); with nothing
//! selected the pane says "No selection" and has no switch.
//!
//! For now the pane shows its heading and the Show All switch; the label/value rows (from
//! `photo.metadata`) arrive later in this milestone.

use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets;
use egui::{Align, Layout, RichText, Ui, UiBuilder};

use crate::app::App;
use crate::state::{Section, UiState};

/// The Show All switch's widget id.
pub const SHOW_ALL_ID: &str = "button:properties.showAll";

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

/// Draws the pane's body into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect().shrink(8.0);
    let mut col = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
    let text = heading(app, app.state());
    col.add(egui::Label::new(RichText::new(text).font(fonts::semibold(t.body)).color(t.text)).truncate());
    // Nothing selected: nothing to list, so no switch either.
    let sel = &app.state().selection;
    if sel.ids.is_empty() && !sel.whole {
        return;
    }
    col.add_space(8.0);
    let all = app.state().properties_show_all;
    let label = if all { "Show Fewer" } else { "Show All Metadata" };
    if widgets::text_button(&mut col, &t, SHOW_ALL_ID, label, all).clicked() {
        app.state_mut().properties_show_all = !all;
    }
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
}
