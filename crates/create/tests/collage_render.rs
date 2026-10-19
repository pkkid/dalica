//! Rendering a collage (M14.10, `creating.md` §3): the page from the document and its placements,
//! each picture asked one at a time (here `scenes` pictures), and the JPEG.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use dalica_core::{PhotoId, Rect01};
use dalica_create::collage::render::{self, INSTANT_BORDER, MAX_PAGE_PIXELS, Picture, QUALITY, RenderError, WHITE_BORDER};
use dalica_create::collage::{Background, Border, Collage, Format, Orientation, Page, Placement, Settings, Style, layout};
use dalica_image::Rgba8;
use dalica_meta::DateTaken;
use dalica_scenes::fingerprint;

use crate::common::{check_fingerprint, full, pixels};

/// A colour per picture, painted as a patch at the centre of the part it shows, so a centre pixel
/// says which picture is there.
fn patch_colour(clip: usize) -> [u8; 3] {
    const COLOURS: [[u8; 3]; 8] =
        [[230, 40, 40], [40, 200, 60], [40, 70, 230], [240, 220, 30], [220, 40, 220], [30, 210, 220], [250, 140, 20], [120, 60, 200]];
    COLOURS[clip % COLOURS.len()]
}

/// The `scenes` picture of a placement at `size`, with its patch (a tenth of the part shown).
fn scene_picture(p: &Placement, (w, h): (u32, u32)) -> Picture {
    let mut img = pixels(100 + p.clip as u64, w, h);
    let (cx, cy) = p.shown.center();
    let (pw, ph) = (p.shown.width() * f64::from(w) * 0.1, p.shown.height() * f64::from(h) * 0.1);
    let (x0, x1) = (((cx * f64::from(w)) - pw / 2.0).max(0.0) as u32, ((cx * f64::from(w)) + pw / 2.0) as u32);
    let (y0, y1) = (((cy * f64::from(h)) - ph / 2.0).max(0.0) as u32, ((cy * f64::from(h)) + ph / 2.0) as u32);
    let [r, g, b] = patch_colour(p.clip);
    for y in y0..y1.min(h) {
        for x in x0..x1.min(w) {
            let i = ((y * w + x) * 4) as usize;
            img.data[i..i + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
    Picture { image: img, caption: format!("Day {} at the lake", p.clip + 1), name: format!("IMG_{:04}.jpg", p.clip + 1) }
}

fn collage(style: Style, n: u64) -> Collage {
    let mut c = Collage::new((1..=n).map(PhotoId::new), 0x5eed_0000 + n);
    c.style = style;
    c
}

/// Pictures of 3:2, 2:3 and 4:3, by clip.
fn aspects(n: usize) -> Vec<f64> {
    (0..n).map(|i| [1.5, 2.0 / 3.0, 4.0 / 3.0][i % 3]).collect()
}

fn page_size(c: &Collage, long: u32) -> (u32, u32) {
    c.page.pixel_size(long)
}

fn draw(c: &Collage, placements: &[Placement], size: (u32, u32)) -> Rgba8 {
    let mut pic = |p: &Placement, s: (u32, u32)| Some(scene_picture(p, s));
    render::draw(c, placements, &mut pic, size, &|| false, &mut |_, _| {}).unwrap()
}

fn at(img: &Rgba8, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * img.width + x) * 4) as usize;
    [img.data[i], img.data[i + 1], img.data[i + 2]]
}

fn close(a: [u8; 3], b: [u8; 3], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tolerance)
}

/// Whether the page point `(x, y)` (normalised) lies inside `p`'s box, `margin` of its size in from
/// the edge, and where in the box: `(u, v)` from 0 to 1.
fn inside(p: &Placement, (w, h): (u32, u32), x: f64, y: f64) -> Option<(f64, f64)> {
    let (w, h) = (f64::from(w), f64::from(h));
    let (dx, dy) = ((x - p.centre[0]) * w, (y - p.centre[1]) * h);
    let (s, c) = p.angle.to_radians().sin_cos();
    let (u, v) = (c * dx + s * dy, -s * dx + c * dy);
    let (bw, bh) = (p.size[0] * w, p.size[1] * h);
    let (u, v) = (u / bw + 0.5, v / bh + 0.5);
    ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some((u, v))
}

