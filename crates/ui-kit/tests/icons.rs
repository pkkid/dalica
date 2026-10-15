//! Every family of glyphs drawn in code (`design-system.md` §5), table-driven:
//!
//! - [`Icon`], the shell icons (M3.2), at 12, 16, 18, 20 and 28 pt;
//! - [`FolderIcon`], the Folder Manager's states and the tree roots (M5.18, `organizing.md` §2), at
//!   the same sizes, each state also at the inherited 50 % opacity in its own colour;
//! - [`EditIcon`], the edit view's glyphs (M7.6, `editing.md` §2, §5–§7), [`OutputIcon`], Save's
//!   disk and the tree's project (M8.5, `creating.md` §2), and [`AlbumIcon`], the automatic albums'
//!   books (M9.5, `organizing.md` §4.1), at 12, 16, 20 and 28 pt in the rest (`icon`), hover
//!   (`text`) and disabled (`text_disabled`) colours, at 1× and 2×.
//!
//! Each family's sheet renders identically twice and matches its fingerprint
//! (`fingerprints/icons.json`, `icons-folder-modes.json`, `icons-edit[@2x].json`,
//! `icons-output[@2x].json`, `icons-albums[@2x].json`; renders in `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints, with a 3× render of the shell and folder sheets to
//! judge the drawings: commit them only after looking at the PNGs). Every glyph inks its square
//! only and is centred, no two of a family look alike at 16 pt, glyphs take the caller's colour,
//! and degenerate rectangles paint nothing. Then what is particular to a family: the activity
//! spinner's eight frames, the inherited folder states' half opacity, the filmstrip arrows'
//! mirror, and the album and output glyphs against the shell icons they stand beside.

use std::fmt::Debug;
use std::path::PathBuf;

use crate::common::out_dir;
use dalica_scenes::fingerprint;
use dalica_ui_kit::icons::{
    self, AlbumIcon, EditIcon, FolderIcon, INHERITED_OPACITY, OutputIcon, paint_activity, paint_album, paint_edit, paint_folder, paint_output,
    state_color,
};
use dalica_ui_kit::{Icon, Image, Tokens, fonts};
use egui::{Align2, Color32, Painter, Rect, pos2, vec2};

const BG: Color32 = Color32::BLACK;
/// Air round each glyph when it is drawn alone.
const AIR: f32 = 12.0;
/// Glyphs per row when drawn alone.
const SOLO_COLS: usize = 10;

/// How a glyph paints: in a square, in a colour.
type Paint = Box<dyn Fn(&Painter, Rect, Color32)>;
/// A sheet: its size in points and what it draws.
type Sheet = ((u32, u32), Box<dyn Fn(&mut egui::Ui)>);

/// One glyph of a family: its debug name, its label on the sheet, and how it paints.
struct Glyph {
    debug: String,
    name: &'static str,
    paint: Paint,
}

/// A family of glyphs and what its checks need.
struct Family {
    /// The sheet's name: its fingerprint and render.
    name: &'static str,
    glyphs: Vec<Glyph>,
    /// The colours the ink checks run in (dim ones measured against their brightest channel).
    colors: fn(&Tokens) -> Vec<(&'static str, Color32)>,
    /// The fewest red pixels the family paints at 20 pt in pure red.
    min_red: usize,
}

fn glyphs<I: Copy + Debug + 'static>(all: &[I], name: fn(I) -> &'static str, paint: fn(&Painter, Rect, I, Color32)) -> Vec<Glyph> {
    all.iter().map(|&i| Glyph { debug: format!("{i:?}"), name: name(i), paint: Box::new(move |p, r, c| paint(p, r, i, c)) }).collect()
}

fn white(_: &Tokens) -> Vec<(&'static str, Color32)> {
    vec![("white", Color32::WHITE)]
}

/// The three states the state sheets show, with the token each paints in.
fn states(t: &Tokens) -> Vec<(&'static str, Color32)> {
    vec![("rest", t.icon), ("hover", t.text), ("disabled", t.text_disabled)]
}

fn shell() -> Family {
    Family { name: "icons", glyphs: glyphs(&Icon::ALL, Icon::name, icons::paint), colors: white, min_red: 1000 }
}

fn folders() -> Family {
    Family { name: "icons-folder-modes", glyphs: glyphs(&FolderIcon::ALL, FolderIcon::name, paint_folder), colors: white, min_red: 0 }
}

