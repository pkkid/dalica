//! The edit view's preview renders (`roadmap.md` M7.11; `editing.md` §3, §5.2; `architecture.md`
//! §5–§6) on temporary libraries of `scenes` photos: the preview has the recipe's displayed size
//! and equals the renderer's output; a newer request supersedes an older one; a tool's draft comes
//! before its final; a refined window equals that window of a full render; a prefetched neighbour
//! opens without a decode; the histogram equals a direct count; closing cancels; a history step
//! renders again; the owner thread makes no `fs` call; graceful failures; a frame's draft renders
//! from the halved base (M13.35).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dalica_core::{PhotoId, SystemClock};
use dalica_edit::{Recipe, Region, Target};
use dalica_engine::preview::halve;
use dalica_engine::{CacheDir, Histogram, Options, PreviewError, Quality, Session, ThumbOptions, UpdateKind};
use dalica_fs::Expect;
use dalica_image::Rgba8;
use serde_json::json;

const W: u32 = 640;
const H: u32 = 480;
const WAIT: Duration = Duration::from_secs(60);

fn photo_path(folder: &Path, i: usize) -> PathBuf {
    folder.join(format!("IMG_{:04}.jpg", i + 1))
}

/// A library watching `Porto` with `n` photos of `W × H`; ids in name order.
fn setup(n: usize) -> (tempfile::TempDir, PathBuf, Session, Vec<PhotoId>) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Porto");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64 + 7, W, H), 85, None).unwrap();
        dalica_fs::replace(&photo_path(&folder, i), &bytes, Expect::Absent).unwrap();
    }
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 2, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    let ids = ids.into_iter().map(|(_, id)| id).collect();
    (dir, folder, s, ids)
}

fn apply(s: &mut Session, id: PhotoId, op: &str) {
    s.execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
}

/// The photo decoded whole, upright (what the preview's sources are made from at this size).
fn decoded(folder: &Path, i: usize) -> Rgba8 {
    let bytes = std::fs::read(photo_path(folder, i)).unwrap();
    dalica_image::decode(&bytes, &dalica_image::Limits::default()).unwrap()
}

fn recipe(texts: &[&str]) -> Recipe {
    Recipe::parse(texts).unwrap()
}

fn strings(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|t| t.to_string()).collect()
}

/// `roadmap.md` M7.11: the preview has the recipe's displayed size (a rotation turns it) and is
/// the renderer's output for the photo and its recipe fitted to the box, never enlarged.
#[test]
fn the_preview_has_the_recipes_displayed_size() {
    let (_dir, folder, mut s, ids) = setup(1);
    apply(&mut s, ids[0], "rotate(90)");
    apply(&mut s, ids[0], "tuning(0.3,0,0,0.2,none)");
    s.open_preview(ids[0], (300, 300)).unwrap();
    assert!(s.wait_preview(WAIT));
    let frame = s.preview().expect("a preview").clone();
    assert_eq!((frame.image.width, frame.image.height), (240, 320), "480 × 640 displayed, halved once: the largest power of two covering 300 × 300");
    assert_eq!(frame.quality, Quality::Final);
    assert_eq!(frame.recipe, strings(&["rotate(90)", "tuning(0.3,0,0,0.2,none)"]));
    let source = decoded(&folder, 0);
    let half = halve(&source).unwrap();
    let expected =
        dalica_edit::render(dalica_edit::Source::upright(&half), &recipe(&["rotate(90)", "tuning(0.3,0,0,0.2,none)"]), Target::Full).unwrap();
    assert!(*frame.image == expected, "the preview is the renderer's output");
    // A tool's candidate: the renderer's picture of the halved source.
    let candidate = strings(&["rotate(90)", "tuning(0.5,0.1,0,-0.3,none)"]);
    s.set_preview_candidate(Some(&candidate)).unwrap();
    assert!(s.wait_preview(WAIT));
    let again = s.preview().unwrap();
    let expected =
        dalica_edit::render(dalica_edit::Source::upright(&half), &recipe(&["rotate(90)", "tuning(0.5,0.1,0,-0.3,none)"]), Target::Full).unwrap();
    assert!(*again.image == expected, "the candidate is the renderer's picture");
    s.set_preview_candidate(None).unwrap();
    // A box larger than the photo never enlarges it; an unedited photo is its source.
    s.set_preview_fit((4000, 4000)).unwrap();
    assert!(s.wait_preview(WAIT));
    let big = s.preview().unwrap();
    assert_eq!((big.image.width, big.image.height), (H, W));
    let updates = s.take_preview_updates();
    assert!(updates.iter().all(|u| u.photo == ids[0] && u.kind != UpdateKind::Refined && u.ms >= 0.0));
    assert_eq!(updates.last().map(|u| u.kind), Some(UpdateKind::Final));
    assert_eq!(s.preview_stats().failures, 0);
}

