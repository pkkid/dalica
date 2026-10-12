//! The Folder Manager's glyphs (M5.18, `organizing.md` §2): the three folder states and the tree
//! roots' icons at 12, 16, 18, 20 and 28 pt, each state also at the inherited 50 % opacity in its
//! own colour (Scan Always `accent`, Scan Once `text_label`, Remove `danger`), rendered by the CPU
//! rasterizer. The same frame gives identical pixels twice and matches the stored fingerprint
//! (`fingerprints/icons-folder-modes.json`); the render is written to
//! `target/test-output/ui-kit/icons-folder-modes.png` (with `DALICA_BLESS=1` also a 3× render, and
//! a new fingerprint is stored: commit it only after looking at both PNGs).

use std::path::PathBuf;

use dalica_scenes::fingerprint;
use dalica_ui_kit::icons::{FolderIcon, INHERITED_OPACITY, paint_folder, state_color};
use dalica_ui_kit::{Image, Tokens, fonts};
use egui::{Align2, Color32, Rect, pos2, vec2};

const SIZES: [f32; 5] = [12.0, 16.0, 18.0, 20.0, 28.0];
const NAME_W: f32 = 120.0;
const CELL_W: f32 = 40.0;
const ROW_H: f32 = 36.0;
const TOP: f32 = 34.0;
const MARGIN: f32 = 12.0;
/// The state glyphs, then the same inherited, then the root icons.
const ROWS: usize = 3 + 3 + 6;
const SIZE: (u32, u32) = ((2.0 * MARGIN + NAME_W + CELL_W * SIZES.len() as f32) as u32, (TOP + ROWS as f32 * ROW_H + 8.0) as u32);

fn out_dir() -> PathBuf {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui-kit");
    std::fs::create_dir_all(&out).unwrap();
    out
}

/// What the sheet draws in each row: the glyph, its colour and its label.
fn rows(t: &Tokens) -> Vec<(FolderIcon, Color32, String)> {
    let states = [(FolderIcon::ScanAlways, t.accent), (FolderIcon::ScanOnce, t.text_label), (FolderIcon::Remove, t.danger)];
    let mut rows: Vec<_> = states.iter().map(|&(i, c)| (i, state_color(c, false), i.name().to_string())).collect();
    rows.extend(states.iter().map(|&(i, c)| (i, state_color(c, true), format!("{} (inherited)", i.name()))));
    rows.extend(FolderIcon::ALL[3..].iter().map(|&i| (i, t.icon, i.name().to_string())));
    rows
}

fn sheet(ui: &mut egui::Ui) {
    let t = Tokens::default();
    let p = ui.painter().clone();
    p.text(pos2(MARGIN, 10.0), Align2::LEFT_TOP, "Glyph", fonts::semibold(t.small), t.text_label);
    for (col, side) in SIZES.iter().enumerate() {
        let cx = MARGIN + NAME_W + col as f32 * CELL_W + CELL_W / 2.0;
        p.text(pos2(cx, 10.0), Align2::CENTER_TOP, format!("{side}"), fonts::semibold(t.small), t.text_label);
    }
    for (row, (icon, color, label)) in rows(&t).into_iter().enumerate() {
        let cy = TOP + row as f32 * ROW_H + ROW_H / 2.0;
        p.text(pos2(MARGIN, cy), Align2::LEFT_CENTER, label, fonts::regular(t.small), t.text_dim);
        for (col, &side) in SIZES.iter().enumerate() {
            let cx = MARGIN + NAME_W + col as f32 * CELL_W + CELL_W / 2.0;
            paint_folder(&p, Rect::from_center_size(pos2(cx, cy), vec2(side, side)), icon, color);
        }
    }
}

fn render(pixels_per_point: f32) -> Image {
    let (w, h) = ((SIZE.0 as f32 * pixels_per_point) as u32, (SIZE.1 as f32 * pixels_per_point) as u32);
    dalica_ui_kit::snapshot(w, h, pixels_per_point, &Tokens::default(), sheet).unwrap()
}

