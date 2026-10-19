//! The collage layouts of Picture Pile, Grid, Contact Sheet and Multiple Exposure (M14.8,
//! `creating.md` §3).

use dalica_core::PhotoId;
use dalica_create::collage::layout::sheet;
use dalica_create::collage::{Border, Collage, Format, Orientation, Page, Placement, Style, layout};
use proptest::prelude::*;

use crate::common::{cases, full};

const EPS: f64 = 1e-9;

fn collage(n: u64, style: Style, seed: u64) -> Collage {
    let mut c = Collage::new((1..=n).map(PhotoId::new), seed);
    c.style = style;
    c
}

/// Landscape and portrait pictures, a square and a panorama, in turn.
fn aspects(n: usize) -> Vec<f64> {
    [1.5, 0.75, 1.0, 4.0 / 3.0, 2.5].iter().copied().cycle().take(n).collect()
}

fn page(ratio_w: f64, ratio_h: f64, orientation: Orientation) -> Page {
    Page { format: Format::Ratio { name: "test".into(), width: ratio_w, height: ratio_h }, orientation }
}

fn pages() -> Vec<Page> {
    vec![
        Page::default(),
        page(16.0, 9.0, Orientation::Portrait),
        page(1.0, 1.0, Orientation::Landscape),
        page(4.0, 6.0, Orientation::Portrait),
        page(3.0, 1.0, Orientation::Landscape),
    ]
}

/// The corners of a placement's box and its border in page units (the page's height is 1), turned
/// clockwise about its centre.
fn corners(p: &Placement, ratio: f64, border: Border) -> [[f64; 2]; 4] {
    let (w, h) = (p.size[0] * ratio, p.size[1]);
    let b = w.min(h);
    let (l, t, r, bot) = (w / 2.0 + border.sides() * b, h / 2.0 + border.sides() * b, w / 2.0 + border.sides() * b, h / 2.0 + border.bottom() * b);
    let (cx, cy) = (p.centre[0] * ratio, p.centre[1]);
    let (sin, cos) = p.angle.to_radians().sin_cos();
    [[-l, -t], [r, -t], [r, bot], [-l, bot]].map(|[x, y]| [cx + x * cos - y * sin, cy + x * sin + y * cos])
}

/// Whether the point (page units) lies on the placement's box.
fn covers(p: &Placement, ratio: f64, [x, y]: [f64; 2]) -> bool {
    let (dx, dy) = (x - p.centre[0] * ratio, y - p.centre[1]);
    let (sin, cos) = (-p.angle).to_radians().sin_cos();
    let (u, v) = (dx * cos - dy * sin, dx * sin + dy * cos);
    u.abs() <= p.size[0] * ratio / 2.0 && v.abs() <= p.size[1] / 2.0
}

/// The share of the page no picture covers, sampled on a grid.
fn bare(ps: &[Placement], ratio: f64) -> f64 {
    let steps = 120;
    let mut empty = 0;
    for i in 0..steps {
        for j in 0..steps {
            let pt = [(i as f64 + 0.5) / steps as f64 * ratio, (j as f64 + 0.5) / steps as f64];
            if !ps.iter().any(|p| covers(p, ratio, pt)) {
                empty += 1;
            }
        }
    }
    f64::from(empty) / f64::from(steps * steps)
}