/// Whether the box point `(u, v)` falls in the patch (with a little room for the patch's edge).
fn in_patch((u, v): (f64, f64)) -> bool {
    (u - 0.5).abs() < 0.04 && (v - 0.5).abs() < 0.04
}

#[test]
fn each_style_at_3000_px_has_the_pages_ratio_and_every_centre_where_its_placement_says() {
    for style in Style::ALL {
        let mut c = collage(style, 5);
        c.settings = Settings { shadows: false, ..Settings::default() };
        c.background.colour = [90, 90, 90];
        let placements = layout(&c, &aspects(5));
        // Every style at 3000 px at full strength; a quick run draws the pile there and the
        // others at 1000 px.
        let long = if full() || style == Style::Pile { 3000 } else { 1000 };
        let size = page_size(&c, long);
        let page = draw(&c, &placements, size);
        assert_eq!((page.width, page.height), size, "{style:?}");
        assert_eq!(page.width.max(page.height), long, "{style:?}");
        let ratio = f64::from(page.width) / f64::from(page.height);
        assert!((ratio - c.page.ratio()).abs() < 1.0 / f64::from(page.width.min(page.height)), "{style:?}: {ratio} for {}", c.page.ratio());

        let mut checked = 0;
        for p in &placements {
            let (x, y) = (p.centre[0], p.centre[1]);
            let (px, py) = ((x * f64::from(size.0)) as u32, (y * f64::from(size.1)) as u32);
            if px >= size.0 || py >= size.1 {
                continue;
            }
            let expected = if style == Style::MultipleExposure {
                // Every picture over this point weighs the same; only checked where each of them
                // shows its patch there.
                let over: Vec<_> = placements.iter().filter_map(|q| inside(q, size, x, y).map(|uv| (q, uv))).collect();
                if !over.iter().all(|(_, uv)| in_patch(*uv)) {
                    continue;
                }
                let mut sum = [0u32; 3];
                for (q, _) in &over {
                    for (s, v) in sum.iter_mut().zip(patch_colour(q.clip)) {
                        *s += u32::from(v);
                    }
                }
                sum.map(|s| (s as f64 / over.len() as f64).round() as u8)
            } else {
                // The picture on top at this point (a pile's pictures overlap).
                let top = placements.iter().filter(|q| inside(q, size, x, y).is_some()).max_by_key(|q| q.z).unwrap();
                if top.z != p.z {
                    continue;
                }
                patch_colour(p.clip)
            };
            let got = at(&page, px, py);
            assert!(close(got, expected, 2), "{style:?}: picture {} at its centre ({px}, {py}) is {got:?}, not {expected:?}", p.clip);
            checked += 1;
        }
        assert!(checked >= 1, "{style:?}: no centre was checked");
    }
}