/// A crop keeps its part of the source: the preview has the cropped shape and is the renderer's
/// output at the largest power-of-two scale covering the box.
#[test]
fn a_cropped_preview_is_the_renderers_output() {
    let (_dir, folder, mut s, ids) = setup(1);
    apply(&mut s, ids[0], "crop(0,0,0.5,1)");
    for (fit, scale) in [((1000, 1000), 1.0), ((200, 200), 0.5)] {
        s.open_preview(ids[0], fit).unwrap();
        assert!(s.wait_preview(WAIT));
        let frame = s.preview().unwrap();
        let expected =
            dalica_edit::render(dalica_edit::Source::upright(&decoded(&folder, 0)), &recipe(&["crop(0,0,0.5,1)"]), Target::Scale(scale)).unwrap();
        assert_eq!((frame.image.width, frame.image.height), (expected.width, expected.height));
        assert!(*frame.image == expected, "{fit:?}");
    }
    assert_eq!((s.preview().unwrap().image.width, s.preview().unwrap().image.height), (160, 240));
}

/// A history step on the open photo renders its preview again with the new recipe.
#[test]
fn a_history_step_renders_the_preview_again() {
    let (_dir, _folder, mut s, ids) = setup(1);
    s.open_preview(ids[0], (320, 320)).unwrap();
    assert!(s.wait_preview(WAIT));
    assert_eq!((s.preview().unwrap().image.width, s.preview().unwrap().image.height), (320, 240));
    apply(&mut s, ids[0], "rotate(90)");
    assert!(s.wait_preview(WAIT));
    let frame = s.preview().unwrap();
    assert_eq!((frame.image.width, frame.image.height), (240, 320));
    assert_eq!(frame.recipe, strings(&["rotate(90)"]));
    s.execute("photo.history.undo", &json!({ "id": ids[0] })).unwrap();
    assert!(s.wait_preview(WAIT));
    assert_eq!(s.preview().unwrap().recipe, Vec::<String>::new());
}

/// `roadmap.md` M7.11: a newer request supersedes an older one: candidates sent faster than they
/// render end with the last one shown, and no older generation is shown after a newer one.
#[test]
fn a_newer_request_supersedes_an_older_one() {
    let (_dir, _folder, mut s, ids) = setup(1);
    s.open_preview(ids[0], (640, 640)).unwrap();
    assert!(s.wait_preview(WAIT));
    s.take_preview_updates();
    let mut last = Vec::new();
    for i in 0..20 {
        last = vec![format!("tuning({},0,0,0,none)", f64::from(i) / 20.0)];
        s.set_preview_candidate(Some(&last)).unwrap();
    }
    assert!(s.wait_preview(WAIT));
    let frame = s.preview().unwrap();
    assert_eq!(frame.recipe, last);
    assert_eq!(frame.quality, Quality::Final);
    let updates = s.take_preview_updates();
    assert!(updates.windows(2).all(|w| w[0].generation <= w[1].generation), "generations only go up: {updates:?}");
    assert!(updates.len() < 40, "most of the superseded renders were never shown: {}", updates.len());
    let stats = s.preview_stats();
    assert!(stats.cancelled + stats.stale >= 10, "superseded jobs were cancelled or dropped: {stats:?}");
    // Back to the photo's own recipe.
    s.set_preview_candidate(None).unwrap();
    assert!(s.wait_preview(WAIT));
    assert!(s.preview().unwrap().recipe.is_empty());
}

