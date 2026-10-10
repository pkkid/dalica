//! The icon sheet (M3.2): every shell icon at 12, 16, 18, 20 and 28 pt, rendered by the CPU
//! rasterizer. The same frame must give identical pixels twice and match the stored fingerprint
//! (`fingerprints/icons.json`, `architecture.md` §11). The render is always written to
//! `target/test-output/ui-kit/icons.png`; with `DALICA_BLESS=1` a 3× render (`icons@3x.png`) is
//! written next to it to judge the drawings, and a new fingerprint is stored (commit it only after
//! looking at both PNGs).

use std::path::PathBuf;
use std::time::Instant;

use dalica_scenes::fingerprint;
use dalica_ui_kit::icons::{self, paint_activity};
use dalica_ui_kit::{Icon, Image, Tokens, fonts};
use egui::{Align2, Color32, Rect, pos2, vec2};

const SIZES: [f32; 5] = [12.0, 16.0, 18.0, 20.0, 28.0];
const ROWS: usize = 20;
const NAME_W: f32 = 104.0;
const CELL_W: f32 = 40.0;
const ROW_H: f32 = 36.0;
const TOP: f32 = 34.0;
const HALF_W: f32 = NAME_W + CELL_W * SIZES.len() as f32;
const GAP: f32 = 24.0;
const MARGIN: f32 = 12.0;
const SIZE: (u32, u32) = ((2.0 * MARGIN + 2.0 * HALF_W + GAP) as u32, (TOP + ROWS as f32 * ROW_H + 8.0) as u32);

fn out_dir() -> PathBuf {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui-kit");
    std::fs::create_dir_all(&out).unwrap();
    out
}

/// The square, in points, where the sheet draws `icon` at size column `col`.
fn sheet_cell(index: usize, col: usize, side: f32) -> Rect {
    let (half, row) = (index / ROWS, index % ROWS);
    let x = MARGIN + half as f32 * (HALF_W + GAP) + NAME_W + col as f32 * CELL_W + CELL_W / 2.0;
    let y = TOP + row as f32 * ROW_H + ROW_H / 2.0;
    Rect::from_center_size(pos2(x, y), vec2(side, side))
}

fn sheet(ui: &mut egui::Ui) {
    let t = Tokens::default();
    let p = ui.painter().clone();
    for half in 0..2 {
        let x0 = MARGIN + half as f32 * (HALF_W + GAP);
        p.text(pos2(x0, 10.0), Align2::LEFT_TOP, "Icon", fonts::semibold(t.small), t.text_label);
        for (col, side) in SIZES.iter().enumerate() {
            let cx = x0 + NAME_W + col as f32 * CELL_W + CELL_W / 2.0;
            p.text(pos2(cx, 10.0), Align2::CENTER_TOP, format!("{side}"), fonts::semibold(t.small), t.text_label);
        }
    }
    for (i, icon) in Icon::ALL.into_iter().enumerate() {
        let (half, row) = (i / ROWS, i % ROWS);
        let x0 = MARGIN + half as f32 * (HALF_W + GAP);
        let cy = TOP + row as f32 * ROW_H + ROW_H / 2.0;
        p.text(pos2(x0, cy), Align2::LEFT_CENTER, icon.name(), fonts::regular(t.small), t.text_dim);
        for (col, &side) in SIZES.iter().enumerate() {
            icons::paint(&p, sheet_cell(i, col, side), icon, t.icon);
        }
    }
}

fn render(pixels_per_point: f32) -> Image {
    let (w, h) = ((SIZE.0 as f32 * pixels_per_point) as u32, (SIZE.1 as f32 * pixels_per_point) as u32);
    dalica_ui_kit::snapshot(w, h, pixels_per_point, &Tokens::default(), sheet).unwrap()
}

