//! The geometric operations on real pixels: every recipe of up to four rotations and flips, on a
//! `dalica-scenes` photo stored with each of the eight EXIF orientations, must give exactly the
//! picture that turning the upright photo step by step gives (an independent implementation
//! below), the right displayed size, and the stored fingerprint of its net transform
//! (`fingerprints/geometry-<transform>.json`, `architecture.md` §11). On a mismatch the actual
//! and expected PNGs go to `target/test-output/edit/`; `DALICA_BLESS=1` stores new fingerprints
//! (look at the PNGs first).

use std::collections::BTreeMap;
use std::path::PathBuf;

use dalica_edit::{Dihedral, Op, Recipe};
use dalica_image::{Orientation, Rgba8};
use dalica_scenes::{Rgb8, Scene, fingerprint, orient};

const GEOMETRY: [&str; 5] = ["rotate(90)", "rotate(180)", "rotate(270)", "flip_h()", "flip_v()"];

fn rgba(img: &Rgb8) -> Rgba8 {
    Rgba8 { width: img.width, height: img.height, data: img.data.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() }
}

fn px(img: &Rgba8, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.width + x) * 4) as usize;
    img.data[i..i + 4].try_into().unwrap()
}

/// Builds a `w × h` image whose pixel `(x, y)` is `f(x, y)`.
fn build(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Rgba8 {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            data.extend(f(x, y));
        }
    }
    Rgba8 { width: w, height: h, data }
}

/// The reference: one operation applied the obvious way, pixel by pixel.
fn step(img: &Rgba8, op: &str) -> Rgba8 {
    let (w, h) = (img.width, img.height);
    let cw = |i: &Rgba8| build(i.height, i.width, |x, y| px(i, y, i.height - 1 - x));
    match op {
        "rotate(90)" => cw(img),
        "rotate(180)" => build(w, h, |x, y| px(img, w - 1 - x, h - 1 - y)),
        "rotate(270)" => build(h, w, |x, y| px(img, w - 1 - y, x)),
        "flip_h()" => build(w, h, |x, y| px(img, w - 1 - x, y)),
        "flip_v()" => build(w, h, |x, y| px(img, x, h - 1 - y)),
        _ => panic!("not a geometric operation: {op}"),
    }
}

/// Every sequence of up to `max` operations from `GEOMETRY`, the empty one first.
fn recipes(max: usize) -> Vec<Vec<&'static str>> {
    let mut all = vec![vec![]];
    let mut last = vec![vec![]];
    for _ in 0..max {
        last = last.iter().flat_map(|r: &Vec<&str>| GEOMETRY.iter().map(move |op| [r.clone(), vec![*op]].concat())).collect();
        all.extend(last.iter().cloned());
    }
    all
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/edit");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn every_short_recipe_on_every_exif_orientation_is_the_exact_transform() {
    let up = Scene::new(29, 48, 32).render().unwrap();
    let upright = rgba(&up);
    let all = recipes(4);
    assert_eq!(all.len(), 1 + 5 + 25 + 125 + 625);
    // One output per transform of the upright photo, to fingerprint; every other output with that
    // transform must equal it exactly.
    let mut by_transform: BTreeMap<&str, Rgba8> = BTreeMap::new();
    let mut cases = 0;
    for o in 1..=8u8 {
        let exif = Orientation::new(o).unwrap();
        let stored = rgba(&orient::store(&up, o).unwrap());
        for ops in &all {
            let case = format!("EXIF {o} + {ops:?}");
            let recipe = Recipe::parse(ops).unwrap();
            let expected = ops.iter().fold(upright.clone(), |img, op| step(&img, op));
            let net = recipe.display(exif);
            assert_eq!(recipe.display_size(exif, stored.width, stored.height), (expected.width, expected.height), "{case}");
            let actual = net.apply(stored.clone()).unwrap();
            if actual != expected {
                let dir = out_dir();
                std::fs::write(dir.join("geometry-actual.png"), fingerprint::png_rgba(actual.width, actual.height, &actual.data)).unwrap();
                std::fs::write(dir.join("geometry-expected.png"), fingerprint::png_rgba(expected.width, expected.height, &expected.data)).unwrap();
                panic!("{case}: {} differs from the step-by-step reference (PNGs in {})", net.name(), dir.display());
            }
            // The same recipe built through the merge rules gives the same picture.
            let mut pushed = Recipe::new();
            for op in ops {
                pushed.push(Op::parse(op)).unwrap();
            }
            assert_eq!(pushed.display(exif), net, "{case}: pushed as {:?}", pushed.print());
            // The picture depends only on what the recipe does to the upright photo.
            let shown = recipe.orientation().name();
            match by_transform.get(shown) {
                Some(first) => assert_eq!(&actual, first, "{case}"),
                None => {
                    by_transform.insert(shown, actual);
                }
            }
            cases += 1;
        }
    }
    assert_eq!(cases, 8 * 781);
    assert_eq!(by_transform.len(), 8, "every transform is reached");
    for (name, img) in &by_transform {
        let case = format!("geometry-{name}");
        std::fs::write(out_dir().join(format!("{case}.png")), fingerprint::png_rgba(img.width, img.height, &img.data)).unwrap();
        let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{case}.json"));
        if std::env::var_os("DALICA_BLESS").is_some() {
            std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(img.width, img.height, &img.data)) + "\n").unwrap();
            println!("blessed {}", stored.display());
        }
        let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
        if let Err(m) = fingerprint::check(img.width, img.height, &img.data, &expected, 0.5) {
            std::fs::write(out_dir().join(format!("{case}-actual.png")), &m.actual_png).unwrap();
            panic!("{case}: {}", m.message);
        }
    }
}

#[test]
fn applying_a_transform_and_its_inverse_gives_the_picture_back() {
    let img = rgba(&Scene::new(5, 13, 7).render().unwrap());
    for d in Dihedral::ALL {
        let there = d.apply(img.clone()).unwrap();
        assert_eq!(there.width * there.height, img.width * img.height);
        assert_eq!(d.inverse().apply(there).unwrap(), img, "{}", d.name());
        // Applying two transforms one after the other is applying their composition.
        for e in Dihedral::ALL {
            let both = e.apply(d.apply(img.clone()).unwrap()).unwrap();
            assert_eq!(d.then(e).apply(img.clone()).unwrap(), both, "{} then {}", d.name(), e.name());
        }
    }
    // Four quarter turns and two identical flips are the identity.
    let four = Recipe::parse(["rotate(90)"; 4]).unwrap().orientation();
    assert_eq!(four.apply(img.clone()).unwrap(), img);
    for flip in ["flip_h()", "flip_v()"] {
        assert_eq!(Recipe::parse([flip, flip]).unwrap().orientation(), Dihedral::IDENTITY);
    }
}

#[test]
fn the_exif_orientation_alone_turns_stored_pixels_upright() {
    let up = Scene::new(3, 20, 12).render().unwrap();
    for o in 1..=8u8 {
        let stored = rgba(&orient::store(&up, o).unwrap());
        let shown = Recipe::new().display(Orientation::new(o).unwrap()).apply(stored).unwrap();
        assert_eq!(shown, rgba(&up), "EXIF {o}");
    }
}

#[test]
fn single_pixels_and_lines_turn_too() {
    for (w, h) in [(1, 1), (1, 5), (5, 1)] {
        let img = build(w, h, |x, y| [x as u8, y as u8, 7, 255]);
        for op in GEOMETRY {
            let d = Op::parse(op).orientation().unwrap();
            assert_eq!(d.apply(img.clone()).unwrap(), step(&img, op), "{w} × {h} {op}");
        }
    }
}