/// `roadmap.md` M7.11: a tool's candidate comes as a draft before its final, at a lower scale when
/// a full render would miss the frame, and the final is the full size.
#[test]
fn a_draft_comes_before_the_final() {
    let (_dir, folder, mut s, ids) = setup(1);
    s.open_preview(ids[0], (640, 640)).unwrap();
    assert!(s.wait_preview(WAIT));
    s.take_preview_updates();
    let candidate = strings(&["tuning(0.5,0.2,0,0,none)"]);

    // A fast machine: the draft is the full render, and the final follows it at once.
    s.plant_preview_cost(0.0);
    s.set_preview_candidate(Some(&candidate)).unwrap();
    let draft = s.wait_preview_update(WAIT).unwrap();
    assert_eq!(draft.kind, UpdateKind::Draft);
    let fin = s.wait_preview_update(WAIT).unwrap();
    assert_eq!((fin.kind, fin.generation), (UpdateKind::Final, draft.generation));
    assert_eq!((s.preview().unwrap().image.width, s.preview().unwrap().image.height), (W, H));

    // A slow machine: the draft is smaller, then the final at full size.
    s.plant_preview_cost(200.0);
    let candidate = strings(&["tuning(0.6,0.2,0,0,none)"]);
    s.set_preview_candidate(Some(&candidate)).unwrap();
    let draft = s.wait_preview_update(WAIT).unwrap();
    assert_eq!(draft.kind, UpdateKind::Draft);
    let small = s.preview().unwrap().clone();
    assert_eq!(small.quality, Quality::Draft);
    assert!(small.image.width < W && small.image.height < H, "a draft at a lower scale: {} × {}", small.image.width, small.image.height);
    assert!(small.image.width as f64 >= f64::from(W) * 0.25 - 1.0, "never below the smallest draft scale");
    let fin = s.wait_preview_update(WAIT).unwrap();
    assert_eq!((fin.kind, fin.generation), (UpdateKind::Final, draft.generation));
    let frame = s.preview().unwrap();
    let expected = dalica_edit::render(
        dalica_edit::Source::upright(&decoded(&folder, 0)),
        &recipe(&["tuning(0.6,0.2,0,0,none)"]),
        Target::Fit { width: 640, height: 640 },
    )
    .unwrap();
    assert!(*frame.image == expected, "the final is the full render");
}

/// The picture box-averaged to `width × height` (each output pixel the area-weighted mean of the
/// source pixels under it), as the renderer's two-sizes tests shrink their large render.
fn shrunk(img: &Rgba8, width: u32, height: u32) -> Rgba8 {
    let (fx, fy) = (f64::from(img.width) / f64::from(width), f64::from(img.height) / f64::from(height));
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let (y0, y1) = (f64::from(y) * fy, f64::from(y + 1) * fy);
        for x in 0..width {
            let (x0, x1) = (f64::from(x) * fx, f64::from(x + 1) * fx);
            let (mut sum, mut total) = ([0.0f64; 4], 0.0);
            for sy in y0.floor() as u32..(y1.ceil() as u32).min(img.height) {
                let wy = (y1.min(f64::from(sy + 1)) - y0.max(f64::from(sy))).max(0.0);
                for sx in x0.floor() as u32..(x1.ceil() as u32).min(img.width) {
                    let wx = (x1.min(f64::from(sx + 1)) - x0.max(f64::from(sx))).max(0.0);
                    let at = ((sy * img.width + sx) * 4) as usize;
                    for (c, v) in sum.iter_mut().enumerate() {
                        *v += f64::from(img.data[at + c]) * wx * wy;
                    }
                    total += wx * wy;
                }
            }
            data.extend(sum.map(|v| (v / total).round().clamp(0.0, 255.0) as u8));
        }
    }
    Rgba8 { width, height, data }
}

/// The fingerprint distance between two pictures of the same size (`dalica_scenes::fingerprint`).
fn distance(a: &Rgba8, b: &Rgba8) -> f64 {
    use dalica_scenes::fingerprint::{distance, fingerprint};
    distance(&fingerprint(a.width, a.height, &a.data), &fingerprint(b.width, b.height, &b.data))
}

