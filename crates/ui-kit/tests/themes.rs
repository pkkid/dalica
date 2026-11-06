//! Themes (`design-system.md` §2, Themes; M16.3, M16.7): the fifteen `ThemeKind`s, their ids, labels and
//! names, `next` round the list; `Tokens::for_kind` (Dark is `Tokens::DARK`, a theme not ported yet
//! wears Dark's values); the rule table's fixed values; Classic's bevel; and the gallery of widgets
//! fingerprinted in every theme (`fingerprints/theme-<id>-1400x900.json`). The renders go to
//! `target/test-output/ui-kit/`; `DALICA_BLESS=1` stores new fingerprints (look at the PNGs first).

use std::collections::HashSet;

use crate::common::check;
use dalica_ui_kit::theme::{BEVEL_DARK, BEVEL_LIGHT, Palette};
use dalica_ui_kit::widgets::{
    Segment, SegmentLabel, action_button, checkbox, dropdown, filter_banner, icon_button, primary_button, radio, search_field, segmented, star_badge,
    star_toggle, text_button, text_field, toggle_filter, zoom_slider,
};
use dalica_ui_kit::{Icon, Image, ThemeKind, Tokens, fonts};
use egui::{Align2, Color32, CornerRadius, Rect, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

const IDS: [&str; 15] = [
    "dark",
    "pro",
    "proMedium",
    "proLightGray",
    "proLight",
    "studio",
    "studioLight",
    "classic",
    "adwaita",
    "adwaitaDark",
    "solarizedDark",
    "breezeLight",
    "breezeDark",
    "softDark",
    "softLight",
];

#[test]
fn every_id_and_label_names_its_theme_and_unknown_names_none() {
    assert_eq!(ThemeKind::ALL.map(ThemeKind::id), IDS);
    assert_eq!(ThemeKind::default(), ThemeKind::Dark);
    assert_eq!(ThemeKind::Dark.label(), "Dark");
    let labels: HashSet<&str> = ThemeKind::ALL.iter().map(|k| k.label()).collect();
    assert_eq!(labels.len(), 15, "labels are distinct");
    for kind in ThemeKind::ALL {
        assert_eq!(ThemeKind::from_name(kind.id()), Some(kind), "{kind:?} by id");
        assert_eq!(ThemeKind::from_name(kind.label()), Some(kind), "{kind:?} by label");
        assert_eq!(ThemeKind::from_name(&kind.id().to_uppercase()), Some(kind), "{kind:?} ignoring case");
    }
    // PhotoCraft's aliases, kept.
    for (name, kind) in [
        ("Pro (Medium Gray)", ThemeKind::ProMedium),
        ("medium_gray", ThemeKind::ProMedium),
        ("light-gray", ThemeKind::ProLightGray),
        ("lightest", ThemeKind::ProLight),
        ("studio (dark)", ThemeKind::Studio),
        ("retro", ThemeKind::Classic),
        ("Adwaita Light", ThemeKind::Adwaita),
        ("solarized", ThemeKind::SolarizedDark),
        ("breeze", ThemeKind::BreezeLight),
    ] {
        assert_eq!(ThemeKind::from_name(name), Some(kind), "{name}");
    }
    // Dalica's Dark owns `dark`; `light` and the names of other companies' products are dropped.
    assert_eq!(ThemeKind::from_name("dark"), Some(ThemeKind::Dark));
    for name in ["light", "gnome", "kde", "plasma", "win2000", "neon", "", " ", "pro dark x", "dark\u{0}"] {
        assert_eq!(ThemeKind::from_name(name), None, "{name:?}");
    }
}

#[test]
fn next_visits_all_fifteen_and_comes_back() {
    let mut seen = Vec::new();
    let mut k = ThemeKind::Dark;
    for _ in 0..15 {
        seen.push(k);
        k = k.next();
    }
    assert_eq!(k, ThemeKind::Dark, "back to the first");
    assert_eq!(seen, ThemeKind::ALL);
}

#[test]
fn dark_is_the_dark_tokens_and_every_kind_reports_itself() {
    assert_eq!(Tokens::for_kind(ThemeKind::Dark), Tokens::DARK);
    assert_eq!(Tokens::DARK.kind, ThemeKind::Dark);
    assert!(Tokens::DARK.is_dark() && !Tokens::DARK.bevel);
    for kind in ThemeKind::ALL {
        let t = Tokens::for_kind(kind);
        assert_eq!(t.kind, kind);
        // What never changes with a theme.
        assert_eq!((t.body, t.title, t.font_scale, t.focus_width), (13.0, 15.0, 1.0, 1.5), "{kind:?}");
        assert_eq!(t.metrics, Tokens::DARK.metrics, "{kind:?}");
        assert_eq!(
            (t.viewer_bg, t.overlay_bar, t.face_box, t.crosshair, t.backdrop),
            (Tokens::DARK.viewer_bg, Tokens::DARK.overlay_bar, Tokens::DARK.face_box, Tokens::DARK.crosshair, Tokens::DARK.backdrop)
        );
    }
}

/// A light, square-cornered palette for the rule table and the bevel (not a real theme).
fn test_palette(bevel: bool) -> Palette {
    let g = Color32::from_gray;
    Palette {
        chrome: g(212),
        canvas: g(128),
        canvas_dot: g(128),
        dock: g(200),
        card: g(212),
        card_border: g(128),
        field: Color32::WHITE,
        field_border: g(128),
        hover: g(226),
        pressed: g(190),
        text: Color32::BLACK,
        text_dim: g(64),
        text_faint: g(128),
        icon: Color32::BLACK,
        accent: Color32::from_rgb(10, 36, 106),
        accent_soft: Color32::from_rgb(10, 36, 106),
        accent_border: Color32::from_rgb(10, 36, 106),
        accent_text: Color32::WHITE,
        separator: g(128),
        shadow: Color32::from_black_alpha(0),
        primary_bg: g(212),
        primary_text: Color32::BLACK,
        danger: Color32::from_rgb(160, 0, 0),
        warning: Color32::from_rgb(128, 96, 0),
        tab_strip: g(212),
        row_selected: Color32::from_rgb(10, 36, 106),
        histogram_bg: g(40),
        caption_close: Color32::from_rgb(196, 43, 28),
        caption_close_text: Color32::WHITE,
        scrim: Color32::from_black_alpha(110),
        radius_sm: 0,
        radius: 0,
        radius_lg: 0,
        bevel,
    }
}

#[test]
fn the_rule_table_maps_a_palette_onto_the_tokens() {
    let p = test_palette(true);
    let t = Tokens::from_palette(ThemeKind::Classic, &p);
    assert_eq!(t.kind, ThemeKind::Classic);
    assert!(t.bevel && !t.is_dark());
    assert_eq!((t.chrome, t.canvas, t.grid_bg, t.field, t.divider), (p.chrome, p.canvas, p.dock, p.field, p.separator));
    assert_eq!((t.text, t.text_dim, t.text_disabled, t.icon, t.accent, t.caution), (p.text, p.text_dim, p.text_faint, p.icon, p.accent, p.warning));
    assert_eq!((t.radius, t.window_radius, t.toast_radius, t.overlay_radius, t.filter_radius), (0, 0, 0, 0, 0), "square corners");
    assert_eq!(t.on_accent, Color32::WHITE, "the better of white and black on the accent");
    assert_eq!(t.photo_focus, Color32::BLACK, "the selection outline reads on a light table");
    assert_eq!(t.window_shadow.color, p.shadow);
    assert_eq!((t.metrics, t.body, t.viewer_bg), (Tokens::DARK.metrics, Tokens::DARK.body, Tokens::DARK.viewer_bg));
}

/// A `text_button` in tokens `t`: the image and the button's rect.
fn button(t: &Tokens) -> (Image, Rect) {
    let mut rect = Rect::NOTHING;
    let img = dalica_ui_kit::snapshot(120, 64, 1.0, t, |ui| {
        ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(pos2(20.0, 20.0), vec2(90.0, 24.0))), |ui| {
            rect = text_button(ui, t, "button:bevel", "Bevel", false).rect;
        });
    })
    .unwrap();
    (img, rect)
}

