//! The effects' primitives (`dalica_edit::fx`, M12.3, made fast in M13.34): the blur and the grid
//! blur, the seeded noise and its rows, the masks, the brush dabs, the blends and Fade, and luma and
//! linear light and their tables, each on its own; the effects that use them are tested in their own
//! files.

use dalica_edit::fx::{self, Blend, Circle, Dabs, Edges, Gaussian, Gradient, GridBlur, Mono, Noise, Plane, Space, Window};
use dalica_edit::{Geometry, Point};

use crate::common::{exif, recipe};

/// A `w × h` picture with detail everywhere (values in 0..1).
fn picture(w: u32, h: u32) -> Plane {
    Plane::from_fn(0, 0, w, h, |x, y| {
        let v = |k: u32| ((x * 37 + y * 11 + k * 101) % 97) as f32 / 96.0;
        [v(0), v(1), ((x / 4 + y / 3) % 2) as f32]
    })
    .unwrap()
}

/// The space of a whole render of a `w × h` upright photo with `ops` at `scale`.
fn space(ops: &[&str], o: u8, w: u32, h: u32, scale: f64) -> (Space, Geometry) {
    let g = Geometry::new(&recipe(ops), exif(o), w, h);
    let (ow, oh) = g.output_size(scale).unwrap();
    (Space::from_geometry(&g, ow, oh).unwrap(), g)
}

#[test]
fn the_blur_keeps_a_flat_plane() {
    let mut flat = Plane::from_fn(0, 0, 31, 17, |_, _| [0.25, 0.5, 1.0]).unwrap();
    assert!(Gaussian::new(3.0, (31, 17)).apply(&mut flat));
    assert!(flat.data.iter().all(|p| (p[0] - 0.25).abs() < 1e-4 && (p[1] - 0.5).abs() < 1e-4 && (p[2] - 1.0).abs() < 1e-4));
}

#[test]
fn the_blur_spreads_a_dot_by_its_sigma() {
    let n = 121u32;
    let c = n / 2;
    for sigma in [1.5, 4.0, 9.0] {
        let mut dot = Plane::from_fn(0, 0, n, n, |x, y| if (x, y) == (c, c) { [1.0; 3] } else { [0.0; 3] }).unwrap();
        let g = Gaussian::new(sigma, (n, n));
        assert!(g.apply(&mut dot));
        let (mut sum, mut var) = (0.0f64, 0.0f64);
        for y in 0..n {
            for x in 0..n {
                let v = f64::from(dot.get(i64::from(x), i64::from(y))[0]);
                sum += v;
                var += v * (f64::from(x) - f64::from(c)).powi(2);
            }
        }
        assert!((sum - 1.0).abs() < 1e-3, "{sigma}: the dot's weight {sum}");
        let spread = (var / sum).sqrt();
        assert!((spread - g.sigma()).abs() < 0.02 * g.sigma(), "{sigma}: spread {spread} for {}", g.sigma());
        assert!((g.sigma() - sigma).abs() < 0.6, "{sigma}: boxes make {}", g.sigma());
        // About 3σ: the boxes' radii added up.
        assert!((f64::from(g.reach()) - 3.0 * sigma).abs() <= 2.0, "{sigma}: reach {}", g.reach());
    }
}

/// A region cut from the picture with the blur's reach as margin (cut to the picture) blurs to
/// exactly the whole picture's blur inside it: at a corner, an edge and in the middle.
#[test]
fn the_blur_of_a_region_with_its_margin_equals_the_whole_blur() {
    let (w, h) = (90u32, 70u32);
    let source = picture(w, h);
    for sigma in [0.8, 3.5, 12.0] {
        let g = Gaussian::new(sigma, (w, h));
        let mut whole = source.clone();
        assert!(g.apply(&mut whole));
        let m = g.reach();
        for (x0, y0, rw, rh) in [(0, 0, 13, 9), (40, 0, 20, 30), (31, 22, 25, 17), (70, 50, 20, 20)] {
            let (ax, ay) = (x0 - m.min(x0), y0 - m.min(y0));
            let (bx, by) = ((x0 + rw + m).min(w), (y0 + rh + m).min(h));
            let mut part = Plane::from_fn(ax, ay, bx - ax, by - ay, |x, y| source.get(i64::from(x), i64::from(y))).unwrap();
            assert!(g.apply(&mut part));
            for y in y0..y0 + rh {
                for x in x0..x0 + rw {
                    let (x, y) = (i64::from(x), i64::from(y));
                    assert_eq!(part.get(x, y), whole.get(x, y), "sigma {sigma}, region at ({x0}, {y0}), pixel ({x}, {y})");
                }
            }
        }
    }
}

/// M12's box passes, kept as the oracle of the fast ones (M13.34): each value to fixed point, the
/// three passes along every row by a bounds-checked running sum in `i64`, the plane turned, the
/// passes along every row again, and turned back.
mod m12 {
    const ONE: f32 = 1_048_576.0;
    const LIMIT: f32 = 2_000.0;

