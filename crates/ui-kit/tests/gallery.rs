//! The widget gallery (M1.16): every `ui-kit` widget in its states, rendered by the CPU rasterizer.
//! The same frame must give identical pixels twice and match the stored fingerprint
//! (`fingerprints/gallery.json`, `architecture.md` §11). The render is always written to
//! `target/test-output/ui-kit/gallery.png` to look at; `DALICA_BLESS=1` stores a new fingerprint
//! (commit it only after looking at the PNG).

use std::path::PathBuf;
use std::time::Instant;

use dalica_scenes::fingerprint;
use dalica_ui_kit::widgets::{icon_button, star_badge, star_toggle, text_button};
use dalica_ui_kit::{Icon, Image, Role, Tokens, fonts, registry};
use egui::{Align2, Rect, pos2, vec2};

const SIZE: (u32, u32) = (480, 200);

fn gallery(ui: &mut egui::Ui, roles: &mut Vec<(String, Role)>) {
    let t = Tokens::default();
    let p = ui.painter().clone();
    p.text(pos2(16.0, 12.0), Align2::LEFT_TOP, "Widget gallery", fonts::semibold(t.title), t.text);
    p.text(pos2(16.0, 34.0), Align2::LEFT_TOP, "Body text in Inter 13 · text_label", fonts::regular(t.body), t.text_label);
    p.text(pos2(16.0, 52.0), Align2::LEFT_TOP, "Small dim caption, 11 pt", fonts::regular(t.small), t.text_dim);
    let row = |ui: &mut egui::Ui, y: f32, add: &mut dyn FnMut(&mut egui::Ui)| {
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(pos2(16.0, y), vec2(300.0, 32.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| add(ui),
        );
    };
    row(ui, 72.0, &mut |ui| {
        text_button(ui, &t, "gallery.button", "Add Folder…", false);
        text_button(ui, &t, "gallery.button.active", "Hold", true);
        ui.add_enabled_ui(false, |ui| text_button(ui, &t, "gallery.button.disabled", "Clear", false));
    });
    row(ui, 110.0, &mut |ui| {
        icon_button(ui, &t, "gallery.icon", Icon::Plus, 28.0, false, true, "Add");
        icon_button(ui, &t, "gallery.icon.active", Icon::Star, 28.0, true, true, "Starred only");
        icon_button(ui, &t, "gallery.icon.disabled", Icon::StarOutline, 28.0, false, false, "Nothing to star");
        star_toggle(ui, &t, "gallery.star.off", false);
        star_toggle(ui, &t, "gallery.star.on", true);
    });
    // A starred thumbnail: the light-table background, the cell, the letterboxed photo and its badge.
    let grid = Rect::from_min_size(pos2(330.0, 12.0), vec2(136.0, 176.0));
    p.rect_filled(grid, 0, t.grid_bg);
    let cell = Rect::from_min_size(pos2(338.0, 40.0), vec2(120.0, 120.0));
    p.rect_filled(cell, 0, t.cell_selected);
    let photo = Rect::from_min_size(pos2(338.0, 60.0), vec2(120.0, 80.0));
    p.rect_filled(photo, 0, egui::Color32::from_rgb(70, 110, 150));
    p.rect_filled(Rect::from_min_size(photo.min, vec2(120.0, 40.0)), 0, egui::Color32::from_rgb(140, 170, 200));
    star_badge(&p, &t, "gallery.badge", photo);
    *roles = registry::widgets(ui.ctx()).into_iter().map(|w| (w.id, w.role)).collect();
}

fn render() -> (Image, Vec<(String, Role)>) {
    let mut roles = Vec::new();
    let img = dalica_ui_kit::snapshot(SIZE.0, SIZE.1, 1.0, &Tokens::default(), |ui| gallery(ui, &mut roles)).unwrap();
    (img, roles)
}

#[test]
fn the_gallery_renders_identically_twice_and_matches_its_fingerprint() {
    let started = Instant::now();
    let (a, roles) = render();
    let first = started.elapsed();
    let (b, _) = render();
    assert_eq!((a.width, a.height), SIZE);
    assert!(a.rgba == b.rgba, "two renders of the same frame differ");

    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui-kit");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("gallery.png"), fingerprint::png_rgba(a.width, a.height, &a.rgba)).unwrap();

    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fingerprints/gallery.json");
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(a.width, a.height, &a.rgba)) + "\n").unwrap();
        println!("blessed {}", stored.display());
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(a.width, a.height, &a.rgba, &expected, 2.0) {
        std::fs::write(out.join("gallery-actual.png"), &m.actual_png).unwrap();
        panic!("{} (render in {})", m.message, out.display());
    }
    println!("gallery {} × {}: first render {:.0} ms", a.width, a.height, first.as_secs_f64() * 1000.0);

    // Every widget registered itself with its role.
    let expect = [
        ("gallery.button", Role::Button),
        ("gallery.button.active", Role::Button),
        ("gallery.button.disabled", Role::Button),
        ("gallery.icon", Role::IconButton),
        ("gallery.icon.active", Role::IconButton),
        ("gallery.icon.disabled", Role::IconButton),
        ("gallery.star.off", Role::Toggle),
        ("gallery.star.on", Role::Toggle),
        ("gallery.badge", Role::Badge),
    ];
    assert_eq!(roles, expect.map(|(id, r)| (id.to_string(), r)));
}

/// Pixels the gallery must have, independent of the fingerprint: the backgrounds are exact tokens,
/// the star badge's glyph is light on its dark plate, and text was drawn.
#[test]
fn the_gallery_paints_tokens_text_and_the_badge() {
    let (img, _) = render();
    let t = Tokens::default();
    let px = |x, y| img.pixel(x, y).unwrap();
    assert_eq!(px(2, 2), t.chrome.to_array());
    assert_eq!(px(334, 185), t.grid_bg.to_array());
    assert_eq!(px(345, 45), t.cell_selected.to_array());
    // The badge: plate at the photo's bottom-right corner (inset 4, 18 pt), glyph in its middle.
    let plate_center = (458 - 4 - 9, 140 - 4 - 9);
    let glyph = px(plate_center.0, plate_center.1);
    assert!(glyph[0] > 180, "the star glyph is light: {glyph:?}");
    let plate_corner = px(458 - 4 - 17, 140 - 4 - 16);
    assert!(plate_corner[0] < 60, "the plate darkens the photo: {plate_corner:?}");
    // The title row has light text pixels on chrome.
    let lit = (12..30).flat_map(|y| (16..140).map(move |x| (x, y))).filter(|&(x, y)| px(x, y)[0] > 150).count();
    assert!(lit > 100, "{lit} text pixels");
}
