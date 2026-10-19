//! Mosaic and Frame Mosaic (M14.9, `creating.md` §3): the blocks tile the page, keep their
//! pictures' ratios, put the frame centre in the largest block, and a swap exchanges two blocks.

use dalica_core::PhotoId;
use dalica_create::collage::{Collage, Format, Orientation, Page, Placement, Style, layout};
use proptest::prelude::*;

use crate::common::cases;

const MOSAICS: [Style; 2] = [Style::Mosaic, Style::FrameMosaic];
/// Photo ratios: 3:2, 2:3, square, 4:3, 3:4, 16:9 and 9:16.
const RATIOS: [f64; 7] = [1.5, 2.0 / 3.0, 1.0, 4.0 / 3.0, 0.75, 16.0 / 9.0, 9.0 / 16.0];

fn collage(style: Style, n: u64, seed: u64) -> Collage {
    let mut c = Collage::new((1..=n).map(PhotoId::new), seed);
    c.style = style;
    c
}

fn aspects(n: u64) -> Vec<f64> {
    (0..n as usize).map(|i| RATIOS[i % RATIOS.len()]).collect()
}

fn page(width: f64, height: f64, orientation: Orientation) -> Page {
    Page { format: Format::Ratio { name: "test".into(), width, height }, orientation }
}

/// A placement's box in page units (the page's height is 1, its width `r`), grown by `d` on
/// every side: `[x0, y0, x1, y1]`.
fn rect(p: &Placement, r: f64, d: f64) -> [f64; 4] {
    let (cx, cy, w, h) = (p.centre[0] * r, p.centre[1], p.size[0] * r, p.size[1]);
    [cx - w / 2.0 - d, cy - h / 2.0 - d, cx + w / 2.0 + d, cy + h / 2.0 + d]
}

fn overlap(a: [f64; 4], b: [f64; 4]) -> f64 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    w * h
}

fn by_id(ps: &[Placement], id: u64) -> Placement {
    *ps.iter().find(|p| p.id == PhotoId::new(id)).expect("the picture is placed")
}

fn largest(ps: &[Placement]) -> PhotoId {
    ps.iter().max_by(|a, b| (a.size[0] * a.size[1]).total_cmp(&(b.size[0] * b.size[1]))).expect("placements").id
}

#[test]
fn the_blocks_tile_the_page_exactly_and_repeat_for_a_seed() {
    let pages = [page(16.0, 9.0, Orientation::Landscape), page(16.0, 9.0, Orientation::Portrait), page(1.0, 1.0, Orientation::Landscape)];
    for style in MOSAICS {
        for pg in &pages {
            for n in [1, 2, 7, 100] {
                for spacing in [0.0, 0.01, 0.05] {
                    let mut c = collage(style, n, 0xfeed + n);
                    c.page = pg.clone();
                    c.settings.spacing = spacing;
                    let r = c.page.ratio();
                    let ps = layout(&c, &aspects(n));
                    let what = format!("{style:?}, {n} pictures, page {r:.3}, spacing {spacing}");
                    // The gap is the spacing of the short side, narrowed when the pictures are
                    // too many for it; the blocks keep it from the page's edge too.
                    let spaced = spacing * r.min(1.0);
                    let gap = ps.iter().map(|p| rect(p, r, 0.0)[0]).fold(f64::INFINITY, f64::min);
                    assert!(gap <= spaced + 1e-12 && (spacing == 0.0) == (gap.abs() < 1e-12), "{what}: a gap of {gap} for {spaced}");
                    if n <= 7 {
                        assert!((gap - spaced).abs() < 1e-12, "{what}: the whole gap for a few pictures");
                    }
                    assert_eq!(ps.len(), n as usize, "{what}");
                    assert_eq!(ps, layout(&c, &aspects(n)), "{what}: the same seed lays out the same");
                    // Each block with half a gap round it tiles the page less half a gap round
                    // the edge: the areas add up and no two overlap.
                    let grown: Vec<[f64; 4]> = ps.iter().map(|p| rect(p, r, gap / 2.0)).collect();
                    let sum: f64 = grown.iter().map(|g| (g[2] - g[0]) * (g[3] - g[1])).sum();
                    let inner = (r - gap) * (1.0 - gap);
                    assert!((sum - inner).abs() < 1e-9 * r.max(1.0), "{what}: {sum} of {inner}");
                    for (i, a) in grown.iter().enumerate() {
                        assert!(a[0] >= gap / 2.0 - 1e-9 && a[1] >= gap / 2.0 - 1e-9, "{what}: on the page");
                        assert!(a[2] <= r - gap / 2.0 + 1e-9 && a[3] <= 1.0 - gap / 2.0 + 1e-9, "{what}: on the page");
                        for b in grown.iter().skip(i + 1) {
                            assert!(overlap(*a, *b) < 1e-12, "{what}: two blocks overlap");
                        }
                    }
                    for p in &ps {
                        assert_eq!(p.angle, 0.0, "{what}");
                        assert!(p.size[0] > 0.0 && p.size[1] > 0.0, "{what}");
                    }
                }
            }
        }
    }
}