    fn to_fixed(v: f32) -> i32 {
        let v = if v.is_nan() { 0.0 } else { v.clamp(-LIMIT, LIMIT) * ONE };
        (if v >= 0.0 { v + 0.5 } else { v - 0.5 }) as i32
    }

    fn box_line<const C: usize>(line: &mut [[i32; C]], r: usize) {
        let n = line.len();
        if r == 0 || n == 0 {
            return;
        }
        let tmp = line.to_vec();
        let last = n - 1;
        let at = |i: usize| tmp[i.min(last)];
        let count = (2 * r + 1) as i64;
        let first = at(0);
        let mut acc: [i64; C] = std::array::from_fn(|c| i64::from(first[c]) * (r as i64 + 1));
        for i in 1..=r.min(last) {
            let v = at(i);
            for c in 0..C {
                acc[c] += i64::from(v[c]);
            }
        }
        if r > last {
            let v = at(last);
            for c in 0..C {
                acc[c] += i64::from(v[c]) * (r - last) as i64;
            }
        }
        for (x, out) in line.iter_mut().enumerate() {
            *out = std::array::from_fn(|c| ((acc[c] * 2 + count).div_euclid(count * 2)).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32);
            let (add, sub) = (at(x.saturating_add(r + 1)), at(x.saturating_sub(r)));
            for c in 0..C {
                acc[c] += i64::from(add[c]) - i64::from(sub[c]);
            }
        }
    }

    fn transpose<const C: usize>(data: &[[i32; C]], width: usize, height: usize) -> Vec<[i32; C]> {
        (0..width).flat_map(|x| (0..height).map(move |y| data[y * width + x])).collect()
    }

    /// `data` (`width × height` pixels of `C` channels) blurred by boxes of `radii`.
    pub fn blur<const C: usize>(data: &[[f32; C]], width: usize, height: usize, radii: [usize; 3]) -> Vec<[f32; C]> {
        if radii == [0; 3] {
            return data.to_vec();
        }
        let mut fixed: Vec<[i32; C]> = data.iter().map(|p| p.map(to_fixed)).collect();
        for row in fixed.chunks_mut(width) {
            for r in radii {
                box_line(row, r);
            }
        }
        let mut turned = transpose(&fixed, width, height);
        for row in turned.chunks_mut(height) {
            for r in radii {
                box_line(row, r);
            }
        }
        transpose(&turned, height, width).iter().map(|q| q.map(|v| v as f32 / ONE)).collect()
    }
}

/// A seeded generator of test values (xorshift).
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Mostly working values, now and then far outside them (and a `NaN` or an infinity).
    fn value(&mut self) -> f32 {
        match self.next() % 200 {
            0 => f32::NAN,
            1 => f32::INFINITY,
            2 => -1e30,
            3 => 1999.5,
            4 => -2000.0,
            _ => (self.next() % 1_000_000) as f32 / 1e6 * 1.4 - 0.2,
        }
    }
}

/// The fast box passes give exactly M12's planes, RGB and one channel, on seeded planes of odd
/// sizes with radii up to the plane's size and past it.
#[test]
fn the_box_passes_give_m12_planes_bit_for_bit() {
    let mut rng = Seeded(0x9e37_79b9_7f4a_7c15);
    for (w, h) in [(1u32, 1u32), (1, 23), (29, 1), (7, 5), (17, 33), (61, 19), (40, 40), (97, 3)] {
        let rgb = Plane::from_fn(0, 0, w, h, |_, _| [0.0; 3]).unwrap();
        let rgb = Plane { data: rgb.data.iter().map(|_| [rng.value(), rng.value(), rng.value()]).collect(), ..rgb };
        let mono = Mono { x0: 0, y0: 0, width: w, height: h, data: (0..w * h).map(|_| rng.value()).collect() };
        let longest = f64::from(w.max(h));
        for sigma in [0.5, 0.9, 1.7, 3.0, 6.5, longest / 3.0, longest / 2.0, longest, longest * 1.7] {
            let g = Gaussian::new(sigma, (100_000, 100_000));
            let (w, h) = (w as usize, h as usize);
            let mut fast = rgb.clone();
            assert!(g.apply(&mut fast));
            let bits = |d: &[[f32; 3]]| d.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>();
            assert!(bits(&fast.data) == bits(&m12::blur(&rgb.data, w, h, g.radii())), "{w} × {h}, radii {:?}", g.radii());
            let mut one = mono.clone();
            assert!(g.apply_mono(&mut one));
            let want: Vec<f32> = m12::blur(&mono.data.iter().map(|v| [*v]).collect::<Vec<_>>(), w, h, g.radii()).into_iter().map(|[v]| v).collect();
            assert!(one.data.iter().map(|v| v.to_bits()).eq(want.iter().map(|v| v.to_bits())), "one channel, {w} × {h}, radii {:?}", g.radii());
        }
    }
}