/// A pile of six pictures, turned and overlapping, with every option on.
fn showcase(style: Style) -> (Collage, Vec<Placement>) {
    let mut c = collage(style, 6);
    c.settings = Settings { border: Border::Instant, spacing: 0.02, shadows: true, captions: true };
    let full = Rect01::FULL;
    let p = |clip: usize, centre: [f64; 2], size: [f64; 2], angle: f64, shown: Rect01| Placement {
        clip,
        id: PhotoId::new(clip as u64 + 1),
        centre,
        size,
        angle,
        z: clip,
        shown,
    };
    let crop = |x0, y0, x1, y1| Rect01::new(x0, y0, x1, y1).unwrap();
    let placements = match style {
        Style::Pile => {
            c.background.colour = [24, 30, 40];
            vec![
                p(0, [0.25, 0.3], [0.3, 0.4], -12.0, full),
                p(1, [0.6, 0.35], [0.22, 0.55], 8.0, full),
                p(2, [0.4, 0.65], [0.32, 0.42], 4.0, full),
                p(3, [0.78, 0.7], [0.25, 0.33], -6.0, full),
                p(4, [0.15, 0.75], [0.2, 0.3], 14.0, full),
                p(5, [0.55, 0.55], [0.18, 0.24], -2.0, full),
            ]
        }
        Style::Mosaic => vec![
            p(0, [0.25, 0.25], [0.48, 0.46], 0.0, crop(0.0, 0.1, 1.0, 0.9)),
            p(1, [0.25, 0.75], [0.48, 0.46], 0.0, crop(0.1, 0.0, 0.9, 1.0)),
            p(2, [0.625, 0.5], [0.23, 0.96], 0.0, crop(0.3, 0.0, 0.7, 1.0)),
            p(3, [0.875, 0.25], [0.23, 0.46], 0.0, full),
            p(4, [0.875, 0.75], [0.23, 0.46], 0.0, crop(0.2, 0.0, 0.8, 1.0)),
        ],
        Style::FrameMosaic => vec![
            p(0, [0.5, 0.5], [0.48, 0.6], 0.0, crop(0.05, 0.0, 0.95, 1.0)),
            p(1, [0.125, 0.25], [0.23, 0.46], 0.0, crop(0.2, 0.0, 0.8, 1.0)),
            p(2, [0.125, 0.75], [0.23, 0.46], 0.0, full),
            p(3, [0.875, 0.25], [0.23, 0.46], 0.0, crop(0.0, 0.2, 1.0, 0.8)),
            p(4, [0.875, 0.75], [0.23, 0.46], 0.0, full),
            p(5, [0.5, 0.1], [0.48, 0.16], 0.0, crop(0.0, 0.4, 1.0, 0.6)),
        ],
        Style::Grid => {
            c.background.picture = Some(PhotoId::new(6));
            (0..6).map(|i| p(i, [0.18 + 0.32 * (i % 3) as f64, 0.27 + 0.46 * (i / 3) as f64], [0.3, 0.42], 0.0, crop(0.1, 0.0, 0.9, 1.0))).collect()
        }
        Style::ContactSheet => {
            c.background.colour = [236, 236, 232];
            c.settings.border = Border::White;
            (0..6).map(|i| p(i, [0.18 + 0.32 * (i % 3) as f64, 0.22 + 0.45 * (i / 3) as f64], [0.22, 0.3], 0.0, full)).collect()
        }
        Style::MultipleExposure => (0..3).map(|i| p(i, [0.5, 0.5], [1.0, 1.0], 0.0, crop(0.0, 0.1 * i as f64, 1.0, 0.7 + 0.1 * i as f64))).collect(),
    };
    (c, placements)
}

#[test]
fn a_render_at_600_px_agrees_with_one_at_3000_px_downscaled() {
    let (c, placements) = showcase(Style::Pile);
    let small = draw(&c, &placements, page_size(&c, 600));
    let large = draw(&c, &placements, page_size(&c, 3000));
    let shrunk = dalica_image::resize::resize(&large, small.width, small.height).unwrap();
    let (a, b) =
        (fingerprint::fingerprint(small.width, small.height, &small.data), fingerprint::fingerprint(shrunk.width, shrunk.height, &shrunk.data));
    let d = fingerprint::distance(&a, &b);
    assert!(d <= 2.0, "600 px and 3000 px downscaled are {d:.2} apart");
}