/// `roadmap.md` M13.35: a frame grows the canvas but warps nothing, so a framed recipe's draft
/// renders from the halved base at `Target::Full` (the frame laid out at the base's scale), not
/// from the source through the fractional resample: Border's and Drop Shadow's quarter-scale drafts
/// are the renderer's output on the source halved twice, the frame on all four sides, within the
/// effect's fingerprint tolerance (0.5) of the draft it was (the source at a quarter) and within its
/// two-sizes tolerance (`crates/edit/tests/canvas.rs`, `mats.rs`) of the final shrunk; the source
/// is left as it was. A cropped Border still renders from the source at a quarter.
#[test]
fn a_frames_draft_renders_from_the_halved_base() {
    let (_dir, folder, mut s, ids) = setup(1);
    s.open_preview(ids[0], (640, 640)).unwrap();
    assert!(s.wait_preview(WAIT));
    s.take_preview_updates();
    let source = decoded(&folder, 0);
    let quarter = halve(&halve(&source).unwrap()).unwrap();
    let border = "border(#000000,20,#ffffff,5,0,0)";
    for (entry, two_sizes) in [(border, 6.0), ("drop_shadow(#000000,15,90,25,30,#ffffff)", 7.0)] {
        // A slow machine: the draft at a quarter, then the final at full size.
        s.plant_preview_cost(200.0);
        s.set_preview_candidate(Some(&strings(&[entry]))).unwrap();
        let draft_update = s.wait_preview_update(WAIT).unwrap();
        assert_eq!(draft_update.kind, UpdateKind::Draft, "{entry}");
        let draft = s.preview().unwrap().image.clone();
        let from_base = dalica_edit::render(dalica_edit::Source::upright(&quarter), &recipe(&[entry]), Target::Full).unwrap();
        assert!(*draft == from_base, "{entry}: the draft is the renderer's output on the base halved twice");
        let was = dalica_edit::render(dalica_edit::Source::upright(&source), &recipe(&[entry]), Target::Scale(0.25)).unwrap();
        assert_eq!((draft.width, draft.height), (was.width, was.height), "{entry}: the frame's bands are the same whole pixels");
        let d = distance(&draft, &was);
        assert!(d <= 0.5, "{entry}: the draft is {d:.2} from the one through the resample");
        // The frame on all four sides: the photo inside the canvas, each edge's middle the frame's
        // colour (Border's black outer band, Drop Shadow's white background).
        let geometry = dalica_edit::Geometry::new(&recipe(&[entry]), dalica_image::Orientation::default(), quarter.width, quarter.height);
        let area = geometry.photo_area().unwrap();
        assert!(area.x0 > 0.0 && area.y0 > 0.0 && area.x1 < 1.0 && area.y1 < 1.0, "{entry}: {area:?}");
        let (w, h) = (draft.width, draft.height);
        for (x, y) in [(w / 2, 0), (w / 2, h - 1), (0, h / 2), (w - 1, h / 2)] {
            let at = ((y * w + x) * 4) as usize;
            let rgb = &draft.data[at..at + 3];
            let framed = if entry == border { rgb.iter().all(|&v| v <= 8) } else { rgb.iter().all(|&v| v >= 200) };
            assert!(framed, "{entry}: the frame at {x}, {y}: {rgb:?}");
        }
        let final_update = s.wait_preview_update(WAIT).unwrap();
        assert_eq!((final_update.kind, final_update.generation), (UpdateKind::Final, draft_update.generation), "{entry}");
        let fin = s.preview().unwrap().image.clone();
        let d = distance(&draft, &shrunk(&fin, w, h));
        assert!(d <= two_sizes, "{entry}: the draft is {d:.2} from the final shrunk (two-sizes tolerance {two_sizes})");
    }
    // A crop changes the photo's own size: its Border still renders from the source at a quarter.
    let cropped = ["crop(0,0,0.5,1)", border];
    s.plant_preview_cost(200.0);
    s.set_preview_candidate(Some(&strings(&cropped))).unwrap();
    assert_eq!(s.wait_preview_update(WAIT).unwrap().kind, UpdateKind::Draft);
    let expected = dalica_edit::render(dalica_edit::Source::upright(&source), &recipe(&cropped), Target::Scale(0.25)).unwrap();
    assert!(*s.preview().unwrap().image == expected, "a cropped Border's draft is the source rendered at a quarter");
    assert!(s.wait_preview(WAIT));
    // The preview's source untouched: without the candidate, the unedited photo is its source.
    s.set_preview_candidate(None).unwrap();
    assert!(s.wait_preview(WAIT));
    assert!(*s.preview().unwrap().image == source, "the preview's source is untouched");
}

