//! The thumbnail badge sheet (M4.10, `design-system.md` §7): every combination of the badge row
//! (edited, geotagged, held, starred), as a photo and as a video with its sprocket strip and
//! duration pill, over a letterboxed photo with its drop shadow, at three cell sizes (64, 144 and
//! 320, the thumbnail slider's ends and its default). Landscape and portrait photos alternate, so
//! the narrow portrait cells show which badges are left out when the row does not fit.
//!
//! Each sheet must render identically twice and match `fingerprints/badges-<size>.json`
//! (`architecture.md` §11); the renders are written to `target/test-output/ui-kit/badges-<size>.png`
//! and `DALICA_BLESS=1` stores new fingerprints (look at the PNGs first). The geometry the painter
//! reports is checked against the rules for every tile.

use std::path::PathBuf;

use dalica_scenes::fingerprint;
use dalica_ui_kit::widgets::{BADGE_GAP, BADGE_INSET, BADGE_PLATE, Badge, BadgeRects, Badges, badges, photo_shadow};
use dalica_ui_kit::{Image, Tokens};
use egui::{Color32, Mesh, Painter, Rect, pos2, vec2};

const SIZES: [f32; 3] = [64.0, 144.0, 320.0];
const COLUMNS: usize = 8;
const PAD: f32 = 8.0;
const SECONDS: u32 = 83;

/// The letterboxed photo of tile `i` in `cell`: landscape 3:2 on even tiles, portrait 2:3 on odd.
fn image_rect(cell: Rect, i: usize) -> Rect {
    let s = cell.width();
    let size = if i.is_multiple_of(2) { vec2(s, (s * 2.0 / 3.0).round()) } else { vec2((s * 2.0 / 3.0).round(), s) };
    Rect::from_center_size(cell.center(), size)
}

/// A photo-like gradient: a light sky over a darker ground.
fn photo(p: &Painter, r: Rect) {
    let mut mesh = Mesh::default();
    let sky = Color32::from_rgb(196, 214, 232);
    let horizon = Color32::from_rgb(226, 220, 196);
    let ground = Color32::from_rgb(92, 104, 70);
    let mid = r.min.y + r.height() * 0.55;
    for (y0, y1, c0, c1) in [(r.min.y, mid, sky, horizon), (mid, r.max.y, horizon, ground)] {
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(r.min.x, y0), c0);
        mesh.colored_vertex(pos2(r.max.x, y0), c0);
        mesh.colored_vertex(pos2(r.max.x, y1), c1);
        mesh.colored_vertex(pos2(r.min.x, y1), c1);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    p.add(mesh);
}

struct Tile {
    badges: Badges,
    image: Rect,
    rects: BadgeRects,
}

fn render(size: f32) -> (Image, Vec<Tile>) {
    let t = Tokens::default();
    let combos = Badges::combinations(SECONDS);
    let tile = size + 2.0 * PAD;
    let rows = combos.len().div_ceil(COLUMNS);
    let (w, h) = ((COLUMNS as f32 * tile) as u32, (rows as f32 * tile) as u32);
    let mut tiles = Vec::new();
    let img = dalica_ui_kit::snapshot(w, h, 1.0, &t, |ui| {
        let p = ui.painter().clone();
        p.rect_filled(ui.max_rect().expand(1.0), 0, t.grid_bg);
        tiles.clear();
        for (i, b) in combos.iter().enumerate() {
            let (col, row) = ((i % COLUMNS) as f32, (i / COLUMNS) as f32);
            let cell = Rect::from_min_size(pos2(col * tile + PAD, row * tile + PAD), vec2(size, size));
            let image = image_rect(cell, i);
            photo_shadow(&p, image);
            photo(&p, image);
            let rects = badges(&p, &t, image, b);
            tiles.push(Tile { badges: *b, image, rects });
        }
    })
    .unwrap();
    (img, tiles)
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui-kit");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_badge_sheets_render_identically_twice_and_match_their_fingerprints() {
    for size in SIZES {
        let name = format!("badges-{size}");
        let (a, _) = render(size);
        let (b, _) = render(size);
        assert!(a.rgba == b.rgba, "{name}: two renders of the same sheet differ");
        std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(a.width, a.height, &a.rgba)).unwrap();
        let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
        if std::env::var_os("DALICA_BLESS").is_some() {
            std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(a.width, a.height, &a.rgba)) + "\n").unwrap();
            println!("blessed {}", stored.display());
        }
        let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
        if let Err(m) = fingerprint::check(a.width, a.height, &a.rgba, &expected, 2.0) {
            std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
            panic!("{name}: {}", m.message);
        }
    }
}

