//! M7's renderer checked across every operation it adds, in this process (`dalica-edit`, the one
//! renderer behind the thumbnails, the edit view, Save and Export): each operation alone and all of
//! them in one recipe render alike at two sizes (the larger box-downscaled to the smaller), and an
//! operation Dalica does not know, or a legacy one, is skipped by the render and kept in the recipe.

use dalica_edit::{Recipe, Source, Target, render};
use dalica_image::Rgba8;
use dalica_scenes::{Scene, fingerprint};

/// The `scenes` photo of `seed` at `width × height`, upright, as RGBA.
fn scene(seed: u64, width: u32, height: u32) -> Rgba8 {
    let img = Scene::new(seed, width, height).render().unwrap();
    Rgba8 { width: img.width, height: img.height, data: img.data.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() }
}

/// `img` reduced to `width × height` by averaging the source area under each output pixel (an
/// independent reference, not the renderer's resampler).
pub fn box_downscale(img: &Rgba8, width: u32, height: u32) -> Rgba8 {
    let (fx, fy) = (f64::from(img.width) / f64::from(width), f64::from(img.height) / f64::from(height));
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let (x0, x1) = (f64::from(x) * fx, f64::from(x + 1) * fx);
            let (y0, y1) = (f64::from(y) * fy, f64::from(y + 1) * fy);
            let (mut sum, mut total) = ([0.0f64; 4], 0.0);
            for sy in y0.floor() as u32..(y1.ceil() as u32).min(img.height) {
                let wy = (y1.min(f64::from(sy + 1)) - y0.max(f64::from(sy))).max(0.0);
                for sx in x0.floor() as u32..(x1.ceil() as u32).min(img.width) {
                    let wx = (x1.min(f64::from(sx + 1)) - x0.max(f64::from(sx))).max(0.0);
                    let at = ((sy * img.width + sx) * 4) as usize;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += f64::from(img.data[at + c]) * wx * wy;
                    }
                    total += wx * wy;
                }
            }
            data.extend(sum.map(|v| (v / total.max(1e-12)).round().clamp(0.0, 255.0) as u8));
        }
    }
    Rgba8 { width, height, data }
}

/// The operations M7 adds, one recipe each, and all of them in one recipe with an unknown and a
/// legacy operation among them (the source photo is 600 × 400).
pub const RECIPES: [(&str, &[&str]); 12] = [
    ("no edit (the resampling alone)", &[]),
    ("crop", &["crop(0.1,0.15,0.8,0.9)"]),
    ("straighten", &["straighten(3.5)"]),
    ("autofix", &["autofix()"]),
    ("autocontrast", &["autocontrast()"]),
    ("autocolor", &["autocolor()"]),
    ("tuning fill, highlights, shadows, temperature", &["tuning(0.3,0.2,0.15,0.4,none)"]),
    ("tuning neutral", &["tuning(0,0,0,0,#c8a080)"]),
    ("fill_light as written by older files", &["fill_light(0.4)"]),
    ("retouch", &["retouch(0.62,0.55,0.3,0.4,0.04;0.2,0.7,0.5,0.75,0.02)"]),
    ("geometry", &["rotate(90)", "flip_h()"]),
    (
        "every M7 operation, an unknown and a legacy one",
        &[
            "future_op(1;2)",
            "crop(0.1,0.15,0.8,0.9)",
            "straighten(3.5)",
            "autofix()",
            "legacy:redeye(1)",
            "autocontrast()",
            "autocolor()",
            "tuning(0.3,0.2,0.15,0.4,#c8a080)",
            "retouch(0.62,0.55,0.3,0.4,0.04)",
            "rotate(90)",
            "flip_h()",
        ],
    ),
];

/// The unknown and legacy operations of the last recipe of [`RECIPES`].
pub const UNKNOWN: [&str; 2] = ["future_op(1;2)", "legacy:redeye(1)"];

