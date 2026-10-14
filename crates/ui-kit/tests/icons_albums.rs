//! The automatic albums' glyphs (M9.5, `organizing.md` §4.1, `design-system.md` §5): every
//! [`AlbumIcon`] (Starred Photos' book with a star, Recently Updated's book with a clock, the
//! search results' book with a magnifier) at 12, 16, 20 and 28 pt in the rest (`icon`), hover
//! (`text`) and disabled (`text_disabled`) colours, rendered by the CPU rasterizer at 1× and 2×.
//! The same frame gives identical pixels twice and matches the stored fingerprints
//! (`fingerprints/icons-albums.json`, `fingerprints/icons-albums@2x.json`); the renders are written
//! to `target/test-output/ui-kit/icons-albums.png` and `icons-albums@2x.png` (with
//! `DALICA_BLESS=1` new fingerprints are stored: commit them only after looking at both PNGs).

use std::path::PathBuf;

use dalica_scenes::fingerprint;
use dalica_ui_kit::icons::{self, AlbumIcon, paint_album};
use dalica_ui_kit::{Icon, Image, Tokens, fonts};
use egui::{Align2, Color32, Rect, pos2, vec2};

const SIZES: [f32; 4] = [12.0, 16.0, 20.0, 28.0];
const NAME_W: f32 = 92.0;
const CELL_W: f32 = 38.0;
const GROUP_GAP: f32 = 14.0;
const ROW_H: f32 = 36.0;
const TOP: f32 = 46.0;
const MARGIN: f32 = 12.0;
const GROUP_W: f32 = CELL_W * SIZES.len() as f32;
const SIZE: (u32, u32) = ((2.0 * MARGIN + NAME_W + 3.0 * GROUP_W + 2.0 * GROUP_GAP) as u32, (TOP + AlbumIcon::ALL.len() as f32 * ROW_H + 8.0) as u32);

/// The three states the sheet shows, with the token each paints in.
fn states(t: &Tokens) -> [(&'static str, Color32); 3] {
    [("rest", t.icon), ("hover", t.text), ("disabled", t.text_disabled)]
}

fn out_dir() -> PathBuf {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui-kit");
    std::fs::create_dir_all(&out).unwrap();
    out
}

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
        for (row, icon) in AlbumIcon::ALL.into_iter().enumerate() {
            let cy = TOP + row as f32 * ROW_H + ROW_H / 2.0;
            if g == 0 {
                p.text(pos2(MARGIN, cy), Align2::LEFT_CENTER, icon.name(), fonts::regular(t.small), t.text_dim);
            }
            for (col, &side) in SIZES.iter().enumerate() {
                let cx = x0 + col as f32 * CELL_W + CELL_W / 2.0;
                paint_album(&p, Rect::from_center_size(pos2(cx, cy), vec2(side, side)), icon, color);
            }
        }
    }
}

fn render(pixels_per_point: f32) -> Image {
    let (w, h) = ((SIZE.0 as f32 * pixels_per_point) as u32, (SIZE.1 as f32 * pixels_per_point) as u32);
    dalica_ui_kit::snapshot(w, h, pixels_per_point, &Tokens::default(), sheet).unwrap()
}