#[test]
fn every_tile_follows_the_badge_rules() {
    let grid = Tokens::default().grid_bg.r();
    for size in SIZES {
        let (img, tiles) = render(size);
        assert_eq!(tiles.len(), 32);
        for (i, tile) in tiles.iter().enumerate() {
            let at = format!("{size}/{i} {:?}", tile.badges);
            let image = tile.image;
            // The drop shadow darkens the grid just below the photo, not above it.
            let below = img.pixel(image.center().x as u32, image.max.y as u32 + 1).unwrap();
            let above = img.pixel(image.center().x as u32, image.min.y as u32 - 2).unwrap();
            assert!(below[0] < grid && above[0] == grid, "{at}: shadow {below:?}, above {above:?}");
            let wanted: Vec<Badge> = tile.badges.row().collect();
            let fit = ((image.width() - 2.0 * BADGE_INSET + BADGE_GAP) / (BADGE_PLATE + BADGE_GAP)).floor() as usize;
            let drawn: Vec<Badge> = tile.rects.row.iter().map(|(b, _)| *b).collect();
            // As many as fit, the leftmost left out first: the star always stays.
            assert_eq!(drawn.len(), wanted.len().min(fit), "{at}");
            assert_eq!(drawn, wanted[wanted.len() - drawn.len()..], "{at}: the row keeps its order, dropping from the left");
            if tile.badges.starred {
                assert_eq!(drawn.last(), Some(&Badge::Starred), "{at}: the star is rightmost");
            }
            // Plates are 18 × 18, 2 apart, the row right-aligned 4 in from the bottom-right corner.
            for (k, (_, r)) in tile.rects.row.iter().enumerate() {
                assert_eq!(r.size(), vec2(BADGE_PLATE, BADGE_PLATE), "{at}");
                assert!(image.contains_rect(*r), "{at}: inside the image");
                assert_eq!(r.max.y, image.max.y - BADGE_INSET, "{at}");
                if let Some((_, next)) = tile.rects.row.get(k + 1) {
                    assert_eq!(next.min.x - r.max.x, BADGE_GAP, "{at}");
                }
            }
            if let Some((_, last)) = tile.rects.row.last() {
                assert_eq!(last.max.x, image.max.x - BADGE_INSET, "{at}");
            }
            // The star glyph is light on its dark plate.
            if let Some(star) = tile.rects.get(Badge::Starred) {
                let c = star.center();
                let px = img.pixel(c.x as u32, c.y as u32).unwrap();
                assert!(px[0] > 170 && px[1] > 170 && px[2] > 170, "{at}: star pixel {px:?}");
            }
            // Videos: the strip along the left edge, the pill bottom-left clear of the row.
            match tile.badges.video {
                None => assert_eq!((tile.rects.sprockets, tile.rects.duration), (None, None), "{at}"),
                Some(_) => {
                    let strip = tile.rects.sprockets.unwrap_or_else(|| panic!("{at}: a sprocket strip"));
                    assert_eq!((strip.min, strip.height()), (image.min, image.height()), "{at}");
                    if let Some(pill) = tile.rects.duration {
                        assert!(image.contains_rect(pill) && pill.min.x > strip.max.x, "{at}: {pill:?}");
                        assert_eq!(pill.max.y, image.max.y - BADGE_INSET, "{at}");
                        assert!(tile.rects.row.iter().all(|(_, r)| r.min.x >= pill.max.x + BADGE_GAP), "{at}: clear of the row");
                    }
                }
            }
        }
        // Where there is room (144 and up, at most two badges beside it), every video shows its pill.
        if size >= 144.0 {
            let roomy = tiles.iter().filter(|t| t.badges.video.is_some() && t.badges.row().count() <= 2);
            assert!(roomy.clone().count() == 11 && roomy.clone().all(|t| t.rects.duration.is_some()), "{size}");
        }
    }
}

#[test]
fn hostile_and_tiny_rects_paint_nothing() {
    let t = Tokens::default();
    let all = Badges { edited: true, geotagged: true, held: true, starred: true, video: Some(dalica_ui_kit::widgets::Video { seconds: Some(5) }) };
    dalica_ui_kit::snapshot(64, 64, 1.0, &t, |ui| {
        let p = ui.painter().clone();
        for r in [
            Rect::NOTHING,
            Rect::from_min_size(pos2(0.0, 0.0), vec2(f32::NAN, 40.0)),
            Rect::from_min_size(pos2(0.0, 0.0), vec2(f32::INFINITY, 40.0)),
            Rect::from_min_size(pos2(10.0, 10.0), vec2(12.0, 12.0)),
        ] {
            photo_shadow(&p, r);
            assert_eq!(badges(&p, &t, r, &all), BadgeRects::default(), "{r:?}");
        }
    })
    .unwrap();
}