/// The two sizes compared.
const SMALL: u32 = 96;
const LARGE: u32 = 384;
/// The largest mean difference per channel allowed between the two sizes, in levels of 255, and
/// the largest 99th percentile of the pixels' differences (their largest channel). A cell's mean
/// (the fingerprint distance the crate suites use on scenes made for it) is printed, not judged:
/// these scenes' hard-edged corner marker alone moves one cell by up to about 10 between sizes,
/// with no edit at all.
pub const MEAN_TOLERANCE: f64 = 1.0;
pub const P99_TOLERANCE: u8 = 8;

/// How two renders of one size differ: (mean per channel, 99th percentile of the pixels' largest
/// channel difference, the fingerprint distance).
fn compare(a: &Rgba8, b: &Rgba8) -> (f64, u8, f64) {
    let mut px: Vec<u8> = a.data.chunks(4).zip(b.data.chunks(4)).map(|(p, q)| (0..3).map(|k| p[k].abs_diff(q[k])).max().unwrap_or(0)).collect();
    let sum: f64 = a.data.chunks(4).zip(b.data.chunks(4)).map(|(p, q)| (0..3).map(|k| f64::from(p[k].abs_diff(q[k]))).sum::<f64>()).sum();
    let mean = sum / (px.len() * 3).max(1) as f64;
    px.sort_unstable();
    let p99 = px.get((px.len() * 99).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0);
    let d = fingerprint::distance(&fingerprint::fingerprint(a.width, a.height, &a.data), &fingerprint::fingerprint(b.width, b.height, &b.data));
    (mean, p99, d)
}

/// Every recipe of [`RECIPES`] at two sizes on three scenes, the larger box-downscaled to the
/// smaller. Panics past [`MEAN_TOLERANCE`] or [`P99_TOLERANCE`]. Returns the worst of each
/// measure per recipe, as a line.
pub fn two_sizes() -> Vec<String> {
    let mut lines = Vec::new();
    for (what, ops) in RECIPES {
        let recipe = Recipe::parse(ops).unwrap();
        let (mut mean_max, mut p99_max, mut d_max) = (0.0f64, 0u8, 0.0f64);
        for seed in [21u64, 61, 77] {
            let photo = scene(seed, 600, 400);
            let source = Source::upright(&photo);
            let a = render(source, &recipe, Target::Fit { width: SMALL, height: SMALL }).unwrap();
            let b = render(source, &recipe, Target::Fit { width: LARGE, height: LARGE }).unwrap();
            let (mean, p99, d) = compare(&a, &box_downscale(&b, a.width, a.height));
            assert!(
                mean <= MEAN_TOLERANCE && p99 <= P99_TOLERANCE,
                "{what} on scene {seed}: {SMALL} px and {LARGE} px renders differ after downscale by {mean:.2} per channel on average (at most {MEAN_TOLERANCE}), {p99} at the 99th percentile (at most {P99_TOLERANCE})"
            );
            (mean_max, p99_max, d_max) = (mean_max.max(mean), p99_max.max(p99), d_max.max(d));
        }
        lines.push(format!("{what}: mean {mean_max:.2}, p99 {p99_max}, a cell's mean {d_max:.2}"));
    }
    lines
}

/// The last recipe of [`RECIPES`] prints back with its unknown and legacy operations in place, and
/// renders exactly as the same recipe without them.
pub fn unknown_operations_survive() {
    let (_, ops) = RECIPES[RECIPES.len() - 1];
    let recipe = Recipe::parse(ops).unwrap();
    assert_eq!(recipe.print(), ops.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "printed back in place");
    let known: Vec<&str> = ops.iter().copied().filter(|o| !UNKNOWN.contains(o)).collect();
    let without = Recipe::parse(&known).unwrap();
    let photo = scene(61, 600, 400);
    for target in [Target::Full, Target::Fit { width: 160, height: 160 }] {
        let a = render(Source::upright(&photo), &recipe, target).unwrap();
        let b = render(Source::upright(&photo), &without, target).unwrap();
        assert!(a == b, "the unknown and legacy operations are skipped by the render ({target:?})");
    }
}
