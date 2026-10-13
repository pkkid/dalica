//! Edited photos in the ladder (`architecture.md` §6.3–§6.4, M7.9): what a recipe puts into the
//! cache key (the recipe without its rotations and flips, and the versions of the operations it
//! uses), and the scan and fetch jobs drawing a photo's entries with `edit::render` under that key,
//! beside the photo's unedited entries.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use dalica_edit::Target;
use dalica_image::Limits;
use dalica_thumbs::Rgba8;
use dalica_thumbs::ladder::{self, Edits, Render, Source};
use dalica_thumbs::pack::{Cache, Options, Size};

fn ops(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// A `scenes` JPEG of `w × h` in `dir`.
fn photo(dir: &Path, w: u32, h: u32) -> Source {
    let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(11, w, h), 90, None).unwrap();
    let path = dir.join("IMG_0001.jpg");
    std::fs::write(&path, &bytes).unwrap();
    Source { path, file_size: bytes.len() as u64, mtime_ms: 1_700_000_000_000 }
}

/// Mean absolute difference per channel of two images of one size.
fn difference(a: &Rgba8, b: &Rgba8) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let sum: u64 = a.data.chunks(4).zip(b.data.chunks(4)).map(|(p, q)| (0..3).map(|c| u64::from(p[c].abs_diff(q[c]))).sum::<u64>()).sum();
    sum as f64 / (f64::from(a.width) * f64::from(a.height) * 3.0)
}

#[test]
fn only_operations_that_change_pixels_make_a_photo_edited() {
    assert_eq!(Edits::of(&[]), None);
    assert_eq!(Edits::of(&ops(&["rotate(90)", "flip_h()"])), None, "the cells turn the picture themselves");
    let e = Edits::of(&ops(&["rotate(90)", "autofix()", "flip_v()"])).unwrap();
    assert_eq!(e.recipe.print(), ops(&["autofix()"]));
    assert_eq!(e, Edits::of(&ops(&["autofix()"])).unwrap(), "rotations do not change the key");
    // Unknown and legacy entries are edits too: they are kept, and a newer Dalica may draw them.
    assert!(Edits::of(&ops(&["someday(1)"])).is_some());
    assert!(Edits::of(&ops(&["legacy:enhance=1"])).is_some());
    // A recipe that cannot be read shows the photo as its file has it.
    assert_eq!(Edits::of(&vec!["autofix()".to_string(); dalica_edit::MAX_OPERATIONS + 1]), None);
}

#[test]
fn the_key_holds_the_recipe_and_the_versions_of_the_operations_it_uses() {
    let dir = tempfile::tempdir().unwrap();
    let src = photo(dir.path(), 64, 48);
    let render = Render::default();
    assert_eq!(src.key_edited(&render, None), src.key(), "unedited photos keep their keys");
    let fix = Edits::of(&ops(&["autofix()"])).unwrap();
    let color = Edits::of(&ops(&["autocolor()"])).unwrap();
    let (k_fix, k_color) = (src.key_edited(&render, Some(&fix)), src.key_edited(&render, Some(&color)));
    assert_ne!(k_fix, src.key());
    assert_ne!(k_fix, k_color);
    assert_ne!(k_fix, src.key_edited(&Render { color_managed: false }, Some(&fix)), "the render options still count");
    // A new version of `autofix` changes the key of the photos that use it, and no other.
    let bumped = |name: &str| dalica_edit::ops::version(name).map(|v| if name == "autofix" { v + 1 } else { v });
    let fix2 = Edits::with_versions(&ops(&["autofix()"]), bumped).unwrap();
    let color2 = Edits::with_versions(&ops(&["autocolor()"]), bumped).unwrap();
    assert_ne!(src.key_edited(&render, Some(&fix2)), k_fix);
    assert_eq!(src.key_edited(&render, Some(&color2)), k_color);
    assert_eq!(Edits::with_versions(&ops(&["rotate(90)"]), bumped), None);
}

