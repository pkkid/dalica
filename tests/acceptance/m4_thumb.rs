//! The speed thumb held at its top speed through a 100 k-photo library (`docs/roadmap.md` → M4 →
//! *Light table* → Done when; included by `m4.rs`).
//!
//! The library is the index-only synthetic 100 k preset (`dalica_engine::synthetic`, 1 000 folders,
//! every photo with its colour sketch in the index, no photo files, thumbnails off): the case where
//! nothing but the index can be drawn, so every cell has to draw its sketch on the frame it appears.
//! The thumb is pressed and pulled to the end of its track with real pointer events, and held there
//! from the top of the library to the bottom; every frame is checked on its own:
//!
//! - no cell draws the grey placeholder and none is marked failed; every cell laid out draws its
//!   sketch; no texture is made or uploaded (`panels::thumbnails` counters, `ui.inspect` → `perf`);
//! - the view moves by the top speed's share of a frame (40 screen heights a second by default) and
//!   never stops before the bottom;
//! - the cells laid out stay within one screen of rows (flat, whatever the library's size);
//! - the UI time (layout + tessellation, as `perf` measures it) is collected: in a release build
//!   the 95th percentile must be within the 8 ms budget (`development.md` §8); in a debug build
//!   (`cargo xtask ci`) it is printed, since a debug time limit would only measure the machine.
//!
//! At the bottom the release stops the view where it is and the thumb springs back to the centre.

use std::sync::Arc;
use std::time::Duration;

use dalica_engine::settings::ThumbSpeed;
use dalica_engine::synthetic::{self, Spec};
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::panels::light_table::scrollbar::{self, SPRING_SECONDS, THUMB_H};
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, Pos2, vec2};

/// The UI frame budget at 100 k photos, p95 (`development.md` §8).
pub const BUDGET_MS: f64 = 8.0;
/// Frames the first frames and the release may take to settle.
const SETTLE: usize = 60 * 60;
/// More than the whole library takes at the top speed (about 80 s of virtual time).
const MAX_FRAMES: usize = 60 * 300;