/// A placement as a box in page units: left, top, right, bottom.
fn bounds(p: &Placement, ratio: f64) -> [f64; 4] {
    let (cx, cy, w, h) = (p.centre[0] * ratio, p.centre[1], p.size[0] * ratio, p.size[1]);
    [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
}

fn overlap(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[2] - EPS && b[0] < a[2] - EPS && a[1] < b[3] - EPS && b[1] < a[3] - EPS
}

/// The shown part's shape on the picture's: the box's shape when it fills the box.
fn shown_ratio(p: &Placement, aspect: f64) -> f64 {
    let s = p.shown;
    (s.x1() - s.x0()) / (s.y1() - s.y0()) * aspect
}

#[test]
fn a_seed_lays_out_the_same_and_only_the_pile_moves_with_another() {
    let styles = [Style::Pile, Style::Grid, Style::ContactSheet, Style::MultipleExposure];
    for n in [1, 2, 7, 100] {
        let a = aspects(n as usize);
        for style in styles {
            let one = layout(&collage(n, style, 11), &a);
            assert_eq!(one.len(), n as usize);
            assert_eq!(one, layout(&collage(n, style, 11), &a), "{style:?}, {n} pictures: the same seed");
            let other = layout(&collage(n, style, 12), &a);
            if style == Style::Pile {
                assert_ne!(one, other, "{n} pictures: another seed scatters the pile again");
            } else {
                assert_eq!(one, other, "{style:?}, {n} pictures: the seed changes nothing");
            }
        }
    }
}

#[test]
fn the_pile_stays_on_the_page_and_covers_it() {
    let seeds: u64 = if full() { 64 } else { 6 };
    let mut worst: f64 = 0.0;
    for pg in pages() {
        let ratio = pg.ratio();
        for border in [Border::None, Border::White, Border::Instant] {
            for n in [1, 2, 7, 20, 100] {
                let a = aspects(n as usize);
                for seed in 0..seeds {
                    let mut c = collage(n, Style::Pile, seed);
                    c.page = pg.clone();
                    c.settings.border = border;
                    let ps = layout(&c, &a);
                    for p in &ps {
                        assert!(p.angle.abs() <= 15.0, "turned within 15 degrees");
                        assert_eq!(p.shown, dalica_core::Rect01::FULL, "whole pictures");
                        for [x, y] in corners(p, ratio, border) {
                            assert!(
                                (-EPS..=ratio + EPS).contains(&x) && (-EPS..=1.0 + EPS).contains(&y),
                                "{n} pictures, {border:?}: a corner at {x}, {y} on a page of {ratio}"
                            );
                        }
                    }
                    assert_eq!(ps.iter().map(|p| p.clip).collect::<Vec<_>>(), (0..n as usize).collect::<Vec<_>>(), "stacked in clip order");
                    if n == 20 && border == Border::None {
                        let b = bare(&ps, ratio);
                        worst = worst.max(b);
                        assert!(b <= 0.2, "20 pictures, seed {seed}, page {ratio}: {b} of the page bare");
                    }
                }
            }
        }
    }
    eprintln!("the pile of 20 leaves at most {worst:.3} of the page bare");
}

#[test]
fn grid_cells_are_equal_apart_by_the_spacing_and_filled() {
    for pg in pages() {
        let ratio = pg.ratio();
        let short = ratio.min(1.0);
        for spacing in [0.0, 0.01, 0.05] {
            for n in [1, 2, 7, 100] {
                let a = aspects(n as usize);
                let mut c = collage(n, Style::Grid, 5);
                c.page = pg.clone();
                c.settings.spacing = spacing;
                let ps = layout(&c, &a);
                // The spacing, capped so the gaps of a crowded grid leave its cells half the page.
                let mut tops: Vec<f64> = ps.iter().map(|p| bounds(p, ratio)[1]).collect();
                tops.sort_by(f64::total_cmp);
                tops.dedup_by(|a, b| (*a - *b).abs() < EPS);
                let rows = tops.len() as f64;
                let cols = ps.iter().filter(|p| (bounds(p, ratio)[1] - tops[0]).abs() < EPS).count() as f64;
                let gap = (spacing * short).min(0.5 * ratio / (cols + 1.0)).min(0.5 / (rows + 1.0));
                if n == 7 {
                    assert_eq!(gap, spacing * short, "a grid of 7 keeps the whole spacing");
                }
                let first = ps.first().expect("a picture");
                for p in &ps {
                    assert!((p.size[0] - first.size[0]).abs() < EPS && (p.size[1] - first.size[1]).abs() < EPS, "equal cells");
                    assert_eq!(p.angle, 0.0);
                    let cell = p.size[0] * ratio / p.size[1];
                    let aspect = a.get(p.clip).copied().unwrap_or(1.0);
                    assert!((shown_ratio(p, aspect) - cell).abs() < 1e-4 * cell.max(1.0), "the picture fills its cell");
                    let [l, t, r, b] = bounds(p, ratio);
                    assert!(
                        l >= gap - EPS && t >= gap - EPS && r <= ratio - gap + EPS && b <= 1.0 - gap + EPS,
                        "{n} pictures, spacing {spacing}, page {ratio}: the spacing round the edge {l} {t} {r} {b}"
                    );
                }
                for (i, p) in ps.iter().enumerate() {
                    for q in ps.iter().skip(i + 1) {
                        let (bp, bq) = (bounds(p, ratio), bounds(q, ratio));
                        assert!(!overlap(bp, bq), "cells never overlap");
                        // Neighbours in a row or a column are the spacing apart.
                        if (bp[1] - bq[1]).abs() < EPS && bq[0] > bp[2] - EPS && bq[0] - bp[2] < gap + (bp[2] - bp[0]) / 2.0 {
                            assert!((bq[0] - bp[2] - gap).abs() < 1e-9, "{n} pictures, spacing {spacing}: {} apart in a row", bq[0] - bp[2]);
                        }
                        if (bp[0] - bq[0]).abs() < EPS && bq[1] > bp[3] - EPS && bq[1] - bp[3] < gap + (bp[3] - bp[1]) / 2.0 {
                            assert!((bq[1] - bp[3] - gap).abs() < 1e-9, "{n} pictures, spacing {spacing}: {} apart in a column", bq[1] - bp[3]);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_contact_sheets_names_sit_under_their_pictures_clear_of_the_next_row() {
    for pg in pages() {
        let ratio = pg.ratio();
        for border in [Border::None, Border::Instant] {
            for n in [1, 2, 7, 100] {
                let a = aspects(n as usize);
                let mut c = collage(n, Style::ContactSheet, 1);
                c.page = pg.clone();
                c.settings.border = border;
                let ps = layout(&c, &a);
                let names = sheet::names(&c, &a);
                assert_eq!(names.len(), ps.len());
                let lines: Vec<[f64; 4]> = names
                    .iter()
                    .map(|l| {
                        let (cx, cy, w, h) = (l.centre[0] * ratio, l.centre[1], l.size[0] * ratio, l.size[1]);
                        [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
                    })
                    .collect();
                for p in &ps {
                    assert_eq!(p.shown, dalica_core::Rect01::FULL, "whole pictures");
                    let aspect = a.get(p.clip).copied().unwrap_or(1.0);
                    assert!((p.size[0] * ratio / p.size[1] - aspect).abs() < 1e-6 * aspect, "the picture keeps its shape");
                    // The picture with its border, as a box.
                    let pts = corners(p, ratio, border);
                    let framed = [pts[0][0], pts[0][1], pts[2][0], pts[2][1]];
                    assert!(framed[0] >= -EPS && framed[1] >= -EPS && framed[2] <= ratio + EPS && framed[3] <= 1.0 + EPS, "on the page");
                    for (line, name) in lines.iter().zip(&names) {
                        if name.clip == p.clip {
                            assert!(line[1] >= framed[3] - EPS, "the name is under its picture");
                            assert!(line[0] <= framed[0] + EPS && line[2] >= framed[2] - EPS, "the line is as wide as the cell");
                        } else {
                            assert!(!overlap(*line, framed), "{n} pictures: a name never runs into another picture");
                        }
                    }
                }
                for (i, l) in lines.iter().enumerate() {
                    assert!(l[3] <= 1.0 + EPS && l[1] >= -EPS, "on the page");
                    for m in lines.iter().skip(i + 1) {
                        assert!(!overlap(*l, *m), "names never overlap");
                    }
                }
            }
        }
    }
}

#[test]
fn multiple_exposure_covers_the_page_with_every_picture() {
    for pg in pages() {
        let a = aspects(7);
        let mut c = collage(7, Style::MultipleExposure, 3);
        c.page = pg.clone();
        let ps = layout(&c, &a);
        assert_eq!(ps.len(), 7);
        for p in &ps {
            assert_eq!((p.centre, p.size, p.angle), ([0.5, 0.5], [1.0, 1.0], 0.0));
            let aspect = a.get(p.clip).copied().unwrap_or(1.0);
            assert!((shown_ratio(p, aspect) - pg.ratio()).abs() < 1e-4 * pg.ratio().max(1.0), "cropped to the page");
        }
    }
}

fn style() -> impl Strategy<Value = Style> {
    prop_oneof![Just(Style::Pile), Just(Style::Grid), Just(Style::ContactSheet), Just(Style::MultipleExposure)]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn any_count_and_page_lay_out_finite(
        style in style(),
        n in 0u64..=1000,
        ratio in 1.0f64..1000.0,
        portrait in any::<bool>(),
        seed in any::<u64>(),
        spacing in 0.0f64..=0.05,
        border in prop_oneof![Just(Border::None), Just(Border::White), Just(Border::Instant)],
        odd in proptest::collection::vec(prop_oneof![Just(f64::NAN), Just(0.0), Just(-1.0), Just(f64::INFINITY), Just(1e-9), Just(1e9), 0.01f64..100.0], 0..8),
    ) {
        let mut c = collage(n, style, seed);
        c.page = page(ratio, 1.0, if portrait { Orientation::Portrait } else { Orientation::Landscape });
        c.settings.spacing = spacing;
        c.settings.border = border;
        let mut a = aspects(n as usize);
        for (slot, v) in a.iter_mut().zip(&odd) {
            *slot = *v;
        }
        let ps = layout(&c, &a);
        prop_assert_eq!(ps.len(), n as usize);
        for p in &ps {
            prop_assert!(p.centre.iter().chain(&p.size).all(|v| v.is_finite()) && p.angle.is_finite());
            prop_assert!(p.size.iter().all(|&v| v >= 0.0));
        }
        if style == Style::ContactSheet {
            prop_assert!(sheet::names(&c, &a).iter().all(|l| l.centre.iter().chain(&l.size).all(|v| v.is_finite())));
        }
    }
}