#[test]
fn a_bevelled_button_has_a_light_top_edge_and_a_dark_bottom_edge() {
    let on = Tokens::from_palette(ThemeKind::Classic, &test_palette(true));
    let (img, r) = button(&on);
    let x = r.center().x as u32;
    let (top, bottom) = (r.min.y as u32, r.max.y as u32 - 1);
    assert_eq!(img.pixel(x, top).unwrap(), BEVEL_LIGHT.to_array(), "light top edge of {r:?}");
    assert_eq!(img.pixel(x, bottom).unwrap(), BEVEL_DARK.to_array(), "dark bottom edge");
    assert_eq!(img.pixel(r.min.x as u32, r.center().y as u32).unwrap(), BEVEL_LIGHT.to_array(), "light left edge");
    assert_eq!(img.pixel(r.max.x as u32 - 1, r.center().y as u32).unwrap(), BEVEL_DARK.to_array(), "dark right edge");

    let off = Tokens::from_palette(ThemeKind::Classic, &test_palette(false));
    let (img, r) = button(&off);
    let x = r.center().x as u32;
    for y in [r.min.y as u32, r.max.y as u32 - 1] {
        assert_eq!(img.pixel(x, y).unwrap(), off.button_border.to_array(), "no bevel: the plain border");
    }
}

