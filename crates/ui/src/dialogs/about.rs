//! About Dalica (`contributors.md` → In Dalica, `menus-and-shortcuts.md` §1.8): opened by
//! `app.about` (Help ▸ About Dalica; on macOS, Dalica ▸ About Dalica). Its tabs **About ·
//! Contributors · Models** are `tab:about.about`, `tab:about.contributors` and `tab:about.models`;
//! the tab and how Contributors lists people are view state ([`crate::state::About`], `ui.set
//! {about}`), so the dialog reopens as it was left. It has no draft and one button, Close (Esc).
//!
//! - **About**: the app icon, the version, the commit and build date the build recorded
//!   (`dalica_core::build`), the licence and where the source lives.
//! - **Contributors** and **Models**: the credits compiled into the binary ([`crate::credits`]).
//!
//! Every tab is drawn in the same height, so switching tabs never resizes the dialog.

use dalica_engine::CmdError;
use dalica_ui_kit::{Tokens, fonts, widgets};
use egui::{RichText, Ui, vec2};
use serde_json::Value;

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::app_icon;
use crate::credits::{self, Baked};
use crate::state::AboutTab;
use crate::tl;

pub const TITLE: &str = "About Dalica";
/// The dialog's width (points): the Contributors table's ten columns fit beside the names.
pub const WIDTH: f32 = 760.0;
/// The height of a tab's content (points), less on a window too short for it.
pub const CONTENT_H: f32 = 340.0;
/// What the dialog's frame, title, tab row and button row take besides the content, and the room
/// kept above and below it (points).
const CHROME_H: f32 = 140.0;
const SCREEN_MARGIN: f32 = 48.0;
/// The app icon's edge on the About tab (points).
const ICON: f32 = 96.0;

/// The tabs: their state value, widget id and label.
pub const TABS: [(AboutTab, &str, &str); 3] = [
    (AboutTab::About, "tab:about.about", "About"),
    (AboutTab::Contributors, "tab:about.contributors", "Contributors"),
    (AboutTab::Models, "tab:about.models", "Models"),
];

/// The licence the About tab names.
pub const LICENCE: &str = env!("CARGO_PKG_LICENSE");
/// Where the source lives.
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::About)
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: tl!("About Dalica"), width: WIDTH, buttons: vec![Button::close()] }
}

/// The height of a tab's content on a screen `screen_h` points tall.
pub fn content_height(screen_h: f32) -> f32 {
    CONTENT_H.min(screen_h - CHROME_H - SCREEN_MARGIN).max(160.0)
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    let baked = credits::baked(ui.ctx());
    let mut view = app.state().about;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (tab, id, label) in TABS {
            if widgets::text_button(ui, &t, id, &tab_label(label), view.tab == tab).clicked() {
                view.tab = tab;
            }
        }
    });
    ui.add_space(12.0);
    let height = content_height(ui.ctx().content_rect().height());
    let width = ui.available_width();
    ui.allocate_ui(vec2(width, height), |ui| {
        ui.set_min_size(vec2(width, height));
        ui.set_max_height(height);
        match view.tab {
            AboutTab::About => about_tab(ui, &t, &baked),
            AboutTab::Contributors => credits::contributors_ui(ui, &t, &baked, &mut view, height),
            AboutTab::Models => credits::models_ui(ui, &t, &baked, height),
        }
    });
    if view != app.state().about {
        app.state_mut().about = view;
    }
}

/// A label of [`TABS`] in the language in effect.
fn tab_label(english: &str) -> String {
    match english {
        "About" => tl!(ctx = "about tab", "About"),
        "Contributors" => tl!("Contributors"),
        "Models" => tl!("Models"),
        other => crate::i18n::tr(other),
    }
}

/// The About tab: the icon beside the name, the build's provenance, the licence and the source.
fn about_tab(ui: &mut Ui, t: &Tokens, baked: &Baked) {
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        ui.add_space(8.0);
        // The app icon (`assets/app-icon/`): the 128 px render at 1×, the 256 px one above 1.33×.
        if let Some(icon) = app_icon::texture(ui.ctx(), ICON) {
            ui.add(egui::Image::new(&icon).fit_to_exact_size(vec2(ICON, ICON)));
        } else {
            ui.allocate_space(vec2(ICON, ICON));
        }
        ui.add_space(20.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(tl!("Dalica")).font(fonts::semibold(t.empty_title)).color(t.text));
            ui.add_space(2.0);
            ui.label(RichText::new(tl!("A photo organizer with simple editing")).font(fonts::regular(t.body)).color(t.text_label));
            ui.add_space(14.0);
            egui::Grid::new("dialog:about/facts").num_columns(2).spacing(vec2(18.0, 6.0)).show(ui, |ui| {
                for (label, value) in facts(baked) {
                    ui.label(RichText::new(label).font(fonts::regular(t.body)).color(t.text_dim));
                    ui.label(RichText::new(value).font(fonts::regular(t.body)).color(t.text));
                    ui.end_row();
                }
            });
            ui.add_space(14.0);
            let source = tl!("Source code: {address}", address = REPOSITORY.trim_start_matches("https://"));
            let copyright = tl!("Copyright © 2026 the Dalica contributors; see the Contributors and Models tabs.");
            for line in [source, copyright] {
                ui.label(RichText::new(line).font(fonts::regular(t.small)).color(t.text_dim));
            }
        });
    });
}

/// The About tab's lines, in the language in effect: version, commit (shortened), build date and
/// licence. A build that did not record its commit or date says so.
pub fn facts(baked: &Baked) -> [(String, String); 4] {
    let dev = tl!("Not recorded (a development build)");
    let commit = baked.commit.map_or_else(|| dev.clone(), |c| c.chars().take(dalica_core::build::SHORT_SHA).collect());
    let date = baked.build_date.map_or_else(|| dev.clone(), |d| d.chars().take(32).collect());
    [(tl!("Version"), baked.version.to_string()), (tl!("Commit"), commit), (tl!("Build date"), date), (tl!("Licence"), LICENCE.to_string())]
}

/// The dialog has no button but Close.
pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_content_fits_the_smallest_window() {
        assert_eq!(content_height(900.0), CONTENT_H);
        assert!(content_height(600.0) + CHROME_H <= 600.0 - SCREEN_MARGIN);
        assert_eq!(content_height(0.0), 160.0);
    }

    #[test]
    fn facts_shorten_the_commit_and_say_when_nothing_was_recorded() {
        let mut b = Baked::compiled();
        b.commit = Some("0123456789abcdef0123");
        b.build_date = None;
        let f = facts(&b);
        assert_eq!(f[1], ("Commit".to_string(), "012345678".to_string()));
        assert!(f[2].1.starts_with("Not recorded"));
        assert_eq!(f[3].1, "MIT OR Apache-2.0");
    }
}