#[test]
fn a_white_border_is_white_and_a_shadow_darkens_only_outside_the_picture() {
    let mut c = collage(Style::Pile, 1);
    c.page = Page { format: Format::Ratio { name: "Square".into(), width: 1.0, height: 1.0 }, orientation: Orientation::Landscape };
    c.background = Background { colour: [128, 128, 128], picture: None };
    c.settings = Settings { border: Border::White, spacing: 0.0, shadows: false, captions: false };
    let placement = Placement { clip: 0, id: PhotoId::new(1), centre: [0.5, 0.5], size: [0.5, 0.5], angle: 0.0, z: 0, shown: Rect01::FULL };
    let size = (800, 800);
    let plain = draw(&c, &[placement], size);
    c.settings.shadows = true;
    let shadowed = draw(&c, &[placement], size);

    // The picture spans 200..600; its border is 3 % of 400 = 12 px all round.
    for (x, y) in [(194, 400), (400, 194), (605, 400), (400, 605), (190, 190), (608, 608)] {
        assert_eq!(at(&plain, x, y), WHITE_BORDER, "border pixel ({x}, {y})");
    }
    assert_eq!(at(&plain, 400, 400), patch_colour(0));
    let card = |x: u32, y: u32| (188..612).contains(&x) && (188..612).contains(&y);
    let mut darker = 0;
    for y in 0..size.1 {
        for x in 0..size.0 {
            let (a, b) = (at(&plain, x, y), at(&shadowed, x, y));
            if card(x, y) {
                assert_eq!(a, b, "inside the card at ({x}, {y})");
            } else {
                assert!(b.iter().zip(a).all(|(s, p)| *s <= p), "the shadow lightened ({x}, {y})");
                darker += usize::from(b != a);
            }
        }
    }
    assert!(darker > 1000, "the shadow darkened only {darker} pixels");
    // Down and to the right of the picture, not up and to the left.
    assert!(at(&shadowed, 616, 616)[0] < 128 && at(&shadowed, 178, 178) == [128, 128, 128]);

    // An instant border is wider at the bottom.
    c.settings = Settings { border: Border::Instant, shadows: false, ..c.settings };
    let instant = draw(&c, &[placement], size);
    assert_eq!(at(&instant, 400, 670), INSTANT_BORDER, "the instant border's bottom (20 % of 400 = 80 px)");
    assert_eq!(at(&instant, 400, 185), INSTANT_BORDER, "its top is 5 %, 20 px");
    assert_eq!(at(&instant, 400, 175), [128, 128, 128]);
}

#[test]
fn cancelling_after_the_first_picture_stops() {
    let c = collage(Style::Grid, 6);
    let placements = layout(&c, &aspects(6));
    let asked = Cell::new(0);
    let mut pic = |p: &Placement, s: (u32, u32)| {
        asked.set(asked.get() + 1);
        Some(scene_picture(p, s))
    };
    let told = RefCell::new(Vec::new());
    let got = render::render(&c, &placements, &mut pic, page_size(&c, 600), None, &|| asked.get() >= 1, &mut |d, t| told.borrow_mut().push((d, t)));
    assert_eq!(got, Err(RenderError::Cancelled));
    assert_eq!(asked.get(), 1, "no picture after the cancel");
    assert_eq!(*told.borrow(), vec![(1, 6)]);
}

#[test]
fn peak_memory_stays_within_the_page_and_one_picture() {
    let (c, placements) = showcase(Style::Pile);
    let size = page_size(&c, 3000);
    // The sizes the render asks for, then every picture made beforehand, so the measured render
    // allocates only its own memory and one copy of each picture as the caller hands it over.
    let mut sizes = HashMap::new();
    let mut ask = |p: &Placement, s: (u32, u32)| {
        sizes.insert(p.clip, s);
        None
    };
    render::draw(&c, &placements, &mut ask, size, &|| false, &mut |_, _| {}).unwrap();
    let made: HashMap<usize, Picture> = placements.iter().map(|p| (p.clip, scene_picture(p, sizes[&p.clip]))).collect();
    let largest = made.values().map(|p| p.image.data.len() as u64).max().unwrap();
    let mut hand = |p: &Placement, _: (u32, u32)| made.get(&p.clip).cloned();
    let info = allocation_counter::measure(|| {
        render::draw(&c, &placements, &mut hand, size, &|| false, &mut |_, _| {}).unwrap();
    });
    let page = u64::from(size.0) * u64::from(size.1) * 4;
    // Room for a caption's strip and the small lists beside the page and the picture.
    let slack = page / 16;
    assert!(info.bytes_max <= page + largest + slack, "peak {} bytes for a {page}-byte page and a {largest}-byte picture", info.bytes_max);
}

