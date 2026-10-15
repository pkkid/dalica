//! The shortcut editor's glyphs, [`OptionsIcon`] (`design-system.md` §5, `menus-and-shortcuts.md`
//! §2.0): Restore (↺) and Clear (×), our own drawings on the 20-unit grid.
//!
//! - the sheet: both glyphs at 12, 16, 20 and 28 pt in the rest (`icon`), hover (`text`) and
//!   disabled (`text_disabled`) colours, at 1× and 2×, renders identically twice and matches its
//!   fingerprint (`fingerprints/icons-options[@2x].json`; renders in `target/test-output/ui-kit/`,
//!   `DALICA_BLESS=1` stores new ones with a 3× render to judge the drawings);
//! - each glyph inks its square only and is centred, at every size in every colour;
//! - the two read apart, and apart from the shell icons they resemble (Rotate Counterclockwise,
//!   Close, Clear Search, the caption Restore), at 16 pt;
//! - they take the caller's colour, and degenerate rectangles paint nothing.

use crate::common::{check_one, out_dir};
use dalica_scenes::fingerprint;
use dalica_ui_kit::icons::{self, OptionsIcon, paint_options};
use dalica_ui_kit::{Icon, Image, Tokens, fonts};
use egui::{Align2, Color32, Rect, pos2, vec2};

const SIZES: [f32; 4] = [12.0, 16.0, 20.0, 28.0];
const BG: Color32 = Color32::BLACK;
/// Air round each glyph when it is drawn alone.
const AIR: f32 = 12.0;