/// `roadmap.md` M7.11: zoomed past the preview, the visible window is rendered from the whole
/// decode and equals that window of a full render at that scale; fitting again drops it.
#[test]
fn a_refined_window_equals_that_window_of_a_full_render() {
    let (_dir, folder, mut s, ids) = setup(1);
    apply(&mut s, ids[0], "tuning(0.4,0.1,0.1,-0.2,none)");
    apply(&mut s, ids[0], "rotate(90)");
    s.open_preview(ids[0], (200, 200)).unwrap();
    assert!(s.wait_preview(WAIT));
    let region = Region { scale: 2.0, x: 100, y: 300, width: 300, height: 200 };
    s.set_preview_window(Some(region)).unwrap();
    assert!(s.wait_preview(WAIT));
    let refined = s.preview_refined().expect("a refined window").clone();
    assert_eq!(refined.region, region);
    assert_eq!((refined.image.width, refined.image.height), (300, 200));
    let full = decoded(&folder, 0);
    let r = recipe(&["tuning(0.4,0.1,0.1,-0.2,none)", "rotate(90)"]);
    let whole = dalica_edit::render(dalica_edit::Source::upright(&full), &r, Target::Scale(2.0)).unwrap();
    let mut window = Vec::new();
    for y in 300..500 {
        let at = (y * whole.width as usize + 100) * 4;
        window.extend_from_slice(&whole.data[at..at + 300 * 4]);
    }
    assert!(refined.image.data == window, "the window of the full render");
    assert_eq!(s.preview_stats().full_decodes, 1);
    assert!(s.take_preview_updates().iter().any(|u| u.kind == UpdateKind::Refined));
    // A history step while zoomed refines the window again with the new recipe.
    apply(&mut s, ids[0], "rotate(90)");
    assert!(s.wait_preview(WAIT));
    assert_eq!(s.preview_stats().full_decodes, 1, "the whole decode is kept while zoomed");
    assert!(s.preview_refined().is_some());
    s.set_preview_window(None).unwrap();
    assert!(s.preview_refined().is_none());
}

/// `roadmap.md` M7.11: the next and previous photos prefetched open without a decode, their
/// previews shown at once.
#[test]
fn a_prefetched_neighbour_opens_without_a_decode() {
    let (_dir, _folder, mut s, ids) = setup(3);
    apply(&mut s, ids[2], "rotate(90)");
    s.open_preview(ids[1], (400, 400)).unwrap();
    s.prefetch_previews(&[ids[2], ids[0]]);
    assert!(s.wait_preview(WAIT));
    let decodes = s.preview_stats().decodes;
    assert_eq!(decodes, 3, "the open photo and its two neighbours");
    for &next in &[ids[2], ids[0], ids[1]] {
        s.open_preview(next, (400, 400)).unwrap();
        let frame = s.preview().expect("a prefetched preview is shown at once");
        assert_eq!(frame.photo, next);
        assert_eq!(frame.quality, Quality::Final);
        s.prefetch_previews(&[ids[0], ids[1], ids[2]]);
        assert!(s.wait_preview(WAIT));
    }
    assert_eq!(s.preview().unwrap().recipe, Vec::<String>::new());
    assert_eq!(s.preview_stats().decodes, decodes, "no photo was decoded again");
    s.open_preview(ids[2], (400, 400)).unwrap();
    assert_eq!((s.preview().unwrap().image.width, s.preview().unwrap().image.height), (H, W));
}

/// A photo decoded for its preview is stored as the ladder's screen entry; the next session on that
/// thumbnail cache reads the entry instead of decoding the photo, and shows the same preview.
#[test]
fn the_screen_entry_is_stored_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Braga");
    dalica_fs::create_dir_all(&folder).unwrap();
    let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(3, W, H), 85, None).unwrap();
    dalica_fs::replace(&photo_path(&folder, 0), &bytes, Expect::Absent).unwrap();
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::At(dir.path().join("cache")), workers: 2, ..ThumbOptions::default() } };
    let open = || {
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
        s.wait_idle();
        let id = s.library().photos().next().unwrap().id;
        (s, id)
    };
    let (mut s, id) = open();
    s.open_preview(id, (400, 400)).unwrap();
    assert!(s.wait_preview(WAIT));
    assert_eq!((s.preview_stats().decodes, s.preview_stats().cache_loads), (1, 0));
    let first = s.preview().unwrap().image.clone();
    drop(s);
    let (mut s, id) = open();
    s.open_preview(id, (400, 400)).unwrap();
    assert!(s.wait_preview(WAIT));
    assert_eq!((s.preview_stats().decodes, s.preview_stats().cache_loads), (0, 1), "the entry was read, the photo not decoded");
    let again = s.preview().unwrap();
    assert_eq!((again.image.width, again.image.height), (first.width, first.height));
    // The entry is a JPEG: close to the first preview, not equal.
    let mean = again.image.data.iter().zip(first.data.iter()).map(|(a, b)| f64::from(a.abs_diff(*b))).sum::<f64>() / first.data.len() as f64;
    assert!(mean < 4.0, "mean difference {mean}");
}