#[test]
fn scan_and_fetch_draw_an_edited_photo_with_the_renderer_beside_its_unedited_entries() {
    let dir = tempfile::tempdir().unwrap();
    let src = photo(dir.path(), 900, 600);
    let cache = Cache::open(&dir.path().join("thumbs"), Options::default()).unwrap();
    let (render, limits, cancel) = (Render::default(), Limits::default(), AtomicBool::new(false));
    let plain = ladder::scan(&cache, &src, true, &limits, &cancel).unwrap();
    let edits = Edits::of(&ops(&["rotate(270)", "autofix()", "someday(2)"])).unwrap();
    let scanned = ladder::scan_edited(&cache, &src, &render, Some(&edits), true, &limits, &cancel).unwrap();
    assert!(scanned.decoded, "the edited entries are made, not the unedited ones read");
    let key = src.key_edited(&render, Some(&edits));
    for size in ladder::SCAN_SIZES {
        assert!(cache.contains(size, key) && cache.contains(size, src.key()), "{size:?}: both kept");
    }
    // The entry is the renderer's picture at that size (through JPEG): the photo decoded and drawn
    // with the recipe, unturned.
    let full = dalica_image::decode(&std::fs::read(&src.path).unwrap(), &limits).unwrap();
    let oracle = dalica_edit::render(dalica_edit::Source::upright(&full), &edits.recipe, Target::Fit { width: 256, height: 256 }).unwrap();
    let (s256, entry) = scanned.images.iter().find(|(s, _)| *s == Size::S256).unwrap();
    assert_eq!(*s256, Size::S256);
    let d = difference(entry, &oracle);
    assert!(d < 3.0, "the 256 entry is edit::render at 256 (mean difference {d:.2})");
    assert_eq!(
        entry,
        &ladder::render_edited(&src, &render, &edits, 256, &limits)
            .map(|i| ladder::decode_entry(&ladder::encode_jpeg(&i).unwrap(), &limits).unwrap())
            .unwrap()
    );
    assert!(scanned.sketch.is_some() && plain.sketch.is_some());
    // Fetch: from the cache under the edited key, else drawn and stored.
    let f = ladder::fetch_edited(&cache, &src, &render, Some(&edits), Size::S256, 2560, &limits, &cancel).unwrap();
    assert!(!f.decoded);
    let f = ladder::fetch_edited(&cache, &src, &render, Some(&edits), Size::S512, 2560, &limits, &cancel).unwrap();
    assert!(f.decoded && f.image.width.max(f.image.height) == 512);
    assert!(cache.contains(Size::S512, key) && !cache.contains(Size::S512, src.key()));
    let oracle = dalica_edit::render(dalica_edit::Source::upright(&full), &edits.recipe, Target::Fit { width: 512, height: 512 }).unwrap();
    let d = difference(&f.image, &oracle);
    assert!(d < 3.0, "the 512 entry is edit::render at 512 (mean difference {d:.2})");
}

#[test]
fn an_edited_render_is_never_enlarged_and_hostile_sizes_do_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let src = photo(dir.path(), 90, 60);
    let edits = Edits::of(&ops(&["autocontrast()"])).unwrap();
    let img = ladder::render_edited(&src, &Render::default(), &edits, 256, &Limits::default()).unwrap();
    assert_eq!((img.width, img.height), (90, 60), "a small photo stays its size");
    for side in [0, 1, u32::MAX] {
        let r = ladder::render_edited(&src, &Render::default(), &edits, side, &Limits::default());
        if let Ok(img) = r {
            assert!(img.width.max(img.height) <= 90 && img.width >= 1);
        }
    }
    // A missing file is the photo's error, as for an unedited one.
    let gone = Source { path: dir.path().join("gone.jpg"), ..src.clone() };
    assert!(matches!(ladder::render_edited(&gone, &Render::default(), &edits, 128, &Limits::default()), Err(ladder::Error::Photo(_))));
}