#[test]
fn the_folder_glyph_sheet_renders_identically_twice_and_matches_its_fingerprint() {
    let a = render(1.0);
    let b = render(1.0);
    assert_eq!((a.width, a.height), SIZE);
    assert!(a.rgba == b.rgba, "two renders of the same frame differ");
    let out = out_dir();
    std::fs::write(out.join("icons-folder-modes.png"), fingerprint::png_rgba(a.width, a.height, &a.rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fingerprints/icons-folder-modes.json");
    if std::env::var_os("DALICA_BLESS").is_some() {
        let big = render(3.0);
        std::fs::write(out.join("icons-folder-modes@3x.png"), fingerprint::png_rgba(big.width, big.height, &big.rgba)).unwrap();
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(a.width, a.height, &a.rgba)) + "\n").unwrap();
        println!("blessed {}", stored.display());
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(a.width, a.height, &a.rgba, &expected, 2.0) {
        std::fs::write(out.join("icons-folder-modes-actual.png"), &m.actual_png).unwrap();
        panic!("{} (render in {})", m.message, out.display());
    }
}

const BG: Color32 = Color32::BLACK;
const PITCH_AIR: f32 = 12.0;

/// Each glyph alone on black in a `side` square with 12 px of air round it, in `color`.
fn solo(side: f32, color: Color32) -> Image {
    let pitch = side + 2.0 * PITCH_AIR;
    let (w, h) = ((pitch * FolderIcon::ALL.len() as f32) as u32, pitch as u32);
    dalica_ui_kit::snapshot(w, h, 1.0, &Tokens { chrome: BG, ..Tokens::default() }, |ui| {
        for (i, icon) in FolderIcon::ALL.into_iter().enumerate() {
            paint_folder(ui.painter(), solo_square(i, side), icon, color);
        }
    })
    .unwrap()
}

fn solo_square(i: usize, side: f32) -> Rect {
    let pitch = side + 2.0 * PITCH_AIR;
    Rect::from_min_size(pos2(i as f32 * pitch + PITCH_AIR, PITCH_AIR), vec2(side, side))
}

/// The inked pixels round glyph `i` with their coverage 0 … 1.
fn ink(img: &Image, i: usize, side: f32) -> Vec<(u32, u32, f32)> {
    let cell = solo_square(i, side).expand(PITCH_AIR);
    let mut ink = Vec::new();
    for y in cell.min.y as u32..cell.max.y as u32 {
        for x in cell.min.x as u32..cell.max.x as u32 {
            let px = img.pixel(x, y).unwrap();
            let cover = px[0].max(px[1]).max(px[2]) as f32 / 255.0;
            if cover > 0.0 {
                ink.push((x, y, cover));
            }
        }
    }
    ink
}

/// Every glyph draws something at every size, stays inside its square (half a pixel of
/// anti-aliasing allowed) and is visually centred (its ink's centre of mass within 15 % of the
/// side from the square's centre).
#[test]
fn every_glyph_inks_its_square_only_and_is_centred() {
    for side in SIZES {
        let img = solo(side, Color32::WHITE);
        for (i, icon) in FolderIcon::ALL.into_iter().enumerate() {
            let square = solo_square(i, side);
            let ink = ink(&img, i, side);
            let total: f32 = ink.iter().map(|p| p.2).sum();
            assert!(total >= side * 0.4, "{icon:?} at {side}: only {total:.1} pixels of ink");
            for &(x, y, c) in &ink {
                let centre = pos2(x as f32 + 0.5, y as f32 + 0.5);
                assert!(square.expand(1.0).contains(centre) || c < 0.2, "{icon:?} at {side}: ink {c:.2} at ({x}, {y}) outside {square:?}");
            }
            let (mx, my) = ink.iter().fold((0.0, 0.0), |(ax, ay), &(x, y, c)| (ax + (x as f32 + 0.5) * c, ay + (y as f32 + 0.5) * c));
            let off = pos2(mx / total, my / total) - square.center();
            assert!(off.x.abs() <= 0.15 * side && off.y.abs() <= 0.15 * side, "{icon:?} at {side}: ink centred at {off:?} from the middle");
        }
    }
}

/// No two glyphs look alike at 16 pt (the tree-row size): for each pair at least a fifth of the
/// larger one's ink differs clearly.
#[test]
fn every_glyph_is_distinct_at_16_pt() {
    let side = 16.0;
    let img = solo(side, Color32::WHITE);
    let cells: Vec<Vec<u8>> = (0..FolderIcon::ALL.len())
        .map(|i| {
            let sq = solo_square(i, side).expand(1.0);
            (sq.min.y as u32..sq.max.y as u32)
                .flat_map(|y| (sq.min.x as u32..sq.max.x as u32).map(move |x| (x, y)))
                .map(|(x, y)| img.pixel(x, y).unwrap()[0])
                .collect()
        })
        .collect();
    let inked = |c: &[u8]| c.iter().filter(|&&v| v > 96).count();
    for a in 0..cells.len() {
        for b in a + 1..cells.len() {
            let differ = cells[a].iter().zip(&cells[b]).filter(|(p, q)| p.abs_diff(**q) > 96).count();
            let share = differ as f32 / inked(&cells[a]).max(inked(&cells[b])).max(1) as f32;
            assert!(share >= 0.2, "{:?} and {:?} differ in only {:.0} % of their ink", FolderIcon::ALL[a], FolderIcon::ALL[b], share * 100.0);
        }
    }
}

/// Glyphs take the caller's colour, and an inherited state is drawn at half its opacity: over black
/// its ink is half as bright.
#[test]
fn glyphs_take_the_callers_colour_and_inherited_ones_are_half_as_opaque() {
    let red = solo(20.0, Color32::from_rgb(255, 0, 0));
    assert!(red.rgba.as_chunks::<4>().0.iter().all(|px| px[1] == 0 && px[2] == 0));
    let full = solo(20.0, Color32::WHITE);
    let half = solo(20.0, state_color(Color32::WHITE, true));
    for (i, icon) in FolderIcon::ALL.into_iter().enumerate() {
        let sum = |img: &Image| ink(img, i, 20.0).iter().map(|p| p.2).sum::<f32>();
        let ratio = sum(&half) / sum(&full);
        assert!((ratio - INHERITED_OPACITY).abs() < 0.12, "{icon:?}: inherited ink is {ratio:.2} of the full ink");
    }
}

/// Degenerate rectangles paint nothing and never panic.
#[test]
fn empty_or_non_finite_rects_paint_nothing() {
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
            for icon in FolderIcon::ALL {
                paint_folder(ui.painter(), rect, icon, Color32::WHITE);
            }
        }
    })
    .unwrap();
    assert!(img.rgba.as_chunks::<4>().0.iter().all(|px| *px == [0, 0, 0, 255]), "something was painted");
}