/// `w × h` RGBA8 pixels with detail everywhere.
fn rgba(w: u32, h: u32, seed: u64) -> Vec<u8> {
    let mut rng = Seeded(seed);
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let base = ((x * 7 + y * 3) % 200) as u8;
            [base.wrapping_add((rng.next() % 56) as u8), ((x / 9 + y / 7) % 2 * 255) as u8, (rng.next() % 256) as u8, 255]
        })
        .collect()
}

/// The window at `(x0, y0)` of `w × h` pixels cut from a whole `full` picture.
fn cut(whole: &[u8], full: (u32, u32), x0: u32, y0: u32, w: u32, h: u32) -> Vec<u8> {
    (y0..y0 + h).flat_map(|y| whole[((y * full.0 + x0) * 4) as usize..((y * full.0 + x0 + w) * 4) as usize].to_vec()).collect()
}

/// The grid blur of a region window with its reach as margin (cut to the picture) reads exactly
/// the whole render's grid blur inside the region, for every form (RGB, weighted, luma), at the
/// corners, an edge and in the middle; a row read equals the pixels read one by one.
#[test]
fn the_grid_blur_of_a_region_with_its_margin_equals_the_whole_render() {
    let full = (330u32, 230u32);
    let whole = rgba(full.0, full.1, 7);
    let all = Window::new(&whole, 0, 0, full.0, full.1, full).unwrap();
    let highlight = |p: [f32; 3]| {
        let k = fx::smoothstep(0.5, 1.0, f64::from(fx::luma(p))) as f32;
        p.map(|v| v * k)
    };
    for sigma in [0.5, 2.0, 7.5, 13.0, 40.0, 75.0] {
        let blur = || GridBlur::new(sigma, full).unwrap();
        let (b_rgb, b_weighted, b_luma) = (blur(), blur(), blur());
        let (rgb, weighted, luma) = (b_rgb.rgb(all).unwrap(), b_weighted.rgb_weighted(all, highlight).unwrap(), b_luma.luma(all).unwrap());
        let m = b_rgb.reach();
        for (x0, y0, rw, rh) in [(0, 0, 37, 29), (101, 0, 50, 31), (77, 91, 61, 43), (300, 190, 30, 40), (5, 150, 17, 80)] {
            let (ax, ay) = (x0 - m.min(x0), y0 - m.min(y0));
            let (bx, by) = ((x0 + rw + m).min(full.0), (y0 + rh + m).min(full.1));
            let part = cut(&whole, full, ax, ay, bx - ax, by - ay);
            let window = Window::new(&part, ax, ay, bx - ax, by - ay, full).unwrap();
            let (p_rgb, p_weighted, p_luma) = (blur(), blur(), blur());
            let (rgb_r, weighted_r, luma_r) =
                (p_rgb.rgb(window).unwrap(), p_weighted.rgb_weighted(window, highlight).unwrap(), p_luma.luma(window).unwrap());
            for y in y0..y0 + rh {
                let mut row = vec![[0.0f32; 3]; rw as usize];
                p_rgb.read_row(rgb_r, x0, y, &mut row);
                for x in x0..x0 + rw {
                    let at = format!("sigma {sigma}, region at ({x0}, {y0}), pixel ({x}, {y})");
                    assert_eq!(p_rgb.read(rgb_r, x, y), b_rgb.read(rgb, x, y), "{at}");
                    assert_eq!(row[(x - x0) as usize], b_rgb.read(rgb, x, y), "a row read, {at}");
                    assert_eq!(p_weighted.read(weighted_r, x, y), b_weighted.read(weighted, x, y), "weighted, {at}");
                    assert_eq!(p_luma.read_luma(luma_r, x, y), b_luma.read_luma(luma, x, y), "luma, {at}");
                }
            }
        }
        // The luma grid is the luma of the RGB grid (within rounding); a later call reuses the grid.
        for (x, y) in [(0, 0), (100, 50), (329, 229)] {
            assert!((b_luma.read_luma(luma, x, y) - fx::luma(b_rgb.read(rgb, x, y))).abs() < 1e-5);
        }
        assert!(std::ptr::eq(b_rgb.rgb(all).unwrap(), rgb));
    }
}