/// `roadmap.md` M7.11: each preview's histogram equals a direct count of its pixels.
#[test]
fn the_histogram_equals_a_direct_count() {
    let (_dir, _folder, mut s, ids) = setup(1);
    s.open_preview(ids[0], (500, 500)).unwrap();
    assert!(s.wait_preview(WAIT));
    let frame = s.preview().unwrap();
    let mut counts = [[0u32; 256]; 3];
    for px in frame.image.data.as_chunks::<4>().0 {
        for (c, count) in counts.iter_mut().enumerate() {
            count[px[c] as usize] += 1;
        }
    }
    let h: &Histogram = &frame.histogram;
    assert_eq!((h.red.as_slice(), h.green.as_slice(), h.blue.as_slice()), (&counts[0][..], &counts[1][..], &counts[2][..]));
    assert_eq!(h.pixels, u64::from(frame.image.width) * u64::from(frame.image.height));
}

/// `roadmap.md` M7.11: closing the photo cancels its jobs; nothing of it is shown afterwards.
#[test]
fn closing_cancels() {
    let (_dir, _folder, mut s, ids) = setup(2);
    s.open_preview(ids[0], (640, 640)).unwrap();
    s.set_preview_window(Some(Region { scale: 1.0, x: 0, y: 0, width: 64, height: 64 })).unwrap();
    s.close_preview();
    assert!(s.preview_photo().is_none());
    assert!(s.wait_preview(WAIT));
    assert!(s.preview().is_none() && s.preview_refined().is_none());
    assert!(s.take_preview_updates().is_empty(), "nothing of a closed photo is shown");
    let stats = s.preview_stats();
    assert!(stats.cancelled + stats.stale >= 1, "{stats:?}");
    assert_eq!(s.preview_pending(), 0);
    // Opening another photo closes the first.
    s.open_preview(ids[0], (64, 64)).unwrap();
    s.open_preview(ids[1], (64, 64)).unwrap();
    assert!(s.wait_preview(WAIT));
    assert_eq!(s.preview().unwrap().photo, ids[1]);
    assert!(s.take_preview_updates().iter().all(|u| u.photo == ids[1]));
}

/// `roadmap.md` M7.11: the owner thread makes no `fs` call while opening, drafting, zooming,
/// prefetching, polling, waiting and closing.
#[test]
fn the_owner_thread_makes_no_fs_call() {
    let (_dir, _folder, mut s, ids) = setup(3);
    let before = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    s.open_preview(ids[0], (300, 300)).unwrap();
    s.prefetch_previews(&[ids[1], ids[2]]);
    let candidate = strings(&["tuning(0.2,0,0,0,none)"]);
    s.set_preview_candidate(Some(&candidate)).unwrap();
    s.set_preview_window(Some(Region { scale: 1.0, x: 10, y: 10, width: 100, height: 100 })).unwrap();
    while s.preview_pending() > 0 {
        s.poll();
        std::thread::yield_now();
    }
    assert!(s.preview_refined().is_some());
    s.open_preview(ids[1], (300, 300)).unwrap();
    assert!(s.wait_preview(WAIT));
    s.close_preview();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), before, "the owner thread made no fs call");
}