#[test]
fn the_jpeg_is_quality_92_with_the_date_of_making() {
    let c = collage(Style::Grid, 2);
    let placements = layout(&c, &aspects(2));
    let made = DateTaken::parse_iso("2026-10-19T14:02:00").unwrap();
    let mut pic = |p: &Placement, s: (u32, u32)| Some(scene_picture(p, s));
    let jpeg = render::render(&c, &placements, &mut pic, page_size(&c, 600), Some(&made), &|| false, &mut |_, _| {}).unwrap();
    assert_eq!(dalica_image::jpeg_quality::read(&jpeg).map(|s| s.quality), Some(QUALITY));
    let meta = dalica_meta::read(&jpeg, &dalica_meta::Limits::default()).unwrap();
    assert_eq!(meta.owned.date_taken, Some(made));
}

#[test]
fn hostile_sizes_placements_and_pictures_never_panic() {
    let c = collage(Style::Pile, 3);
    let mut none = |_: &Placement, _: (u32, u32)| None;
    for size in [(0, 10), (10, 0), (u32::MAX, u32::MAX), (1 << 14, (MAX_PAGE_PIXELS >> 14) as u32 + 1)] {
        assert!(matches!(render::draw(&c, &[], &mut none, size, &|| false, &mut |_, _| {}), Err(RenderError::Size(_))), "{size:?}");
    }
    let base = Placement { clip: 0, id: PhotoId::new(1), centre: [0.5, 0.5], size: [0.5, 0.5], angle: 0.0, z: 0, shown: Rect01::FULL };
    let hostile = [
        Placement { centre: [f64::NAN, 0.5], ..base },
        Placement { size: [f64::INFINITY, 1.0], ..base },
        Placement { size: [1e12, 1e12], angle: 1e300, ..base },
        Placement { size: [-1.0, 0.5], ..base },
        Placement { angle: f64::NAN, z: usize::MAX, ..base },
        Placement { centre: [-1e9, 1e9], ..base },
        Placement { shown: Rect01::new(0.5, 0.5, 0.500001, 0.500001).unwrap(), ..base },
    ];
    // Pictures whose pixels do not match their size, empty ones, and a 1 × 1 one.
    let mut i = 0;
    let mut odd = |_: &Placement, (w, h): (u32, u32)| {
        assert!(u64::from(w) * u64::from(h) <= 200 * 200, "a picture larger than the page was asked for: {w} × {h}");
        i += 1;
        let image = match i % 3 {
            0 => Rgba8 { width: 5, height: 5, data: vec![1; 7] },
            1 => Rgba8 { width: 0, height: 0, data: Vec::new() },
            _ => Rgba8 { width: 1, height: 1, data: vec![200; 4] },
        };
        Some(Picture { image, caption: "\u{202e}\0".repeat(1000), name: String::new() })
    };
    let mut all = c.clone();
    all.settings = Settings { border: Border::Instant, spacing: 0.0, shadows: true, captions: true };
    for style in Style::ALL {
        all.style = style;
        let page = render::draw(&all, &hostile, &mut odd, (200, 200), &|| false, &mut |_, _| {}).unwrap();
        assert_eq!(page.data.len(), 200 * 200 * 4);
    }
}

#[test]
fn fingerprints_of_the_six_styles_at_1200_px() {
    for style in Style::ALL {
        let (c, placements) = showcase(style);
        let page = draw(&c, &placements, page_size(&c, 1200));
        check_fingerprint(&format!("collage-{}", style.id()), &page, 1.0);
    }
}