/// What the run measured.
#[derive(Debug)]
pub struct Run {
    pub photos: usize,
    pub frames: usize,
    pub screens: f64,
    pub sketch_cells: u64,
    pub cells_max: u64,
    pub cells_bound: u64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The nearest-rank `p`th percentile of `sorted` (as `perf` and `library_bench` compute it).
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

fn widget_rect(d: &Driver, id: &str) -> [f32; 4] {
    d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no `{id}` on screen")).rect
}

/// Opens the index-only 100 k library at 1400 × 900, holds the thumb at the end of its track
/// until the bottom, checking every frame, and releases it.
pub fn hold_through_100k() -> Run {
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec::PHOTOS_100K;
    let session = synthetic::open(dir.path(), &spec, Arc::new(dalica_core::SystemClock)).unwrap();
    assert_eq!(session.thumbnail_cache_dir(), None, "an index-only library: thumbnails are off");
    assert!(session.library().photos().all(|p| session.library().sketch(p.id).is_some()), "every photo sketched in the index");
    let mut d = Driver::new(App::new(Some(session)), 1400, 900, 1.0);
    d.settle(SETTLE).unwrap();

    let lt = &d.app().panels().unwrap().light_table;
    let info = lt.frame_info().clone();
    let layout = lt.layout().unwrap().clone();
    let view_h = info.area.unwrap().height();
    let max_offset = layout.max_offset(view_h);
    let screens = max_offset / f64::from(view_h);
    assert!(screens > 2000.0, "100 k photos are thousands of screens: {screens:.0}");
    // A frame lays out at most the rows that touch the view, plus one each side.
    let rows = (f64::from(view_h) / layout.row_pitch()).ceil() as u64 + 2;
    let cells_bound = layout.grid().cols as u64 * rows;

    // Press the thumb and pull it to the end of the track (and beyond: it stops there).
    let track = widget_rect(&d, "scroll:track");
    let reach = (track[3] - THUMB_H) / 2.0;
    let thumb = widget_rect(&d, "scroll:thumb");
    let c = Pos2::new(thumb[0] + thumb[2] / 2.0, thumb[1] + thumb[3] / 2.0);
    let events = [(PointerKind::Move, 0.0), (PointerKind::Down, 0.0), (PointerKind::Drag, reach / 2.0), (PointerKind::Drag, reach + 40.0)]
        .map(|(kind, dy)| PointerEvent { kind, pos: c + vec2(0.0, dy) });
    d.pointer(&events, PointerButton::Primary, Modifiers::NONE).unwrap();
    let top = scrollbar::speed(reach, reach, &ThumbSpeed::default());
    assert_eq!(top, 40.0, "the default curve's top speed: 40 screen heights a second");
    let speed = d.app().panels().unwrap().light_table.scrollbar.speed();
    let want = top * f64::from(view_h);
    assert!((speed - want).abs() <= want * 1e-5, "held at the track's end: {speed} pt/s, want {want}");
    let per_frame = want / 60.0;

    // Top to bottom, one frame at a time.
    let mut times = Vec::new();
    let mut sketch_cells = 0;
    let mut cells_max = 0;
    let mut last = d.app().panels().unwrap().light_table.offset();
    let mut frames = 0;
    while last < max_offset {
        assert!(frames < MAX_FRAMES, "the thumb did not reach the bottom in {MAX_FRAMES} frames (at {last} of {max_offset})");
        d.step();
        frames += 1;
        let panels = d.app().panels().unwrap();
        let offset = panels.light_table.offset();
        let moved = offset - last;
        let to_go = max_offset - last;
        assert!((moved - per_frame.min(to_go)).abs() <= 1.0, "frame {frames}: the view moved {moved} pt, want {} (at {last})", per_frame.min(to_go));
        last = offset;
        let c = panels.thumbnails.counts();
        let cells = panels.light_table.frame_info().cells as u64;
        assert_eq!((c.placeholder_cells, c.failed_cells), (0, 0), "frame {frames} at {offset}: {c:?}");
        assert!(cells > 0 && c.sketch_cells == cells, "frame {frames} at {offset}: every cell draws its sketch ({cells} cells): {c:?}");
        assert_eq!((c.covered_cells, c.upscaled_cells, c.thumb_uploads, c.thumb_textures), (0, 0, 0, 0), "frame {frames}: {c:?}");
        sketch_cells += c.sketch_cells;
        let sample = d.perf().samples().last().unwrap();
        assert!(sample.cells_visited <= cells_bound, "frame {frames}: {} cells visited, at most {cells_bound}", sample.cells_visited);
        cells_max = cells_max.max(sample.cells_visited);
        times.push(ms(sample.frame));
    }
    assert!(frames > 1000, "a long scroll: {frames} frames");

    // The release stops the view there; the thumb is back at the centre 150 ms later.
    let pos = d.pointer_pos().unwrap();
    d.pointer(&[PointerEvent { kind: PointerKind::Up, pos }], PointerButton::Primary, Modifiers::NONE).unwrap();
    assert_eq!(d.app().panels().unwrap().light_table.scrollbar.speed(), 0.0);
    d.wait((SPRING_SECONDS * 1000.0) as u64 + 17).unwrap();
    assert_eq!(widget_rect(&d, "scroll:thumb"), thumb, "the thumb is back at the centre");
    d.settle(SETTLE).unwrap();
    assert_eq!(d.app().panels().unwrap().light_table.offset(), max_offset, "the view stays at the bottom");
    let stats = d.app().session().unwrap().thumbnail_stats();
    assert_eq!((stats.failed, stats.jobs, stats.decodes), (0, 0, 0), "nothing was asked of the disk: {stats:?}");

    times.sort_by(f64::total_cmp);
    Run {
        photos: spec.photos,
        frames,
        screens,
        sketch_cells,
        cells_max,
        cells_bound,
        p50_ms: rank(&times, 50),
        p95_ms: rank(&times, 95),
        max_ms: times.last().copied().unwrap_or(0.0),
    }
}