/// A dot blurred on the grid spreads by the grid's sigma (within the box passes' 2 %), wherever it
/// sits in its cell, and that sigma is the one asked for within the box passes' tolerance at the
/// grid's scale; a blur under the smallest box blur is mixed in by its share.
#[test]
fn the_grid_blur_spreads_a_dot_by_its_sigma() {
    let n = 801u32;
    for sigma in [0.5, 2.0, 5.0, 12.0, 30.0, 70.0] {
        let blur = GridBlur::new(sigma, (n, n)).unwrap();
        // The box passes make their sigma within 0.6 of a step of it; on the grid a step is a cell.
        assert!((blur.sigma() - sigma).abs() < 0.6 * f64::from(blur.cell()), "{sigma}: the grid makes {}", blur.sigma());
        for offset in [0, 1, blur.cell() / 2] {
            let c = n / 2 + offset;
            let mut data = vec![0u8; (n * n * 4) as usize];
            data[((c * n + c) * 4) as usize] = 255;
            let blur = GridBlur::new(sigma, (n, n)).unwrap();
            let grid = blur.rgb(Window::new(&data, 0, 0, n, n, (n, n)).unwrap()).unwrap();
            let (mut sum, mut var) = (0.0f64, 0.0f64);
            let mut row = vec![[0.0f32; 3]; n as usize];
            for y in 0..n {
                blur.read_row(grid, 0, y, &mut row);
                for (x, p) in row.iter().enumerate() {
                    let input = if (x as u32, y) == (c, c) { [1.0, 0.0, 0.0] } else { [0.0; 3] };
                    let v = f64::from(blur.mix(input, *p)[0]);
                    sum += v;
                    var += v * (x as f64 - f64::from(c)).powi(2);
                }
            }
            // One 8-bit dot spread thin: the fixed point's rounding moves its weight a little.
            assert!((sum - 1.0).abs() < 0.01, "{sigma}: the dot's weight {sum}");
            let spread = (var / sum).sqrt();
            assert!((spread - blur.sigma()).abs() < 0.02 * blur.sigma(), "{sigma}, offset {offset}: spread {spread} for {}", blur.sigma());
        }
    }
    assert!(GridBlur::new(0.0, (n, n)).is_none() && GridBlur::new(f64::NAN, (n, n)).is_none());
    assert!(GridBlur::new(0.4, (n, n)).unwrap().strength() < 1.0);
}

/// The row sampler gives the noise of [`Noise::at`] (and the field of [`Noise::value`]) within
/// 10⁻⁴ over seeded rows of plain, turned, straightened and cropped spaces.
#[test]
fn the_noise_by_rows_agrees_with_the_noise_at_each_pixel() {
    let mut rng = Seeded(31);
    let spaces = [
        space(&[], 1, 640, 480, 1.0),
        space(&["straighten(7)"], 1, 640, 480, 0.5),
        space(&["crop(0.1,0.2,0.9,0.8)", "straighten(-4)"], 6, 900, 600, 0.75),
        space(&["straighten(45)", "rotate(90)"], 8, 300, 500, 2.0),
    ];
    for (s, _) in &spaces {
        let (w, h) = s.size();
        for cell in [0.0012, 0.004, 0.02, 1e-6, 1.0] {
            let noise = Noise::new(rng.next() as u32, cell);
            for _ in 0..12 {
                let (x0, y) = ((rng.next() % u64::from(w)) as u32, (rng.next() % u64::from(h)) as u32);
                let n = (w - x0) as usize;
                let channel = (rng.next() % 3) as u32;
                let (mut row, mut values) = (vec![0.0f32; n], vec![0.0f32; n]);
                noise.row(s, x0, y, channel, &mut row);
                noise.row_values(s, x0, y, channel, &mut values);
                for (i, (r, v)) in row.iter().zip(&values).enumerate() {
                    let x = x0 + i as u32;
                    let at = noise.at(s, x, y, channel);
                    assert!((r - at).abs() < 1e-4, "cell {cell}, ({x}, {y}): {r} vs {at}");
                    let (u, w) = s.pixel_units(x, y);
                    let field = noise.value(u, w, channel);
                    assert!((v - field).abs() < 1e-4, "cell {cell}, ({x}, {y}): {v} vs {field}");
                }
            }
        }
    }
}

/// The shared linear-light tables are within a hundredth of a level of the curves.
#[test]
fn the_linear_light_tables_are_within_a_hundredth_of_a_level() {
    let lin = fx::linear_light();
    let worst = (0..=100_000).map(|i| i as f32 / 100_000.0).fold(0.0f32, |w, v| {
        let d = (lin.to_linear(v) - fx::to_linear(v)).abs().max((lin.to_srgb(v) - fx::to_srgb(v)).abs());
        w.max(d)
    });
    assert!(worst * 255.0 < 0.01, "{} of a level", worst * 255.0);
    // Inputs outside 0..1 (and `NaN`) read the ends.
    assert_eq!(lin.to_linear3([-1.0, 2.0, f32::NAN]), [lin.to_linear(0.0), lin.to_linear(1.0), lin.to_linear(0.0)]);
    assert_eq!(lin.to_srgb3([-1.0, 2.0, f32::NAN]), [lin.to_srgb(0.0), lin.to_srgb(1.0), lin.to_srgb(0.0)]);
}