fn check_sheet(pixels_per_point: f32, name: &str) {
    let a = render(pixels_per_point);
    let b = render(pixels_per_point);
    assert_eq!((a.width, a.height), ((SIZE.0 as f32 * pixels_per_point) as u32, (SIZE.1 as f32 * pixels_per_point) as u32));
    assert!(a.rgba == b.rgba, "two renders of the same frame differ");
    let out = out_dir();
    std::fs::write(out.join(format!("{name}.png")), fingerprint::png_rgba(a.width, a.height, &a.rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(a.width, a.height, &a.rgba)) + "\n").unwrap();
        println!("blessed {}", stored.display());
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(a.width, a.height, &a.rgba, &expected, 2.0) {
        std::fs::write(out.join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{} (render in {})", m.message, out.display());
    }
}

#[test]
fn the_album_icon_sheet_renders_identically_twice_and_matches_its_fingerprint() {
    check_sheet(1.0, "icons-albums");
}

#[test]
fn the_album_icon_sheet_at_2x_renders_identically_twice_and_matches_its_fingerprint() {
    check_sheet(2.0, "icons-albums@2x");
}

const BG: Color32 = Color32::BLACK;
const AIR: f32 = 12.0;

/// Each glyph alone on black in a `side` square with 12 px of air round it, in `color`.
fn solo(side: f32, color: Color32) -> Image {
    let pitch = side + 2.0 * AIR;
    let (w, h) = ((pitch * AlbumIcon::ALL.len() as f32) as u32, pitch as u32);
    dalica_ui_kit::snapshot(w, h, 1.0, &Tokens { chrome: BG, ..Tokens::default() }, |ui| {
        for (i, icon) in AlbumIcon::ALL.into_iter().enumerate() {
            paint_album(ui.painter(), solo_square(i, side), icon, color);
        }
    })
    .unwrap()
}

fn solo_square(i: usize, side: f32) -> Rect {
    let pitch = side + 2.0 * AIR;
    Rect::from_min_size(pos2(i as f32 * pitch + AIR, AIR), vec2(side, side))
}

/// The inked pixels round glyph `i` with their coverage 0 … 1, relative to `color`'s brightest
/// channel (so a dim colour's full coverage still reads 1).
fn ink(img: &Image, i: usize, side: f32, color: Color32) -> Vec<(u32, u32, f32)> {
    let full = color.r().max(color.g()).max(color.b()).max(1) as f32;
    let cell = solo_square(i, side).expand(AIR);
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
    ink
}

/// Every glyph draws something at every size and in every state colour, stays inside its square
/// (half a pixel of anti-aliasing allowed) and is visually centred (its ink's centre of mass within
/// 15 % of the side from the square's centre).
#[test]
fn every_glyph_inks_its_square_only_and_is_centred_in_every_state() {
    let t = Tokens::default();
    for (state, color) in states(&t) {
        for side in SIZES {
            let img = solo(side, color);
            for (i, icon) in AlbumIcon::ALL.into_iter().enumerate() {
                let square = solo_square(i, side);
                let ink = ink(&img, i, side, color);
                let total: f32 = ink.iter().map(|p| p.2).sum();
                assert!(total >= side * 0.4, "{icon:?} {state} at {side}: only {total:.1} pixels of ink");
                for &(x, y, c) in &ink {
                    let centre = pos2(x as f32 + 0.5, y as f32 + 0.5);
                    assert!(
                        square.expand(1.0).contains(centre) || c < 0.2,
                        "{icon:?} {state} at {side}: ink {c:.2} at ({x}, {y}) outside {square:?}"
                    );
                }
                let (mx, my) = ink.iter().fold((0.0, 0.0), |(ax, ay), &(x, y, c)| (ax + (x as f32 + 0.5) * c, ay + (y as f32 + 0.5) * c));
                let off = pos2(mx / total, my / total) - square.center();
                assert!(
                    off.x.abs() <= 0.15 * side && off.y.abs() <= 0.15 * side,
                    "{icon:?} {state} at {side}: ink centred at {off:?} from the middle"
                );
            }
        }
    }
}

/// No two glyphs look alike at 16 pt: for each pair at least a fifth of the larger one's ink
/// differs clearly.
#[test]
fn every_glyph_is_distinct_at_16_pt() {
    let side = 16.0;
    let img = solo(side, Color32::WHITE);
    let cells: Vec<Vec<u8>> = (0..AlbumIcon::ALL.len())
        .map(|i| {
            let sq = solo_square(i, side).expand(1.0);
            (sq.min.y as u32..sq.max.y as u32)
                .flat_map(|y| (sq.min.x as u32..sq.max.x as u32).map(move |x| (x, y)))
                .map(|(x, y)| img.pixel(x, y).unwrap()[0])
                .collect()
        })
        .collect();
    let inked = |c: &[u8]| c.iter().filter(|&&v| v > 96).count();
    let mut closest = (f32::MAX, 0, 0);
    for a in 0..cells.len() {
        for b in a + 1..cells.len() {
            let differ = cells[a].iter().zip(&cells[b]).filter(|(p, q)| p.abs_diff(**q) > 96).count();
            let share = differ as f32 / inked(&cells[a]).max(inked(&cells[b])).max(1) as f32;
            assert!(share >= 0.2, "{:?} and {:?} differ in only {:.0} % of their ink", AlbumIcon::ALL[a], AlbumIcon::ALL[b], share * 100.0);
            if share < closest.0 {
                closest = (share, a, b);
            }
        }
    }
    println!(
        "closest pair at 16 pt: {:?} and {:?}, {:.0} % of their ink differs",
        AlbumIcon::ALL[closest.1],
        AlbumIcon::ALL[closest.2],
        closest.0 * 100.0
    );
}

/// The automatic albums read apart from the shell's glyphs they stand beside at 16 pt (the user
/// albums' book in the same group, folder rows below it, the search field's magnifier): at least
/// a fifth of the larger one's ink differs clearly from each.
#[test]
fn the_glyphs_differ_from_their_neighbours_in_the_shell() {
    let side = 16.0;
    let cell = |paint: &dyn Fn(&egui::Painter, Rect)| -> Vec<u8> {
        let img = dalica_ui_kit::snapshot(40, 40, 1.0, &Tokens { chrome: BG, ..Tokens::default() }, |ui| {
            paint(ui.painter(), Rect::from_min_size(pos2(AIR, AIR), vec2(side, side)));
        })
        .unwrap();
        img.rgba.as_chunks::<4>().0.iter().map(|px| px[0]).collect()
    };
    let inked = |c: &[u8]| c.iter().filter(|&&v| v > 96).count();
    for ours in AlbumIcon::ALL {
        let a = cell(&|p, r| paint_album(p, r, ours, Color32::WHITE));
        for theirs in [Icon::Album, Icon::NewAlbum, Icon::AddToAlbum, Icon::Folder, Icon::Search, Icon::Star] {
            let b = cell(&|p, r| icons::paint(p, r, theirs, Color32::WHITE));
            let differ = a.iter().zip(&b).filter(|(p, q)| p.abs_diff(**q) > 96).count();
            let share = differ as f32 / inked(&a).max(inked(&b)).max(1) as f32;
            assert!(share >= 0.2, "{ours:?} and {theirs:?} differ in only {:.0} % of their ink", share * 100.0);
            println!("{ours:?} vs {theirs:?}: {:.0} %", share * 100.0);
        }
    }
}

/// Glyphs take the caller's colour: painted in pure red, no pixel gains green or blue.
#[test]
fn glyphs_take_the_callers_colour() {
    let red = solo(20.0, Color32::from_rgb(255, 0, 0));
    let mut inked = 0;
    for px in red.rgba.as_chunks::<4>().0 {
        assert_eq!((px[1], px[2]), (0, 0), "{px:?}");
        inked += usize::from(px[0] > 0);
    }
    assert!(inked > 300, "{inked} red pixels");
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
            for icon in AlbumIcon::ALL {
                paint_album(ui.painter(), rect, icon, Color32::WHITE);
            }
        }
    })
    .unwrap();
    assert!(img.rgba.as_chunks::<4>().0.iter().all(|px| *px == [0, 0, 0, 255]), "something was painted");
}
