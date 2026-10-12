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
use egui::{Id, RichText, TextureHandle, Ui, vec2};
use serde_json::Value;

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::credits::{self, Baked};
use crate::state::AboutTab;

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

/// The 128 px app icon (`assets/app-icon/`, `docs/assets.md` §7), shown at [`ICON`] points.
const ICON_PNG: &[u8] = include_bytes!("../../../../assets/app-icon/hicolor/128x128/apps/org.dalica.Dalica.png");

pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::About)
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::close()] }
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
            if widgets::text_button(ui, &t, id, label, view.tab == tab).clicked() {
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

/// The About tab: the icon beside the name, the build's provenance, the licence and the source.
fn about_tab(ui: &mut Ui, t: &Tokens, baked: &Baked) {
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        ui.add_space(8.0);
        if let Some(icon) = icon(ui.ctx()) {
            ui.add(egui::Image::new(&icon).fit_to_exact_size(vec2(ICON, ICON)));
        } else {
            ui.allocate_space(vec2(ICON, ICON));
        }
        ui.add_space(20.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("Dalica").font(fonts::semibold(t.empty_title)).color(t.text));
            ui.add_space(2.0);
            ui.label(RichText::new("A photo organizer with simple editing").font(fonts::regular(t.body)).color(t.text_label));
            ui.add_space(14.0);
            egui::Grid::new("dialog:about/facts").num_columns(2).spacing(vec2(18.0, 6.0)).show(ui, |ui| {
                for (label, value) in facts(baked) {
                    ui.label(RichText::new(label).font(fonts::regular(t.body)).color(t.text_dim));
                    ui.label(RichText::new(value).font(fonts::regular(t.body)).color(t.text));
                    ui.end_row();
                }
            });
            ui.add_space(14.0);
            let source = format!("Source code: {}", REPOSITORY.trim_start_matches("https://"));
            for line in [source.as_str(), "Copyright © 2026 the Dalica contributors; see the Contributors and Models tabs."] {
                ui.label(RichText::new(line).font(fonts::regular(t.small)).color(t.text_dim));
            }
        });
    });
}

/// The About tab's lines: version, commit (shortened), build date and licence. A build that did
/// not record its commit or date says so.
pub fn facts(baked: &Baked) -> [(&'static str, String); 4] {
    let dev = "Not recorded (a development build)";
    let commit = baked.commit.map_or_else(|| dev.to_string(), |c| c.chars().take(dalica_core::build::SHORT_SHA).collect());
    let date = baked.build_date.map_or_else(|| dev.to_string(), |d| d.chars().take(32).collect());
    [("Version", baked.version.to_string()), ("Commit", commit), ("Build date", date), ("Licence", LICENCE.to_string())]
}

/// The app icon's texture, decoded once per egui context; `None` (logged once) if the embedded PNG
/// does not decode.
fn icon(ctx: &egui::Context) -> Option<TextureHandle> {
    let id = Id::new("dialog:about/icon");
    if let Some(slot) = ctx.data(|d| d.get_temp::<Option<TextureHandle>>(id)) {
        return slot;
    }
    let handle = match decode_png(ICON_PNG) {
        Ok(image) => Some(ctx.load_texture("dalica-about-icon", image, egui::TextureOptions::LINEAR)),
        Err(e) => {
            log::warn!("dalica: the About icon does not decode: {e}");
            None
        }
    };
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

/// An 8-bit PNG as an egui image (palettes and 16-bit samples are expanded and stripped; grey and
/// RGB get an opaque alpha). At most 64 MiB of pixels.
pub fn decode_png(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("the PNG is too large")?;
    if size > 64 << 20 {
        return Err("the PNG is too large".into());
    }
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let data = buf.get(..info.buffer_size()).ok_or("the PNG is truncated")?;
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect(),
        png::ColorType::GrayscaleAlpha => data.as_chunks::<2>().0.iter().flat_map(|&[g, a]| [g, g, g, a]).collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("the PNG's palette was not expanded".into()),
    };
    if rgba.len() != w.saturating_mul(h).saturating_mul(4) {
        return Err("the PNG's size does not match its pixels".into());
    }
    Ok(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

/// The dialog has no button but Close.
pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_decodes_and_a_broken_png_is_an_error() {
        let image = decode_png(ICON_PNG).unwrap();
        assert_eq!(image.size, [128, 128]);
        assert!(decode_png(b"not a png").is_err());
        assert!(decode_png(&ICON_PNG[..ICON_PNG.len() / 2]).is_err());
    }

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
        assert_eq!(f[1], ("Commit", "012345678".to_string()));
        assert!(f[2].1.starts_with("Not recorded"));
        assert_eq!(f[3].1, "MIT OR Apache-2.0");
    }
}