/// The noise at a stored-frame point is the same in a render three times larger (the pixel whose
/// centre is that point), on a cropped and straightened photo, and the same in any window of a
/// render (it depends on the pixel of the whole output only).
#[test]
fn the_noise_at_a_stored_point_is_the_same_at_two_scales_and_in_a_region() {
    let ops = ["crop(0.1,0.2,0.9,0.8)", "straighten(4)"];
    let (small, _) = space(&ops, 6, 900, 600, 0.25);
    let (large, _) = space(&ops, 6, 900, 600, 0.75);
    let noise = Noise::new(fx::noise::seed("grain(10,false)"), 0.02);
    let (w, h) = small.size();
    for (x, y) in (0..w).step_by(7).flat_map(|x| (0..h).step_by(5).map(move |y| (x, y))) {
        let a = noise.at(&small, x, y, 0);
        let b = noise.at(&large, 3 * x + 1, 3 * y + 1, 0);
        assert!((a - b).abs() < 1e-4, "({x}, {y}): {a} vs {b}");
        // The same stored-frame point straight from the field.
        let p = small.to_stored(f64::from(x) + 0.5, f64::from(y) + 0.5);
        let (u, v) = small.units(p);
        assert_eq!(noise.value(u, v, 0), a);
    }
    // A region render prepares for the whole output: the same space, the same values.
    let (again, _) = space(&ops, 6, 900, 600, 0.25);
    assert_eq!(again, small);
}

#[test]
fn the_noise_has_mean_zero_and_unit_spread_and_differs_by_seed() {
    let (a, b) = (Noise::new(fx::noise::seed("a"), 0.01), Noise::new(fx::noise::seed("b"), 0.01));
    let samples: Vec<(f64, f64)> = (0..300).flat_map(|i| (0..300).map(move |j| (f64::from(i) * 0.00337, f64::from(j) * 0.00291))).collect();
    let n = samples.len() as f64;
    let va: Vec<f64> = samples.iter().map(|&(u, v)| f64::from(a.value(u, v, 0))).collect();
    let vb: Vec<f64> = samples.iter().map(|&(u, v)| f64::from(b.value(u, v, 0))).collect();
    let mean = va.iter().sum::<f64>() / n;
    let spread = (va.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    assert!(mean.abs() < 0.03, "mean {mean}");
    assert!((spread - 1.0).abs() < 0.05, "spread {spread}");
    let corr = va.iter().zip(&vb).map(|(x, y)| x * y).sum::<f64>() / n;
    assert!(corr.abs() < 0.05, "two seeds correlate by {corr}");
    // Each channel draws its own noise; the same seed draws the same.
    let other = samples.iter().filter(|&&(u, v)| a.value(u, v, 1) != a.value(u, v, 0)).count();
    assert!(other as f64 > 0.99 * n);
    assert_eq!(Noise::new(fx::noise::seed("a"), 0.01).value(0.123, 0.456, 2), a.value(0.123, 0.456, 2));
}

/// Where a pixel spans several cells, the noise shrinks by `cell / pixel`, as a large render's
/// grain does once it is shrunk.
#[test]
fn the_noise_shrinks_where_a_pixel_spans_several_cells() {
    let (fine, _) = space(&[], 1, 400, 400, 1.0);
    let noise = Noise::new(7, 1.0 / 1600.0);
    assert!((noise.attenuation(&fine) - 0.25).abs() < 1e-6);
    let coarse = Noise::new(7, 1.0 / 100.0);
    assert_eq!(coarse.attenuation(&fine), 1.0);
}

/// A circle is 1 at its centre and inside its hard core and 0 past its radius, on a photo stored
/// turned (EXIF 6), straightened, cropped and rotated, measured against the stored frame's own
/// pixels.
#[test]
fn a_circle_mask_is_one_at_its_centre_and_zero_past_its_feather() {
    let (rw, rh) = (1200u32, 900u32); // stored turned: the stored frame is 900 × 1200
    let (s, g) = space(&["straighten(-3)", "crop(0.05,0.1,0.95,0.8)", "rotate(90)"], 6, rw, rh, 0.5);
    let centre = Point::new(0.45, 0.4);
    let (radius, feather) = (0.12, 0.5);
    let circle = Circle::new(&s, centre, radius, feather);
    let (cx, cy) = s.to_output(centre);
    assert_eq!(circle.at(cx as u32, cy as u32), 1.0);
    let (w, h) = s.size();
    let (mut inside, mut outside) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let p = g.output_to_stored(Point::new((f64::from(x) + 0.5) / f64::from(w), (f64::from(y) + 0.5) / f64::from(h))).unwrap();
            // In the stored frame's pixels, relative to its shorter side (900).
            let d = ((p.x - centre.x) * 900.0).hypot((p.y - centre.y) * 1200.0) / 900.0;
            let m = circle.at(x, y);
            if d > radius + 1e-3 {
                assert_eq!(m, 0.0, "({x}, {y}) at {d}");
                outside += 1;
            } else if d < radius * (1.0 - feather) - 1e-3 {
                assert_eq!(m, 1.0, "({x}, {y}) at {d}");
                inside += 1;
            } else {
                assert!((0.0..=1.0).contains(&m));
            }
        }
    }
    assert!(inside > 100 && outside > 100, "{inside} in, {outside} out");
}