/// Graceful failures: unknown photos, requests with nothing open, recipes and windows the
/// renderer refuses, hostile boxes and a missing file are errors or failures, never panics.
#[test]
fn graceful_failures() {
    let (_dir, folder, mut s, ids) = setup(2);
    assert_eq!(s.open_preview(PhotoId::new(999_999), (100, 100)), Err(PreviewError::NoPhoto));
    assert_eq!(s.set_preview_candidate(None), Err(PreviewError::NotOpen));
    assert_eq!(s.set_preview_fit((10, 10)), Err(PreviewError::NotOpen));
    assert_eq!(s.set_preview_window(None), Err(PreviewError::NotOpen));
    s.prefetch_previews(&[PhotoId::new(5), ids[0]]);
    s.close_preview();
    s.open_preview(ids[0], (0, u32::MAX)).unwrap();
    assert!(s.wait_preview(WAIT));
    let frame = s.preview().unwrap();
    assert!(frame.image.width <= 2, "an empty box gives the source halved as far as it goes");
    let too_many: Vec<String> = (0..=dalica_edit::MAX_OPERATIONS).map(|_| "rotate(90)".to_string()).collect();
    assert!(matches!(s.set_preview_candidate(Some(&too_many)), Err(PreviewError::BadRecipe(_))));
    for bad in [
        Region { scale: f64::NAN, x: 0, y: 0, width: 1, height: 1 },
        Region { scale: 0.0, x: 0, y: 0, width: 1, height: 1 },
        Region { scale: 1e9, x: 0, y: 0, width: 1, height: 1 },
        Region { scale: 1.0, x: 0, y: 0, width: 0, height: 1 },
        Region { scale: 1.0, x: 0, y: 0, width: u32::MAX, height: u32::MAX },
    ] {
        assert!(matches!(s.set_preview_window(Some(bad)), Err(PreviewError::BadWindow(_))), "{bad:?}");
    }
    // A window outside the picture renders nothing and fails quietly.
    s.set_preview_window(Some(Region { scale: 1.0, x: 100_000, y: 0, width: 10, height: 10 })).unwrap();
    assert!(s.wait_preview(WAIT));
    assert!(s.preview_refined().is_none());
    s.set_preview_window(None).unwrap();
    // An unknown operation is skipped by the renderer and kept in the recipe shown.
    let unknown = strings(&["future_op(1)", "rotate(90)"]);
    s.set_preview_candidate(Some(&unknown)).unwrap();
    assert!(s.wait_preview(WAIT));
    assert_eq!(s.preview().unwrap().recipe, unknown);
    s.close_preview();
    // A photo whose file went away before it was ever read: the failure is reported, nothing
    // panics.
    std::fs::remove_file(photo_path(&folder, 1)).unwrap();
    s.open_preview(ids[1], (100, 100)).unwrap();
    assert!(s.wait_preview(WAIT));
    if s.preview_photo().is_some() {
        assert!(s.preview().is_none());
        assert!(s.preview_failure().is_some());
    }
}

/// `ui.settle` waits for what the edit view shows next (`control-protocol.md` §6.12): the open
/// photo's preview is busy from its first open until its final has arrived, and again while a
/// tool's candidate renders; prefetching the neighbours is not.
#[test]
fn the_open_photos_preview_is_busy_until_its_final_arrives() {
    let (_dir, _folder, mut s, ids) = setup(3);
    assert_eq!(s.preview_busy(), 0, "nothing open");
    s.open_preview(ids[0], (300, 300)).unwrap();
    assert!(s.preview_busy() > 0, "the first open reads the photo");
    assert!(s.wait_preview(WAIT));
    assert_eq!(s.preview().map(|p| p.quality), Some(Quality::Final));
    assert_eq!(s.preview_busy(), 0, "the final arrived");
    s.prefetch_previews(&[ids[1], ids[2]]);
    assert!(s.preview_pending() > 0, "the neighbours are prefetched");
    assert_eq!(s.preview_busy(), 0, "prefetching is not what the edit view shows");
    assert!(s.wait_preview(WAIT));
    s.set_preview_candidate(Some(&strings(&["tuning(0.4,0,0,0,none)"]))).unwrap();
    assert!(s.preview_busy() > 0, "a candidate renders");
    assert!(s.wait_preview(WAIT));
    assert_eq!(s.preview_busy(), 0);
    s.close_preview();
    assert_eq!(s.preview_busy(), 0);
}

/// Runs `cmd` on photo `id` as a save job and waits for it and what it started.
fn save_job(s: &mut Session, cmd: &str, params: serde_json::Value) {
    let reply = s.execute(cmd, &params).unwrap();
    let job = serde_json::from_value(reply["job"].clone()).unwrap();
    let entry = s.wait_job(job, WAIT).unwrap();
    assert!(entry.state.is_finished(), "{cmd}: {:?}", entry.state);
    s.wait_idle();
}