#[test]
fn a_mosaic_block_keeps_within_twice_its_pictures_ratio() {
    let pages = [
        page(16.0, 9.0, Orientation::Landscape),
        page(16.0, 9.0, Orientation::Portrait),
        page(4.0, 3.0, Orientation::Landscape),
        page(1.0, 1.0, Orientation::Landscape),
    ];
    for pg in &pages {
        for n in 2..=30 {
            for (mix, a) in [("mixed", aspects(n)), ("3:2", vec![1.5; n as usize]), ("2:3", vec![2.0 / 3.0; n as usize])] {
                for seed in [1, 2, 0xdead_beef] {
                    let mut c = collage(Style::Mosaic, n, seed);
                    c.page = pg.clone();
                    let r = c.page.ratio();
                    // No two blocks tile a square within twice of both 3:2 and 2:3: side by side
                    // the 3:2 one needs ¾ of the width and the 2:3 one more than a third.
                    if n == 2 && r == 1.0 && mix == "mixed" {
                        continue;
                    }
                    for p in layout(&c, &a) {
                        let block = p.size[0] * r / p.size[1];
                        let picture = a[p.clip];
                        let off = (block / picture).max(picture / block);
                        assert!(off <= 2.0, "page {r:.3}, {n} {mix} pictures, seed {seed}: a block of {block:.3} for a picture of {picture:.3}");
                    }
                }
            }
        }
    }
}

#[test]
fn the_frame_centre_is_the_largest_block_and_set_as_frame_center_moves_it() {
    for pg in [page(16.0, 9.0, Orientation::Landscape), page(16.0, 9.0, Orientation::Portrait)] {
        for n in [2, 3, 4, 5, 7, 9, 30, 100] {
            for spacing in [0.0, 0.05] {
                let mut c = collage(Style::FrameMosaic, n, 7 + n);
                c.page = pg.clone();
                c.settings.spacing = spacing;
                let a = aspects(n);
                let what = format!("{n} pictures, page {:.3}, spacing {spacing}", c.page.ratio());
                assert_eq!(largest(&layout(&c, &a)), PhotoId::new(1), "{what}: the first picture by default");
                c.frame_centre = Some(PhotoId::new(n));
                let ps = layout(&c, &a);
                assert_eq!(largest(&ps), PhotoId::new(n), "{what}: Set as Frame Center");
                let centre = by_id(&ps, n);
                let area = |p: &Placement| p.size[0] * p.size[1];
                assert!(ps.iter().filter(|p| p.id != centre.id).all(|p| area(p) < area(&centre)), "{what}: strictly the largest");
                if n >= 5 {
                    // With a band on every side the centre block sits in the middle of the page.
                    assert!((centre.centre[0] - 0.5).abs() < 1e-9 && (centre.centre[1] - 0.5).abs() < 1e-9, "{what}: {:?}", centre.centre);
                }
            }
        }
    }
}

#[test]
fn a_swap_exchanges_exactly_two_blocks() {
    for style in MOSAICS {
        // 1 ↔ 4 moves Frame Mosaic's default centre; 2 ↔ 6 two band pictures.
        for [a, b] in [[2, 6], [1, 4]] {
            let mut c = collage(style, 9, 5);
            let ratios = aspects(9);
            let before = layout(&c, &ratios);
            c.swaps = vec![[PhotoId::new(a), PhotoId::new(b)]];
            let after = layout(&c, &ratios);
            let block = |p: Placement| (p.centre, p.size);
            assert_eq!(block(by_id(&after, a)), block(by_id(&before, b)), "{style:?}: {a} takes {b}'s block");
            assert_eq!(block(by_id(&after, b)), block(by_id(&before, a)), "{style:?}: {b} takes {a}'s block");
            assert_ne!(block(by_id(&before, a)), block(by_id(&before, b)));
            for id in (1..=9).filter(|&id| id != a && id != b) {
                assert_eq!(by_id(&after, id), by_id(&before, id), "{style:?}: {id} stays");
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(256), ..ProptestConfig::default() })]

    #[test]
    fn no_count_or_ratio_panics(
        n in 0u64..=1000,
        width in prop_oneof![0.001f64..1000.0, any::<f64>()],
        portrait in any::<bool>(),
        spacing in 0.0f64..=0.05,
        seed in any::<u64>(),
        frame in any::<bool>(),
        ratios in proptest::collection::vec(prop_oneof![0.001f64..1000.0, any::<f64>()], 0..64),
    ) {
        let mut c = collage(if frame { Style::FrameMosaic } else { Style::Mosaic }, n, seed);
        c.page = page(width, 1.0, if portrait { Orientation::Portrait } else { Orientation::Landscape });
        c.settings.spacing = spacing;
        c.frame_centre = (n > 0).then(|| PhotoId::new(seed % n + 1));
        let a: Vec<f64> = (0..n as usize).map(|i| ratios.get(i % ratios.len().max(1)).copied().unwrap_or(1.0)).collect();
        let ps = layout(&c, &a);
        prop_assert_eq!(ps.len(), n as usize);
        for p in ps {
            prop_assert!(p.centre.iter().chain(&p.size).all(|v| v.is_finite()));
            prop_assert!(p.size.iter().all(|&v| v >= 0.0));
        }
    }
}