#[test]
fn a_gradient_rises_from_its_start_over_its_feather() {
    let (s, _) = space(&[], 1, 300, 200, 1.0);
    // Rising toward the top from the middle, over a tenth of the shorter side (20 px).
    let up = Gradient::new(&s, Point::new(0.5, 0.5), 0.0, 0.1);
    assert_eq!(up.at(150, 150), 0.0);
    assert_eq!(up.at(150, 100), 0.0);
    assert_eq!(up.at(150, 79), 1.0);
    assert_eq!(up.at(10, 5), 1.0);
    let mid = up.at(150, 89);
    assert!(mid > 0.2 && mid < 0.8, "{mid}");
    // 90° rises toward the right; a feather of 0 is a hard edge.
    let right = Gradient::new(&s, Point::new(0.5, 0.5), 90.0, 0.0);
    assert_eq!((right.at(149, 0), right.at(150, 199)), (0.0, 1.0));
}

#[test]
fn the_distance_from_the_edges_is_relative_to_the_shorter_side() {
    let (s, _) = space(&["crop(0,0,1,0.5)"], 1, 400, 400, 1.0); // a 400 × 200 output
    let edges = Edges::new(&s);
    assert!((edges.at(0, 100) - 0.5 / 200.0).abs() < 1e-12);
    assert!((edges.at(200, 99) - 99.5 / 200.0).abs() < 1e-12);
    assert!((edges.at(399, 199) - 0.5 / 200.0).abs() < 1e-12);
}

/// Dabs along a stroke of a few far-apart points cover every pixel near the path fully (no gaps
/// between dabs), nothing far from it; erasing along the same path takes it away again, and a band
/// paints as the whole picture does.
#[test]
fn dabs_cover_a_stroke_without_gaps() {
    let (s, _) = space(&[], 1, 400, 300, 1.0);
    let points = [Point::new(0.1, 0.2), Point::new(0.8, 0.3), Point::new(0.3, 0.9), Point::new(0.31, 0.9)];
    let stroke = Dabs::new(&s, &points, 0.02, 0.5, false);
    let r = stroke.radius();
    assert!((r - 6.0).abs() < 1e-9);
    let path: Vec<(f64, f64)> = points.iter().map(|p| s.to_output(*p)).collect();
    let dist = |x: f64, y: f64| {
        path.windows(2)
            .map(|w| {
                let ((ax, ay), (bx, by)) = (w[0], w[1]);
                let (dx, dy) = (bx - ax, by - ay);
                let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                (x - ax - t * dx).hypot(y - ay - t * dy)
            })
            .fold(f64::INFINITY, f64::min)
    };
    let whole = fx::paint(std::slice::from_ref(&stroke), 0.0, 0, 0, 400, 300).unwrap();
    let mut covered = 0;
    for y in 0..300u32 {
        for x in 0..400u32 {
            let d = dist(f64::from(x) + 0.5, f64::from(y) + 0.5);
            let v = whole[(y * 400 + x) as usize];
            assert_eq!(v, stroke.alpha(x, y));
            if d < 0.5 * r - 1.0 {
                assert_eq!(v, 1.0, "a gap at ({x}, {y}), {d} px from the path");
                covered += 1;
            } else if d > r + 1e-9 {
                assert_eq!(v, 0.0, "({x}, {y}) is {d} px from the path");
            }
        }
    }
    assert!(covered > 1000);
    // Erased along the same path: nothing left where it was fully painted, at most a quarter in
    // the feather (`s · (1 − s)`).
    let erased = fx::paint(&[stroke.clone(), Dabs::new(&s, &points, 0.02, 0.5, true)], 0.0, 0, 0, 400, 300).unwrap();
    assert!(erased.iter().zip(&whole).all(|(e, w)| if *w == 1.0 { *e == 0.0 } else { *e <= 0.25 + 1e-6 }));
    // A band of rows paints as the whole picture's rows.
    let band = fx::paint(std::slice::from_ref(&stroke), 0.0, 0, 64, 400, 32).unwrap();
    assert_eq!(band[..], whole[64 * 400..96 * 400]);
}

#[test]
fn fade_zero_gives_the_effect_and_a_hundred_the_input() {
    let (input, effect) = ([0.2, 0.4, 0.6], [1.0, 0.0, 0.5]);
    assert_eq!(fx::fade(input, effect, 0.0), effect);
    assert_eq!(fx::fade(input, effect, 100.0), input);
    let half = fx::fade(input, effect, 50.0);
    assert!((half[0] - 0.6).abs() < 1e-6 && (half[1] - 0.2).abs() < 1e-6 && (half[2] - 0.55).abs() < 1e-6);
    assert_eq!(fx::fade(input, effect, 250.0), input);
    assert_eq!(fx::fade(input, effect, -5.0), effect);
}