/// The three states, with the token each paints in.
fn states(t: &Tokens) -> [(&'static str, Color32); 3] {
    [("rest", t.icon), ("hover", t.text), ("disabled", t.text_disabled)]
}

const NAME_W: f32 = 112.0;
const CELL_W: f32 = 38.0;
const GROUP_GAP: f32 = 14.0;
const ROW_H: f32 = 36.0;
const TOP: f32 = 46.0;
const MARGIN: f32 = 12.0;
const GROUP_W: f32 = CELL_W * SIZES.len() as f32;

fn sheet_size() -> (u32, u32) {
    ((2.0 * MARGIN + NAME_W + 3.0 * GROUP_W + 2.0 * GROUP_GAP) as u32, (TOP + OptionsIcon::ALL.len() as f32 * ROW_H + 8.0) as u32)
}

/// The sheet: one row per glyph, three state groups of four sizes.
fn sheet(ui: &mut egui::Ui) {
    let t = Tokens::default();
    let p = ui.painter().clone();
    p.text(pos2(MARGIN, 10.0), Align2::LEFT_TOP, "Glyph", fonts::semibold(t.small), t.text_label);
    for (g, (state, color)) in states(&t).into_iter().enumerate() {
        let x0 = MARGIN + NAME_W + g as f32 * (GROUP_W + GROUP_GAP);
        p.text(pos2(x0 + GROUP_W / 2.0, 6.0), Align2::CENTER_TOP, state, fonts::semibold(t.small), t.text_label);
        for (col, side) in SIZES.iter().enumerate() {
            let cx = x0 + col as f32 * CELL_W + CELL_W / 2.0;
            p.text(pos2(cx, 24.0), Align2::CENTER_TOP, format!("{side}"), fonts::regular(t.small), t.text_dim);
        }
        for (row, icon) in OptionsIcon::ALL.into_iter().enumerate() {
            let cy = TOP + row as f32 * ROW_H + ROW_H / 2.0;
            if g == 0 {
                p.text(pos2(MARGIN, cy), Align2::LEFT_CENTER, icon.name(), fonts::regular(t.small), t.text_dim);
            }
            for (col, &side) in SIZES.iter().enumerate() {
                let cx = x0 + col as f32 * CELL_W + CELL_W / 2.0;
                paint_options(&p, Rect::from_center_size(pos2(cx, cy), vec2(side, side)), icon, color);
            }
        }
    }
}

fn render(pixels_per_point: f32) -> Image {
    let (w, h) = sheet_size();
    let (w, h) = ((w as f32 * pixels_per_point) as u32, (h as f32 * pixels_per_point) as u32);
    dalica_ui_kit::snapshot(w, h, pixels_per_point, &Tokens::default(), sheet).unwrap()
}

#[test]
fn the_sheet_renders_identically_twice_and_matches_its_fingerprint() {
    for (name, ppp) in [("icons-options", 1.0), ("icons-options@2x", 2.0)] {
        let (a, b) = (render(ppp), render(ppp));
        assert!(a.rgba == b.rgba, "{name}: two renders of the same frame differ");
        if ppp == 1.0 && std::env::var_os("DALICA_BLESS").is_some() {
            let big = render(3.0);
            std::fs::write(out_dir().join(format!("{name}@3x.png")), fingerprint::png_rgba(big.width, big.height, &big.rgba)).unwrap();
        }
        check_one(name, &a);
    }
}

// Each glyph alone.

fn solo_square(i: usize, side: f32) -> Rect {
    let pitch = side + 2.0 * AIR;
    Rect::from_min_size(pos2(i as f32 * pitch + AIR, AIR), vec2(side, side))
}

/// Each glyph alone on black in a `side` square with 12 px of air round it, in `color`.
fn solo(side: f32, color: Color32) -> Image {
    let pitch = side + 2.0 * AIR;
    let (w, h) = ((pitch * OptionsIcon::ALL.len() as f32) as u32, pitch as u32);
    dalica_ui_kit::snapshot(w, h, 1.0, &Tokens { chrome: BG, ..Tokens::default() }, |ui| {
        for (i, icon) in OptionsIcon::ALL.into_iter().enumerate() {
            paint_options(ui.painter(), solo_square(i, side), icon, color);
        }
    })
    .unwrap()
}

/// Every glyph draws something at 12, 16, 20 and 28 pt in its rest, hover and disabled colours,
/// stays inside its square (half a pixel of anti-aliasing allowed) and is visually centred: its
/// ink's centre of mass within 15 % of the side from the square's centre.
#[test]
fn both_glyphs_stay_in_their_grid_at_every_size_and_state() {
    let t = Tokens::default();
    for (state, color) in states(&t) {
        let full = color.r().max(color.g()).max(color.b()).max(1) as f32;
        for side in SIZES {
            let img = solo(side, color);
            for (i, icon) in OptionsIcon::ALL.into_iter().enumerate() {
                let case = format!("{icon:?} {state} at {side}");
                let square = solo_square(i, side);
                let cell = square.expand(AIR);
                let mut ink = Vec::new();
                for y in cell.min.y as u32..cell.max.y as u32 {
                    for x in cell.min.x as u32..cell.max.x as u32 {
                        let px = img.pixel(x, y).unwrap();
                        let cover = px[0].max(px[1]).max(px[2]) as f32 / full;
                        if cover > 0.0 {
                            ink.push((x, y, cover));
                        }
                    }
                }
                let total: f32 = ink.iter().map(|p| p.2).sum();
                assert!(total >= side * 0.4, "{case}: only {total:.1} pixels of ink");
                for &(x, y, c) in &ink {
                    let centre = pos2(x as f32 + 0.5, y as f32 + 0.5);
                    assert!(square.expand(1.0).contains(centre) || c < 0.2, "{case}: ink {c:.2} at ({x}, {y}) outside {square:?}");
                }
                let (mx, my) = ink.iter().fold((0.0, 0.0), |(ax, ay), &(x, y, c)| (ax + (x as f32 + 0.5) * c, ay + (y as f32 + 0.5) * c));
                let off = pos2(mx / total, my / total) - square.center();
                assert!(off.x.abs() <= 0.15 * side && off.y.abs() <= 0.15 * side, "{case}: ink centred at {off:?} from the middle");
            }
        }
    }
}

/// How much of the larger glyph's ink differs clearly between two cells (red channel, 0 … 255).
fn share_differing(a: &[u8], b: &[u8]) -> f32 {
    let inked = |c: &[u8]| c.iter().filter(|&&v| v > 96).count();
    let differ = a.iter().zip(b).filter(|(p, q)| p.abs_diff(**q) > 96).count();
    differ as f32 / inked(a).max(inked(b)).max(1) as f32
}

/// At 16 pt, Restore and Clear differ in at least a fifth of their ink, and so does each from the
/// shell icons it could be taken for.
#[test]
fn the_glyphs_read_apart_from_each_other_and_their_shell_lookalikes() {
    let side = 16.0;
    let cell = |paint: &dyn Fn(&egui::Painter, Rect)| -> Vec<u8> {
        let img = dalica_ui_kit::snapshot(40, 40, 1.0, &Tokens { chrome: BG, ..Tokens::default() }, |ui| {
            paint(ui.painter(), Rect::from_min_size(pos2(AIR, AIR), vec2(side, side)));
        })
        .unwrap();
        img.rgba.as_chunks::<4>().0.iter().map(|px| px[0]).collect()
    };
    let ours: Vec<(OptionsIcon, Vec<u8>)> = OptionsIcon::ALL.into_iter().map(|i| (i, cell(&|p, r| paint_options(p, r, i, Color32::WHITE)))).collect();
    let share = share_differing(&ours[0].1, &ours[1].1);
    assert!(share >= 0.2, "Restore and Clear differ in only {:.0} % of their ink", share * 100.0);
    for (icon, a) in &ours {
        for theirs in [Icon::RotateCcw, Icon::RotateCw, Icon::Close, Icon::ClearSearch, Icon::Restore, Icon::ClearTray] {
            let b = cell(&|p, r| icons::paint(p, r, theirs, Color32::WHITE));
            let share = share_differing(a, &b);
            assert!(share >= 0.2, "{icon:?} and {theirs:?} differ in only {:.0} % of their ink", share * 100.0);
        }
    }
}

/// Painted in pure red no pixel gains green or blue; degenerate rectangles paint nothing.
#[test]
fn the_glyphs_take_the_callers_colour_and_ignore_bad_rects() {
    let red = solo(20.0, Color32::from_rgb(255, 0, 0));
    let mut inked = 0;
    for px in red.rgba.as_chunks::<4>().0 {
        assert_eq!((px[1], px[2]), (0, 0), "{px:?}");
        inked += usize::from(px[0] > 0);
    }
    assert!(inked > 100, "{inked} red pixels");
    let bad = [
        Rect::NOTHING,
        Rect::from_min_size(pos2(5.0, 5.0), vec2(0.0, 12.0)),
        Rect::from_min_size(pos2(5.0, 5.0), vec2(-4.0, -4.0)),
        Rect::from_min_size(pos2(f32::NAN, 5.0), vec2(12.0, 12.0)),
        Rect::from_min_size(pos2(5.0, 5.0), vec2(f32::INFINITY, 12.0)),
        Rect::EVERYTHING,
    ];
    let img = dalica_ui_kit::snapshot(32, 32, 1.0, &Tokens { chrome: BG, ..Tokens::default() }, |ui| {
        for rect in bad {
            for icon in OptionsIcon::ALL {
                paint_options(ui.painter(), rect, icon, Color32::WHITE);
            }
        }
    })
    .unwrap();
    assert!(img.rgba.as_chunks::<4>().0.iter().all(|px| *px == [0, 0, 0, 255]), "something was painted");
}