#[test]
fn the_icon_sheet_renders_identically_twice_and_matches_its_fingerprint() {
    let started = Instant::now();
    let a = render(1.0);
    let first = started.elapsed();
    let b = render(1.0);
    assert_eq!((a.width, a.height), SIZE);
    assert!(a.rgba == b.rgba, "two renders of the same frame differ");

    let out = out_dir();
    std::fs::write(out.join("icons.png"), fingerprint::png_rgba(a.width, a.height, &a.rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fingerprints/icons.json");
    if std::env::var_os("DALICA_BLESS").is_some() {
        let big = render(3.0);
        std::fs::write(out.join("icons@3x.png"), fingerprint::png_rgba(big.width, big.height, &big.rgba)).unwrap();
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(a.width, a.height, &a.rgba)) + "\n").unwrap();
        println!("blessed {}", stored.display());
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(a.width, a.height, &a.rgba, &expected, 2.0) {
        std::fs::write(out.join("icons-actual.png"), &m.actual_png).unwrap();
        panic!("{} (render in {})", m.message, out.display());
    }
    println!("icon sheet {} × {}: {} icons, first render {:.0} ms", a.width, a.height, Icon::ALL.len(), first.as_secs_f64() * 1000.0);
}

/// One icon per cell on a black canvas, each in a `side` square with 12 px of air around it, so
/// the ink of every icon can be measured on its own.
struct Solo {
    img: Image,
    side: f32,
    pitch: f32,
    cols: usize,
}

const BG: Color32 = Color32::BLACK;

fn solo(side: f32, color: Color32, draw: impl Fn(&egui::Painter, Rect, Icon, Color32)) -> Solo {
    let pitch = side + 24.0;
    let cols = 10usize;
    let rows = Icon::ALL.len().div_ceil(cols);
    let (w, h) = ((pitch * cols as f32) as u32, (pitch * rows as f32) as u32);
    let tokens = Tokens { chrome: BG, ..Tokens::default() };
    let img = dalica_ui_kit::snapshot(w, h, 1.0, &tokens, |ui| {
        let p = ui.painter().clone();
        for (i, icon) in Icon::ALL.into_iter().enumerate() {
            draw(&p, solo_square(i, side, pitch, cols), icon, color);
        }
    })
    .unwrap();
    Solo { img, side, pitch, cols }
}

fn solo_square(i: usize, side: f32, pitch: f32, cols: usize) -> Rect {
    let min = pos2((i % cols) as f32 * pitch + 12.0, (i / cols) as f32 * pitch + 12.0);
    Rect::from_min_size(min, vec2(side, side))
}

impl Solo {
    /// The inked pixels of cell `i` (anything not the background), with their coverage 0 … 1.
    fn ink(&self, i: usize) -> Vec<(u32, u32, f32)> {
        let cell = solo_square(i, self.side, self.pitch, self.cols).expand(12.0);
        let mut ink = Vec::new();
        for y in cell.min.y as u32..cell.max.y as u32 {
            for x in cell.min.x as u32..cell.max.x as u32 {
                let px = self.img.pixel(x, y).unwrap();
                let cover = px[0].max(px[1]).max(px[2]) as f32 / 255.0;
                if cover > 0.0 {
                    ink.push((x, y, cover));
                }
            }
        }
        ink
    }
}

/// Every icon draws something at every size, stays inside its square (half a pixel of
/// anti-aliasing allowed) and is visually centred: its ink's centre of mass lies within 15 % of the
/// side from the square's centre.
#[test]
fn every_icon_inks_its_square_only_and_is_centred() {
    for side in SIZES {
        let s = solo(side, Color32::WHITE, icons::paint);
        for (i, icon) in Icon::ALL.into_iter().enumerate() {
            let square = solo_square(i, side, s.pitch, s.cols);
            let ink = s.ink(i);
            let total: f32 = ink.iter().map(|p| p.2).sum();
            assert!(total >= side * 0.4, "{icon:?} at {side}: only {total:.1} pixels of ink");
            let allowed = square.expand(1.0);
            for &(x, y, c) in &ink {
                let centre = pos2(x as f32 + 0.5, y as f32 + 0.5);
                assert!(allowed.contains(centre) || c < 0.2, "{icon:?} at {side}: ink {c:.2} at ({x}, {y}) outside {square:?}");
            }
            let (mx, my) = ink.iter().fold((0.0, 0.0), |(ax, ay), &(x, y, c)| (ax + (x as f32 + 0.5) * c, ay + (y as f32 + 0.5) * c));
            let mass = pos2(mx / total, my / total);
            let off = mass - square.center();
            assert!(off.x.abs() <= 0.15 * side && off.y.abs() <= 0.15 * side, "{icon:?} at {side}: ink centred at {off:?} from the middle");
        }
    }
}

/// No two icons look alike at 16 pt (the tree and menu size): for each pair, at least a fifth of
/// the larger one's inked pixels differ clearly (the closest pair, clear-search and clear-tray,
/// shares its circle).
#[test]
fn every_icon_is_distinct_at_16_pt() {
    let side = 16.0;
    let s = solo(side, Color32::WHITE, icons::paint);
    let cells: Vec<Vec<u8>> = (0..Icon::ALL.len())
        .map(|i| {
            let sq = solo_square(i, side, s.pitch, s.cols).expand(1.0);
            (sq.min.y as u32..sq.max.y as u32)
                .flat_map(|y| (sq.min.x as u32..sq.max.x as u32).map(move |x| (x, y)))
                .map(|(x, y)| s.img.pixel(x, y).unwrap()[0])
                .collect()
        })
        .collect();
    let inked = |c: &[u8]| c.iter().filter(|&&v| v > 96).count();
    let mut closest = (f32::MAX, 0, 0);
    for a in 0..cells.len() {
        for b in a + 1..cells.len() {
            let differ = cells[a].iter().zip(&cells[b]).filter(|(p, q)| p.abs_diff(**q) > 96).count();
            let share = differ as f32 / inked(&cells[a]).max(inked(&cells[b])).max(1) as f32;
            assert!(share >= 0.2, "{:?} and {:?} differ in only {:.0} % of their ink", Icon::ALL[a], Icon::ALL[b], share * 100.0);
            if share < closest.0 {
                closest = (share, a, b);
            }
        }
    }
    println!("closest pair at 16 pt: {:?} and {:?}, {:.0} % of their ink differs", Icon::ALL[closest.1], Icon::ALL[closest.2], closest.0 * 100.0);
}

/// Icons are tinted by the caller: painted in pure red, no pixel gains green or blue.
#[test]
fn icons_take_the_callers_colour() {
    let s = solo(20.0, Color32::from_rgb(255, 0, 0), icons::paint);
    let mut red = 0;
    for px in s.img.rgba.as_chunks::<4>().0 {
        assert_eq!((px[1], px[2]), (0, 0), "{px:?}");
        red += usize::from(px[0] > 0);
    }
    assert!(red > 1000, "{red} red pixels");
}

/// The activity spinner steps through eight frames per turn; phase 0 is the static icon, a whole
/// turn comes back to it, and a non-finite phase draws phase 0.
#[test]
fn the_activity_spinner_steps_eight_times_per_turn() {
    let frame = |phase: f32| {
        dalica_ui_kit::snapshot(40, 40, 1.0, &Tokens { chrome: BG, ..Tokens::default() }, |ui| {
            paint_activity(ui.painter(), Rect::from_min_size(pos2(10.0, 10.0), vec2(20.0, 20.0)), Color32::WHITE, phase);
        })
        .unwrap()
        .rgba
    };
    let still = dalica_ui_kit::snapshot(40, 40, 1.0, &Tokens { chrome: BG, ..Tokens::default() }, |ui| {
        icons::paint(ui.painter(), Rect::from_min_size(pos2(10.0, 10.0), vec2(20.0, 20.0)), Icon::Activity, Color32::WHITE);
    })
    .unwrap()
    .rgba;
    let frames: Vec<_> = (0..8).map(|i| frame(i as f32 / 8.0 + 0.01)).collect();
    assert!(frames[0] == still);
    for a in 0..8 {
        for b in a + 1..8 {
            assert!(frames[a] != frames[b], "frames {a} and {b} are the same");
        }
    }
    assert!(frame(1.0) == still);
    assert!(frame(-0.99) == frames[0]);
    assert!(frame(f32::NAN) == still);
    assert!(frame(f32::INFINITY) == still);
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
            for icon in Icon::ALL {
                icons::paint(ui.painter(), rect, icon, Color32::WHITE);
            }
            paint_activity(ui.painter(), rect, Color32::WHITE, 0.3);
        }
    })
    .unwrap();
    assert!(img.rgba.as_chunks::<4>().0.iter().all(|px| *px == [0, 0, 0, 255]), "something was painted");
}