#[test]
fn the_blends_follow_their_formulas() {
    let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
    assert!(near(Blend::Normal.channel(0.3, 0.7), 0.7));
    assert!(near(Blend::Multiply.channel(0.5, 0.4), 0.2));
    assert!(near(Blend::Screen.channel(0.5, 0.5), 0.75));
    assert!(near(Blend::Overlay.channel(0.25, 0.5), 0.25));
    assert!(near(Blend::Overlay.channel(0.75, 0.5), 0.75));
    assert!(near(Blend::Overlay.channel(0.75, 1.0), 1.0));
    // Soft light: a mid-grey layer changes nothing, white lightens and black darkens, never to the
    // ends.
    for b in [0.1f32, 0.3, 0.6, 0.9] {
        assert!(near(Blend::SoftLight.channel(b, 0.5), b));
        let (up, down) = (Blend::SoftLight.channel(b, 1.0), Blend::SoftLight.channel(b, 0.0));
        assert!(up > b && up < 1.0 && down < b && down > 0.0, "{b}: {down} {up}");
    }
    // Every mode keeps black and white layers' order and stays in 0..1.
    for mode in Blend::ALL {
        for b in [0.0f32, 0.2, 0.5, 0.8, 1.0] {
            for t in [0.0f32, 0.3, 0.5, 0.9, 1.0] {
                let v = mode.channel(b, t);
                assert!((0.0..=1.0).contains(&v), "{mode:?}({b}, {t}) = {v}");
            }
        }
    }
    assert_eq!(fx::mix([0.0; 3], [1.0; 3], 0.25), [0.25; 3]);
}

#[test]
fn luma_and_linear_light() {
    assert!((fx::luma([1.0, 1.0, 1.0]) - 1.0).abs() < 1e-6);
    assert!((fx::luma([0.0, 1.0, 0.0]) - 0.587).abs() < 1e-6);
    assert!((fx::to_linear(0.5) - 0.214_041).abs() < 1e-5);
    assert!((fx::to_srgb(0.214_041) - 0.5).abs() < 1e-5);
    for i in 0..=255 {
        let v = i as f32 / 255.0;
        assert!((fx::to_srgb(fx::to_linear(v)) - v).abs() < 1e-5, "{v}");
    }
    assert_eq!(fx::to_linear(0.0), 0.0);
    assert!((fx::to_srgb(1.0) - 1.0).abs() < 1e-6);
}