fn edit() -> Family {
    Family { name: "icons-edit", glyphs: glyphs(&EditIcon::ALL, EditIcon::name, paint_edit), colors: states, min_red: 1000 }
}

fn output() -> Family {
    Family { name: "icons-output", glyphs: glyphs(&OutputIcon::ALL, OutputIcon::name, paint_output), colors: states, min_red: 300 }
}

fn albums() -> Family {
    Family { name: "icons-albums", glyphs: glyphs(&AlbumIcon::ALL, AlbumIcon::name, paint_album), colors: states, min_red: 300 }
}

fn families() -> [Family; 5] {
    [shell(), folders(), edit(), output(), albums()]
}

// The sheets.

/// Renders `draw` on a `size` (points) sheet at `pixels_per_point`.
fn render(size: (u32, u32), pixels_per_point: f32, draw: &dyn Fn(&mut egui::Ui)) -> Image {
    let (w, h) = ((size.0 as f32 * pixels_per_point) as u32, (size.1 as f32 * pixels_per_point) as u32);
    let img = dalica_ui_kit::snapshot(w, h, pixels_per_point, &Tokens::default(), draw).unwrap();
    assert_eq!((img.width, img.height), (w, h));
    img
}

/// The sheet renders identically twice and matches `fingerprints/<name>.json`; with
/// `DALICA_BLESS=1` a 3× render goes next to it when `big` is set.
fn check_sheet(name: &str, size: (u32, u32), pixels_per_point: f32, big: bool, draw: &dyn Fn(&mut egui::Ui)) {
    let a = render(size, pixels_per_point, draw);
    let b = render(size, pixels_per_point, draw);
    assert!(a.rgba == b.rgba, "{name}: two renders of the same frame differ");
    let out = out_dir();
    std::fs::write(out.join(format!("{name}.png")), fingerprint::png_rgba(a.width, a.height, &a.rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        if big {
            let big = render(size, 3.0, draw);
            std::fs::write(out.join(format!("{name}@3x.png")), fingerprint::png_rgba(big.width, big.height, &big.rgba)).unwrap();
        }
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(a.width, a.height, &a.rgba)) + "\n").unwrap();
        println!("blessed {}", stored.display());
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(a.width, a.height, &a.rgba, &expected, 2.0) {
        std::fs::write(out.join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {} (render in {})", m.message, out.display());
    }
}

/// The shell icons in two halves of 20 rows, each at five sizes.
fn shell_sheet() -> Sheet {
    const SIZES: [f32; 5] = [12.0, 16.0, 18.0, 20.0, 28.0];
    const ROWS: usize = 20;
    const NAME_W: f32 = 104.0;
    const CELL_W: f32 = 40.0;
    const ROW_H: f32 = 36.0;
    const TOP: f32 = 34.0;
    const HALF_W: f32 = NAME_W + CELL_W * SIZES.len() as f32;
    const GAP: f32 = 24.0;
    const MARGIN: f32 = 12.0;
    let size = ((2.0 * MARGIN + 2.0 * HALF_W + GAP) as u32, (TOP + ROWS as f32 * ROW_H + 8.0) as u32);
    let draw = |ui: &mut egui::Ui| {
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
                let cx = x0 + NAME_W + col as f32 * CELL_W + CELL_W / 2.0;
                icons::paint(&p, Rect::from_center_size(pos2(cx, cy), vec2(side, side)), icon, t.icon);
            }
        }
    };
    (size, Box::new(draw))
}