const EDIT: &str = "tuning(0.6,0,0,0.4,none)";
const BIG: (u32, u32) = (4000, 4000);

/// Photo `id`'s preview, opened in a box larger than the photo (the file itself while unedited).
fn open_big(s: &mut Session, id: PhotoId) -> Arc<Rgba8> {
    s.open_preview(id, BIG).unwrap();
    assert!(s.wait_preview(WAIT));
    s.preview().unwrap().image.clone()
}

/// After a Save the open preview is the saved file, not the render of the original kept ready
/// from before the edit (its recipe, empty again, and its box are the same; its file is not).
#[test]
fn after_a_save_the_preview_is_the_saved_file() {
    let (_dir, folder, mut s, ids) = setup(1);
    let original = open_big(&mut s, ids[0]);
    assert!(*original == decoded(&folder, 0), "unedited, the preview is the file");
    apply(&mut s, ids[0], EDIT);
    assert!(s.wait_preview(WAIT));
    assert!(*s.preview().unwrap().image != *original, "the edit shows");

    save_job(&mut s, "photo.save", json!({ "ids": [ids[0]], "confirm": true }));
    assert!(s.library().info(ids[0]).edits.is_empty(), "saved: no edits left");
    assert!(s.wait_preview(WAIT));
    let saved = s.preview().unwrap();
    assert!(saved.recipe.is_empty());
    let on_disk = decoded(&folder, 0);
    assert!(on_disk != *original, "the file holds the edit");
    assert!(*saved.image != *original, "the preview no longer shows the original");
    assert!(*saved.image == on_disk, "the preview is the saved file");
}

/// After Undo Save the preview is the original with the edits again, not a render of the saved
/// file with the same recipe kept ready by a prefetch: saved, edited once more and that edit
/// undone (the saved file with the edit is prefetched meanwhile), then Undo Save.
#[test]
fn after_undo_save_the_preview_is_the_original_with_the_edits() {
    let (_dir, folder, mut s, ids) = setup(2);
    let original = open_big(&mut s, ids[0]);
    apply(&mut s, ids[0], EDIT);
    assert!(s.wait_preview(WAIT));
    save_job(&mut s, "photo.save", json!({ "ids": [ids[0]], "confirm": true }));
    assert!(s.wait_preview(WAIT));
    apply(&mut s, ids[0], EDIT);
    assert!(s.wait_preview(WAIT));
    // The neighbour is opened, and the saved photo with the edit prefetched beside it.
    open_big(&mut s, ids[1]);
    s.prefetch_previews(&[ids[0]]);
    assert!(s.wait_preview(WAIT));
    s.execute("photo.history.undo", &json!({ "id": ids[0] })).unwrap();
    save_job(&mut s, "photo.undoSave", json!({ "ids": [ids[0]] }));
    assert_eq!(s.library().info(ids[0]).edits, strings(&[EDIT]), "the edit is back, unsaved");
    let back = decoded(&folder, 0);
    assert!(back == *original, "the original is back on disk");

    let undone = open_big(&mut s, ids[0]);
    let expected = dalica_edit::render(dalica_edit::Source::upright(&back), &recipe(&[EDIT]), Target::Full).unwrap();
    assert!(*undone == expected, "the preview is the original with the edit");
}

/// After a Revert the preview is the original, not the saved file prefetched beside a neighbour.
#[test]
fn after_a_revert_the_preview_is_the_original() {
    let (_dir, folder, mut s, ids) = setup(2);
    let original = open_big(&mut s, ids[0]);
    apply(&mut s, ids[0], EDIT);
    assert!(s.wait_preview(WAIT));
    save_job(&mut s, "photo.save", json!({ "ids": [ids[0]], "confirm": true }));
    open_big(&mut s, ids[1]);
    s.prefetch_previews(&[ids[0]]);
    assert!(s.wait_preview(WAIT));
    save_job(&mut s, "photo.revert", json!({ "ids": [ids[0]], "confirm": true }));
    assert!(s.library().info(ids[0]).edits.is_empty(), "reverted: no edits");
    assert!(decoded(&folder, 0) == *original, "the original is back on disk");
    let reverted = open_big(&mut s, ids[0]);
    assert!(*reverted == *original, "the preview is the original again");
}