/// Zero, huge, infinite and `NaN` sizes and numbers answer without a panic and without a huge
/// allocation.
#[test]
fn hostile_sizes_answer_without_a_panic() {
    let (s, g) = space(&[], 1, 64, 48, 1.0);
    let mut img = picture(16, 12);
    for sigma in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e300, f64::MIN_POSITIVE] {
        for full in [(0, 0), (16, 12), (u32::MAX, u32::MAX)] {
            let gaussian = Gaussian::new(sigma, full);
            assert!(gaussian.apply(&mut img), "{sigma} {full:?}");
            assert!(gaussian.reach() < 1 << 18);
        }
    }
    assert!(img.data.iter().flatten().all(|v| v.is_finite()));
    assert!(Plane::from_fn(0, 0, 0, 10, |_, _| [0.0; 3]).is_none());
    assert!(Plane::from_fn(0, 0, u32::MAX, u32::MAX, |_, _| [0.0; 3]).is_none());
    let mut bad = Plane { x0: 0, y0: 0, width: 5, height: 5, data: vec![[f32::NAN, f32::INFINITY, -1e30]; 3] };
    assert!(!Gaussian::new(2.0, (5, 5)).apply(&mut bad));
    let mut wild = Plane::from_fn(0, 0, 9, 9, |x, _| [f32::NAN, f32::INFINITY, if x % 2 == 0 { 1e30 } else { -1e30 }]).unwrap();
    assert!(Gaussian::new(2.0, (9, 9)).apply(&mut wild));
    assert!(wild.data.iter().flatten().all(|v| v.is_finite()));

    assert!(Space::from_geometry(&g, 0, 48).is_none());
    assert_eq!(s.length(f64::NAN), 0.0);
    assert!(s.length(f64::INFINITY).is_finite() && s.length(1e300).is_finite());
    for cell in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e300] {
        let n = Noise::new(1, cell);
        for (u, v) in [(0.5, 0.5), (f64::NAN, 0.0), (f64::INFINITY, 1.0), (1e300, -1e300)] {
            assert!(n.value(u, v, 0).is_finite());
        }
        assert!(n.at(&s, 63, 47, 2).is_finite());
    }
    let nan = Point::new(f64::NAN, f64::INFINITY);
    for (r, f) in [(f64::NAN, f64::NAN), (f64::INFINITY, 2.0), (-1.0, -1.0), (1e300, 0.5), (0.0, 0.0)] {
        assert!((0.0..=1.0).contains(&Circle::new(&s, Point::new(0.5, 0.5), r, f).at(3, 4)));
        assert_eq!(Circle::new(&s, nan, r, f).at(3, 4), 0.0);
        assert!((0.0..=1.0).contains(&Gradient::new(&s, nan, r, f).at(3, 4)));
        assert!((0.0..=1.0).contains(&Gradient::new(&s, Point::new(0.5, 0.5), r, f).at(3, 4)));
        let dabs = Dabs::new(&s, &[nan, Point::new(0.2, 0.2), Point::new(1e300, -1e300), Point::new(0.8, 0.9), nan], r, f, false);
        assert!(dabs.centres().len() <= fx::brush::MAX_DABS);
        let painted = fx::paint(&[dabs], f32::NAN, 0, 0, 64, 48).unwrap();
        assert!(painted.iter().all(|v| (0.0..=1.0).contains(v)));
    }
    let long = Dabs::new(&s, &[Point::new(-1e9, 0.5), Point::new(1e9, 0.5)], 0.0, 0.0, false);
    assert!(long.centres().len() <= fx::brush::MAX_DABS);
    assert!(Dabs::new(&s, &[], 0.1, 0.1, true).centres().is_empty());
    assert!(fx::paint(&[], 0.5, 0, 0, u32::MAX, u32::MAX).is_none());
    assert!(fx::paint(&[], 0.5, u32::MAX, u32::MAX, 0, 0).unwrap().is_empty());
    let odd = [f32::NAN, f32::INFINITY, -1.0];
    for mode in Blend::ALL {
        assert!(mode.apply(odd, odd).iter().all(|v| (0.0..=1.0).contains(v)));
    }
    let _ = fx::fade(odd, [0.5; 3], f64::NAN);
    assert_eq!((fx::to_linear(f32::NAN), fx::to_srgb(f32::NAN)), (0.0, 0.0));

    let px = vec![128u8; 16 * 12 * 4];
    for sigma in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e300, f64::MIN_POSITIVE, 1e9] {
        let _ = GridBlur::new(sigma, (u32::MAX, u32::MAX));
        if let Some(blur) = GridBlur::new(sigma, (16, 12)) {
            let grid = blur.rgb(Window::new(&px, 0, 0, 16, 12, (16, 12)).unwrap()).unwrap();
            assert!(blur.read(grid, 15, 11).iter().all(|v| (v - 128.0 / 255.0).abs() < 1e-4), "{sigma}");
            assert!(blur.reach() < u32::MAX && blur.sigma().is_finite());
        }
    }
    assert!(Window::new(&px, 0, 0, 16, 11, (16, 12)).is_none());
    assert!(Window::new(&px, 1, 0, 16, 12, (16, 12)).is_none());
    assert!(Window::new(&[], 0, 0, 0, 0, (16, 12)).is_none());
    assert!(Window::new(&px, u32::MAX, u32::MAX, 16, 12, (u32::MAX, u32::MAX)).is_none());
    for cell in [0.0, f64::NAN, f64::INFINITY] {
        let mut out = vec![9.0f32; 5];
        Noise::new(1, cell).row(&s, u32::MAX - 2, u32::MAX, 0, &mut out);
        assert!(out.iter().all(|v| v.is_finite() && v.abs() < 3.0));
    }
    Noise::new(1, 0.1).row(&s, 0, 0, 0, &mut []);
}

/// A pass's shared blur ([`fx::Blurred`], here Glow's) is made once while bands run: a band
/// thread making it never runs another band meanwhile (one waiting on this blur, or on another
/// render's blur whose maker waits on this one). Before the blur had a pool of its own, two
/// threads rendering into one small pool hung.
#[test]
fn blurs_shared_by_bands_never_wait_on_each_other() {
    use dalica_edit::{Source, Target, render};
    use std::sync::Arc;
    let img = Arc::new(crate::common::scene(9, 900, 600));
    let pool = Arc::new(rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap());
    let (done, wait) = std::sync::mpsc::channel();
    for ops in [["glow(1,30)", "glow(0.65,3)", "glow(1,0.3)"], ["glow(0.65,3)", "glow(1,0.3)", "glow(1,30)"]] {
        let (img, pool, done) = (img.clone(), pool.clone(), done.clone());
        std::thread::spawn(move || {
            for _ in 0..4 {
                for op in ops {
                    let a = pool.install(|| render(Source::upright(&img), &recipe(&[op]), Target::Full).unwrap());
                    let b = pool.install(|| render(Source::upright(&img), &recipe(&[op]), Target::Fit { width: 300, height: 300 }).unwrap());
                    let _ = done.send((op, a, b));
                }
            }
        });
    }
    let mut seen = std::collections::HashMap::new();
    for _ in 0..24 {
        let (op, a, b) = wait.recv_timeout(std::time::Duration::from_secs(30)).expect("the renders finish (no band waits on a blur it is making)");
        assert_eq!(seen.entry(op).or_insert_with(|| (a.clone(), b.clone())), &(a, b), "{op} renders the same every time");
    }
}