/// The folder states in their colours, then the same inherited, then the root icons, at five sizes.
fn folder_sheet() -> Sheet {
    const SIZES: [f32; 5] = [12.0, 16.0, 18.0, 20.0, 28.0];
    const NAME_W: f32 = 120.0;
    const CELL_W: f32 = 40.0;
    const ROW_H: f32 = 36.0;
    const TOP: f32 = 34.0;
    const MARGIN: f32 = 12.0;
    const ROWS: usize = 3 + 3 + 6;
    let size = ((2.0 * MARGIN + NAME_W + CELL_W * SIZES.len() as f32) as u32, (TOP + ROWS as f32 * ROW_H + 8.0) as u32);
    let draw = |ui: &mut egui::Ui| {
        let t = Tokens::default();
        let states = [(FolderIcon::ScanAlways, t.accent), (FolderIcon::ScanOnce, t.text_label), (FolderIcon::Remove, t.danger)];
        let mut rows: Vec<_> = states.iter().map(|&(i, c)| (i, state_color(c, false), i.name().to_string())).collect();
        rows.extend(states.iter().map(|&(i, c)| (i, state_color(c, true), format!("{} (inherited)", i.name()))));
        rows.extend(FolderIcon::ALL[3..].iter().map(|&i| (i, t.icon, i.name().to_string())));
        assert_eq!(rows.len(), ROWS);
        let p = ui.painter().clone();
        p.text(pos2(MARGIN, 10.0), Align2::LEFT_TOP, "Glyph", fonts::semibold(t.small), t.text_label);
        for (col, side) in SIZES.iter().enumerate() {
            let cx = MARGIN + NAME_W + col as f32 * CELL_W + CELL_W / 2.0;
            p.text(pos2(cx, 10.0), Align2::CENTER_TOP, format!("{side}"), fonts::semibold(t.small), t.text_label);
        }
        for (row, (icon, color, label)) in rows.into_iter().enumerate() {
            let cy = TOP + row as f32 * ROW_H + ROW_H / 2.0;
            p.text(pos2(MARGIN, cy), Align2::LEFT_CENTER, label, fonts::regular(t.small), t.text_dim);
            for (col, &side) in SIZES.iter().enumerate() {
                let cx = MARGIN + NAME_W + col as f32 * CELL_W + CELL_W / 2.0;
                paint_folder(&p, Rect::from_center_size(pos2(cx, cy), vec2(side, side)), icon, color);
            }
        }
    };
    (size, Box::new(draw))
}

/// A family in three state groups (rest, hover, disabled) of four sizes, its names `name_w` wide.
fn state_sheet(family: Family, name_w: f32) -> Sheet {
    const SIZES: [f32; 4] = [12.0, 16.0, 20.0, 28.0];
    const CELL_W: f32 = 38.0;
    const GROUP_GAP: f32 = 14.0;
    const ROW_H: f32 = 36.0;
    const TOP: f32 = 46.0;
    const MARGIN: f32 = 12.0;
    const GROUP_W: f32 = CELL_W * SIZES.len() as f32;
    let rows = family.glyphs.len();
    let size = ((2.0 * MARGIN + name_w + 3.0 * GROUP_W + 2.0 * GROUP_GAP) as u32, (TOP + rows as f32 * ROW_H + 8.0) as u32);
    let draw = move |ui: &mut egui::Ui| {
        let t = Tokens::default();
        let p = ui.painter().clone();
        p.text(pos2(MARGIN, 10.0), Align2::LEFT_TOP, "Glyph", fonts::semibold(t.small), t.text_label);
        for (g, (state, color)) in states(&t).into_iter().enumerate() {
            let x0 = MARGIN + name_w + g as f32 * (GROUP_W + GROUP_GAP);
            p.text(pos2(x0 + GROUP_W / 2.0, 6.0), Align2::CENTER_TOP, state, fonts::semibold(t.small), t.text_label);
            for (col, side) in SIZES.iter().enumerate() {
                let cx = x0 + col as f32 * CELL_W + CELL_W / 2.0;
                p.text(pos2(cx, 24.0), Align2::CENTER_TOP, format!("{side}"), fonts::regular(t.small), t.text_dim);
            }
            for (row, glyph) in family.glyphs.iter().enumerate() {
                let cy = TOP + row as f32 * ROW_H + ROW_H / 2.0;
                if g == 0 {
                    p.text(pos2(MARGIN, cy), Align2::LEFT_CENTER, glyph.name, fonts::regular(t.small), t.text_dim);
                }
                for (col, &side) in SIZES.iter().enumerate() {
                    let cx = x0 + col as f32 * CELL_W + CELL_W / 2.0;
                    (glyph.paint)(&p, Rect::from_center_size(pos2(cx, cy), vec2(side, side)), color);
                }
            }
        }
    };
    (size, Box::new(draw))
}

#[test]
fn every_sheet_renders_identically_twice_and_matches_its_fingerprint() {
    let (size, draw) = shell_sheet();
    check_sheet("icons", size, 1.0, true, &*draw);
    let (size, draw) = folder_sheet();
    check_sheet("icons-folder-modes", size, 1.0, true, &*draw);
    for (family, name_w) in [(edit(), 124.0), (output(), 72.0), (albums(), 92.0)] {
        let name = family.name;
        let (size, draw) = state_sheet(family, name_w);
        check_sheet(name, size, 1.0, false, &*draw);
        check_sheet(&format!("{name}@2x"), size, 2.0, false, &*draw);
    }
}