/// The widget gallery of a theme: the chrome's text levels and controls in their states, a dialog
/// panel, and a light table with a folder header, cells, the selection outlines, a badge, the
/// filter banner, the tray and a toast plate.
fn gallery(ui: &mut Ui, t: &Tokens) {
    let p = ui.painter().clone();
    p.text(pos2(24.0, 16.0), Align2::LEFT_TOP, "Widget gallery", fonts::semibold(t.empty_title), t.text);
    for (i, (what, color)) in
        [("Text", t.text), ("Label text", t.text_label), ("Dim text", t.text_dim), ("Disabled text", t.text_disabled)].into_iter().enumerate()
    {
        p.text(pos2(24.0, 48.0 + 20.0 * i as f32), Align2::LEFT_TOP, what, fonts::regular(t.body), color);
    }
    let at = |ui: &mut Ui, x: f32, y: f32, w: f32, add: &mut dyn FnMut(&mut Ui)| {
        ui.scope_builder(
            UiBuilder::new().max_rect(Rect::from_min_size(pos2(x, y), vec2(w, 32.0))).layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| add(ui),
        );
    };
    at(ui, 24.0, 136.0, 360.0, &mut |ui| {
        text_button(ui, t, "button:g.rest", "Add Folder…", false);
        text_button(ui, t, "button:g.active", "Hold", true);
        ui.add_enabled_ui(false, |ui| text_button(ui, t, "button:g.disabled", "Clear", false));
        primary_button(ui, t, "button:g.primary", "OK");
        ui.add_enabled_ui(false, |ui| primary_button(ui, t, "button:g.primaryOff", "Apply"));
    });
    at(ui, 24.0, 176.0, 360.0, &mut |ui| {
        star_toggle(ui, t, "button:g.starOff", false);
        star_toggle(ui, t, "button:g.starOn", true);
        icon_button(ui, t, "button:g.icon", Icon::Plus, 28.0, false, true, "Add");
        icon_button(ui, t, "button:g.iconOn", Icon::Star, 28.0, true, true, "Starred");
        icon_button(ui, t, "button:g.iconOff", Icon::StarOutline, 28.0, false, false, "Nothing");
        toggle_filter(ui, t, "button:g.filterOn", Icon::Star, true, "Starred only");
        toggle_filter(ui, t, "button:g.filterOff", Icon::Faces, false, "Faces only");
    });
    at(ui, 24.0, 216.0, 360.0, &mut |ui| {
        let segs = [
            Segment { id: "button:g.flat", label: SegmentLabel::Icon(Icon::FlatList), tooltip: "Flat" },
            Segment { id: "button:g.tree", label: SegmentLabel::Icon(Icon::Tree), tooltip: "Tree" },
        ];
        segmented(ui, t, &segs, Some(1), vec2(30.0, 24.0));
        dropdown(ui, t, "button:g.sort", "Sort by Date", |_| {});
        let mut v = 144.0;
        zoom_slider(ui, t, "slider:g.size", &mut v, 64.0..=320.0, "Thumbnail size");
    });
    at(ui, 24.0, 256.0, 360.0, &mut |ui| {
        let mut name = String::from("Holiday 2026");
        text_field(ui, t, "field:g.name", &mut name, "Name", 160.0);
        let mut empty = String::new();
        search_field(ui, t, "field:g.search", "button:g.clear", &mut empty, "Search", 160.0);
    });
    at(ui, 24.0, 296.0, 360.0, &mut |ui| {
        let (mut on, mut off) = (true, false);
        checkbox(ui, t, "check:g.on", "On", &mut on);
        checkbox(ui, t, "check:g.off", "Off", &mut off);
        radio(ui, t, "radio:g.on", "Chosen", true);
        radio(ui, t, "radio:g.off", "Other", false);
    });
    at(ui, 24.0, 336.0, 360.0, &mut |ui| {
        action_button(ui, t, "action:g.export", Icon::Import, "Import", false, true);
        action_button(ui, t, "action:g.album", Icon::NewAlbum, "Album", true, true);
        ui.add_enabled_ui(false, |ui| action_button(ui, t, "action:g.off", Icon::AddToAlbum, "Add", false, true));
    });

    // A dialog's panel.
    let panel = Rect::from_min_size(pos2(24.0, 430.0), vec2(360.0, 150.0));
    p.rect(panel, CornerRadius::same(t.window_radius), t.chrome, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    dalica_ui_kit::widgets::bevel(&p, t, panel, true);
    p.text(panel.min + vec2(16.0, 12.0), Align2::LEFT_TOP, "Rename", fonts::semibold(t.title), t.text);
    p.text(panel.min + vec2(16.0, 40.0), Align2::LEFT_TOP, "A dialog's body text.", fonts::regular(t.body), t.text_label);
    at(ui, 200.0, 534.0, 170.0, &mut |ui| {
        text_button(ui, t, "button:g.cancel", "Cancel", false);
        primary_button(ui, t, "button:g.ok", "Rename");
    });
    // The tray.
    let tray = Rect::from_min_size(pos2(24.0, 620.0), vec2(360.0, t.metrics.tray_h));
    p.rect_filled(tray, CornerRadius::same(t.radius), t.tray_bg);
    p.text(tray.center(), Align2::CENTER_CENTER, "Photo tray", fonts::regular(t.body), t.text_dim);

    // The light table.
    let table = Rect::from_min_max(pos2(420.0, 0.0), pos2(1400.0, 900.0));
    p.rect_filled(table, 0, t.grid_bg);
    p.vline(table.min.x, table.y_range(), Stroke::new(1.0, t.divider));
    ui.scope_builder(UiBuilder::new().max_rect(Rect::from_min_size(table.min, vec2(table.width(), 40.0))), |ui| {
        filter_banner(ui, t, "banner:g", "button:g.showAll", "Showing starred photos");
    });
    let header = Rect::from_min_size(pos2(444.0, 60.0), vec2(932.0, 72.0));
    p.rect_filled(header, 0, t.folder_header);
    p.text(header.min + vec2(16.0, 12.0), Align2::LEFT_TOP, "Summer 2026", fonts::semibold(t.title), t.text);
    p.text(header.min + vec2(16.0, 38.0), Align2::LEFT_TOP, "Aug 4, 2026 · 6 photos", fonts::regular(t.detail), t.text_dim);
    let photos = [
        Color32::from_rgb(70, 110, 150),
        Color32::from_rgb(180, 140, 60),
        Color32::from_rgb(60, 130, 80),
        Color32::from_rgb(200, 200, 200),
        Color32::from_rgb(150, 60, 70),
        Color32::from_rgb(30, 30, 30),
    ];
    for (i, photo) in photos.into_iter().enumerate() {
        let cell = Rect::from_min_size(pos2(444.0 + 156.0 * i as f32, 156.0), vec2(144.0, 144.0));
        let selected = i == 1 || i == 2;
        p.rect_filled(cell, 0, if selected { t.cell_selected } else { t.cell });
        let image = Rect::from_center_size(cell.center(), vec2(128.0, 86.0));
        // The last one's texture has not arrived.
        p.rect_filled(image, 0, if i == 5 { t.placeholder } else { photo });
        if selected {
            let color = if i == 1 { t.photo_focus } else { t.photo_selected };
            p.rect_stroke(image, 0, Stroke::new(2.0, color), StrokeKind::Outside);
        }
        if i % 2 == 0 {
            star_badge(&p, t, &format!("badge:g.{i}"), image);
        }
        p.text(pos2(cell.center().x, cell.max.y + 4.0), Align2::CENTER_TOP, format!("IMG_{:04}.jpg", 101 + i), fonts::regular(t.caption), t.text_dim);
    }
    // A toast plate on the light table.
    let toast = Rect::from_center_size(pos2(910.0, 820.0), vec2(320.0, 40.0));
    p.rect_filled(toast, CornerRadius::same(t.toast_radius), t.toast_bg);
    p.text(toast.center(), Align2::CENTER_CENTER, "Moved 2 photos to the trash", fonts::regular(t.large), t.text);
}

fn render(t: &Tokens) -> Image {
    dalica_ui_kit::snapshot(1400, 900, 1.0, t, |ui| gallery(ui, t)).unwrap()
}

/// One fingerprint per theme. Themes whose tokens are the same colours (the twelve wear Dark's
/// until their palettes land) share one render.
#[test]
fn the_gallery_in_every_theme_matches_its_fingerprint() {
    let mut rendered: Vec<(Tokens, Image)> = Vec::new();
    let mut failures = Vec::new();
    for kind in ThemeKind::ALL {
        let t = Tokens::for_kind(kind);
        let key = Tokens { kind: ThemeKind::Dark, ..t };
        let i = match rendered.iter().position(|(k, _)| *k == key) {
            Some(i) => i,
            None => {
                rendered.push((key, render(&t)));
                rendered.len() - 1
            }
        };
        let (_, img) = &rendered[i];
        check(&format!("theme-{}-1400x900", kind.id()), img, &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