// Each glyph alone.

fn solo_square(i: usize, side: f32) -> Rect {
    let pitch = side + 2.0 * AIR;
    Rect::from_min_size(pos2((i % SOLO_COLS) as f32 * pitch + AIR, (i / SOLO_COLS) as f32 * pitch + AIR), vec2(side, side))
}

/// Each glyph of `family` alone on black in a `side` square with 12 px of air round it, in
/// `color`.
fn solo(family: &Family, side: f32, color: Color32) -> Image {
    let pitch = side + 2.0 * AIR;
    let (cols, rows) = (family.glyphs.len().min(SOLO_COLS), family.glyphs.len().div_ceil(SOLO_COLS));
    let (w, h) = ((pitch * cols as f32) as u32, (pitch * rows as f32) as u32);
    dalica_ui_kit::snapshot(w, h, 1.0, &Tokens { chrome: BG, ..Tokens::default() }, |ui| {
        for (i, glyph) in family.glyphs.iter().enumerate() {
            (glyph.paint)(ui.painter(), solo_square(i, side), color);
        }
    })
    .unwrap()
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

/// Every glyph draws something at every size and in every colour its family is checked in, stays
/// inside its square (half a pixel of anti-aliasing allowed) and is visually centred: its ink's
/// centre of mass lies within 15 % of the side from the square's centre.
#[test]
fn every_glyph_inks_its_square_only_and_is_centred() {
    let t = Tokens::default();
    for family in families() {
        for (state, color) in (family.colors)(&t) {
            for side in [12.0, 16.0, 18.0, 20.0, 28.0] {
                let img = solo(&family, side, color);
                for (i, glyph) in family.glyphs.iter().enumerate() {
                    let case = format!("{} {state} at {side}", glyph.debug);
                    let square = solo_square(i, side);
                    let ink = ink(&img, i, side, color);
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
}

/// How much of the larger glyph's ink differs clearly between two cells (red channel, 0 … 255).
fn share_differing(a: &[u8], b: &[u8]) -> f32 {
    let inked = |c: &[u8]| c.iter().filter(|&&v| v > 96).count();
    let differ = a.iter().zip(b).filter(|(p, q)| p.abs_diff(**q) > 96).count();
    differ as f32 / inked(a).max(inked(b)).max(1) as f32
}

/// No two glyphs of a family look alike at 16 pt (the tree and menu size): for each pair at least
/// a fifth of the larger one's ink differs clearly.
#[test]
fn every_glyph_is_distinct_at_16_pt() {
    let side = 16.0;
    for family in families() {
        let img = solo(&family, side, Color32::WHITE);
        let cells: Vec<Vec<u8>> = (0..family.glyphs.len())
            .map(|i| {
                let sq = solo_square(i, side).expand(1.0);
                (sq.min.y as u32..sq.max.y as u32)
                    .flat_map(|y| (sq.min.x as u32..sq.max.x as u32).map(move |x| (x, y)))
                    .map(|(x, y)| img.pixel(x, y).unwrap()[0])
                    .collect()
            })
            .collect();
        let mut closest = (f32::MAX, 0, 0);
        for a in 0..cells.len() {
            for b in a + 1..cells.len() {
                let share = share_differing(&cells[a], &cells[b]);
                let (ga, gb) = (&family.glyphs[a].debug, &family.glyphs[b].debug);
                assert!(share >= 0.2, "{ga} and {gb} differ in only {:.0} % of their ink", share * 100.0);
                if share < closest.0 {
                    closest = (share, a, b);
                }
            }
        }
        if closest.0 < f32::MAX {
            let (a, b) = (&family.glyphs[closest.1].debug, &family.glyphs[closest.2].debug);
            println!("{}: closest pair at 16 pt: {a} and {b}, {:.0} % of their ink differs", family.name, closest.0 * 100.0);
        }
    }
}

/// Glyphs take the caller's colour: painted in pure red, no pixel gains green or blue.
#[test]
fn glyphs_take_the_callers_colour() {
    for family in families() {
        let red = solo(&family, 20.0, Color32::from_rgb(255, 0, 0));
        let mut inked = 0;
        for px in red.rgba.as_chunks::<4>().0 {
            assert_eq!((px[1], px[2]), (0, 0), "{}: {px:?}", family.name);
            inked += usize::from(px[0] > 0);
        }
        assert!(inked > family.min_red, "{}: {inked} red pixels", family.name);
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
    let all = families();
    let img = dalica_ui_kit::snapshot(32, 32, 1.0, &Tokens { chrome: BG, ..Tokens::default() }, |ui| {
        for rect in bad {
            for glyph in all.iter().flat_map(|f| &f.glyphs) {
                (glyph.paint)(ui.painter(), rect, Color32::WHITE);
            }
            paint_activity(ui.painter(), rect, Color32::WHITE, 0.3);
        }
    })
    .unwrap();
    assert!(img.rgba.as_chunks::<4>().0.iter().all(|px| *px == [0, 0, 0, 255]), "something was painted");
}

// What is particular to a family.

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

/// An inherited folder state is drawn at half its opacity: over black its ink is half as bright.
#[test]
fn inherited_folder_states_are_half_as_opaque() {
    let family = folders();
    let full = solo(&family, 20.0, Color32::WHITE);
    let half = solo(&family, 20.0, state_color(Color32::WHITE, true));
    for (i, glyph) in family.glyphs.iter().enumerate() {
        let sum = |img: &Image| ink(img, i, 20.0, Color32::WHITE).iter().map(|p| p.2).sum::<f32>();
        let ratio = sum(&half) / sum(&full);
        assert!((ratio - INHERITED_OPACITY).abs() < 0.12, "{}: inherited ink is {ratio:.2} of the full ink", glyph.debug);
    }
}

/// The filmstrip arrows are mirror images of each other.
#[test]
fn the_filmstrip_arrows_mirror_each_other() {
    let side = 20.0;
    let img = solo(&edit(), side, Color32::WHITE);
    let index = |icon| EditIcon::ALL.iter().position(|&i| i == icon).unwrap();
    let (prev, next) = (solo_square(index(EditIcon::FilmstripPrevious), side), solo_square(index(EditIcon::FilmstripNext), side));
    let mut differ = 0;
    for y in 0..side as u32 {
        for x in 0..side as u32 {
            let a = img.pixel(prev.min.x as u32 + x, prev.min.y as u32 + y).unwrap()[0];
            let b = img.pixel(next.max.x as u32 - 1 - x, next.min.y as u32 + y).unwrap()[0];
            differ += usize::from(a.abs_diff(b) > 64);
        }
    }
    assert!(differ <= 4, "{differ} pixels differ between the previous arrow and the mirrored next arrow");
}

/// The album and output glyphs read apart from the shell icons they stand beside at 16 pt (the
/// automatic albums by the user albums' book, folder rows and the search field's magnifier; a
/// project row under folder rows, Save next to Export in the folder header): at least a fifth of
/// the larger one's ink differs clearly from each.
#[test]
fn album_and_output_glyphs_differ_from_their_neighbours_in_the_shell() {
    let side = 16.0;
    let cell = |paint: &dyn Fn(&Painter, Rect)| -> Vec<u8> {
        let img = dalica_ui_kit::snapshot(40, 40, 1.0, &Tokens { chrome: BG, ..Tokens::default() }, |ui| {
            paint(ui.painter(), Rect::from_min_size(pos2(AIR, AIR), vec2(side, side)));
        })
        .unwrap();
        img.rgba.as_chunks::<4>().0.iter().map(|px| px[0]).collect()
    };
    let neighbours = [
        (albums(), [Icon::Album, Icon::NewAlbum, Icon::AddToAlbum, Icon::Folder, Icon::Search, Icon::Star]),
        (output(), [Icon::Folder, Icon::Export, Icon::Album, Icon::Collage, Icon::Import, Icon::Properties]),
    ];
    for (family, theirs) in neighbours {
        for ours in &family.glyphs {
            let a = cell(&|p, r| (ours.paint)(p, r, Color32::WHITE));
            for theirs in theirs {
                let b = cell(&|p, r| icons::paint(p, r, theirs, Color32::WHITE));
                let share = share_differing(&a, &b);
                assert!(share >= 0.2, "{} and {theirs:?} differ in only {:.0} % of their ink", ours.debug, share * 100.0);
            }
        }
    }
}
