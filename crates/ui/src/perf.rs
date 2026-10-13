//! Perf counters (`control-protocol.md` §6.6 → `perf`, `development.md` §10 *Profiling*): what one
//! UI frame cost and how much it touched, over a rolling window of the last [`WINDOW`] frames.
//!
//! Two sources feed a frame's [`Sample`]:
//! - whoever runs the frame (the headless driver, later the window) measures the times and the
//!   texture changes of egui's output;
//! - panels report what they visited through [`count`] during the frame. Until a panel reports a
//!   visit counter, it is derived from the widget registry: every cell, section header and tree row
//!   laid out registers its id (`control-protocol.md` §7), so the registry lists exactly the items
//!   laid out.
//!
//! `DALICA_PROFILE=1` logs every frame slower than [`SLOW_FRAME_MS`] with its counters.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use dalica_ui_kit::WidgetInfo;
use egui::{Context, Id, TextureId, TexturesDelta};
use serde::Serialize;

/// Frames kept for the averages and the 95th percentile.
pub const WINDOW: usize = 240;
/// Frames slower than this are logged when profiling (`development.md` §8 budget).
pub const SLOW_FRAME_MS: f64 = 8.0;

/// Whether `DALICA_PROFILE=1` is set.
pub fn profile_from_env() -> bool {
    std::env::var_os("DALICA_PROFILE").is_some_and(|v| v == "1")
}

/// A per-frame count a panel reports with [`count`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Counter {
    /// Light-table cells laid out.
    CellsVisited,
    /// Light-table section headers laid out.
    SectionsVisited,
    /// Library-tree rows laid out.
    TreeRowsVisited,
    /// Thumbnail jobs requested.
    ThumbRequests,
    /// Scanner `Op`s applied.
    OpsApplied,
    /// Microseconds of per-frame logic before layout (polling the session, shortcuts).
    LogicMicros,
    /// The edit view's preview (`editing.md` §3): microseconds from the request to the frame that
    /// shows it, reported the frame a draft, a final preview or a refined window arrives.
    PreviewDraftMicros,
    PreviewFinalMicros,
    PreviewRefinedMicros,
    /// 1 when the photo canvas drew the scaled preview while the refined window of a zoomed view
    /// was still on its way (the frame did not wait for it).
    PreviewWaiting,
}

#[derive(Debug, Clone, Default)]
struct Reported {
    pass: u64,
    counts: HashMap<Counter, u64>,
}

fn key() -> Id {
    Id::new("dalica-ui.perf")
}

/// Adds `n` to `counter` for the current pass. Cheap: one map entry per counter.
pub fn count(ctx: &Context, counter: Counter, n: u64) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let r = d.get_temp_mut_or_default::<Reported>(key());
        if r.pass != pass {
            r.pass = pass;
            r.counts.clear();
        }
        let c = r.counts.entry(counter).or_insert(0);
        *c = c.saturating_add(n);
    });
}

/// The counts reported in the current pass or, between passes, the one that just ended.
pub fn reported(ctx: &Context) -> HashMap<Counter, u64> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<Reported>(key()).filter(|r| r.pass.saturating_add(1) >= pass).map(|r| r.counts).unwrap_or_default())
}

/// Items laid out, as the widget registry lists them: `(cells, sections, tree rows)`. Sub-widgets
/// (`thumb:3/star`, `section:folder:12/title`, `tree:disclosure:4`) are not items.
pub fn visited_from_widgets(widgets: &[WidgetInfo]) -> (u64, u64, u64) {
    let (mut cells, mut sections, mut rows) = (0u64, 0u64, 0u64);
    for w in widgets {
        let Some((kind, name)) = w.id.split_once(':') else { continue };
        if name.contains('/') {
            continue;
        }
        match kind {
            "thumb" if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()) => cells += 1,
            "section" if !name.is_empty() => sections += 1,
            "tree" if ["folder:", "album:", "person:", "group:"].iter().any(|p| name.starts_with(p)) => rows += 1,
            _ => {}
        }
    }
    (cells, sections, rows)
}

/// One frame's measurements.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    /// The frame's clock time (seconds), for frames per second.
    pub at: f64,
    /// Layout (the app's frame) plus paint tessellation.
    pub frame: Duration,
    /// Per-frame logic before layout, when the app reports it.
    pub logic: Option<Duration>,
    /// Everything the frame took: logic, layout, tessellation, texture upload.
    pub update: Duration,
    pub cells_visited: u64,
    pub sections_visited: u64,
    pub tree_rows_visited: u64,
    pub thumb_requests: Option<u64>,
    pub ops_applied: Option<u64>,
    /// Whole textures created or replaced this frame (partial updates of the font atlas excluded).
    pub textures_uploaded: u64,
    pub textures_freed: u64,
    /// The edit view's preview times that arrived this frame (request to shown).
    pub preview_draft: Option<Duration>,
    pub preview_final: Option<Duration>,
    pub preview_refined: Option<Duration>,
    /// The canvas drew the scaled preview while a refined window was on its way.
    pub preview_waiting: bool,
}

impl Sample {
    /// Fills the counters from what panels reported and, for visit counts they did not report, from
    /// the widgets of the frame.
    pub fn count(&mut self, reported: &HashMap<Counter, u64>, widgets: &[WidgetInfo]) {
        let (cells, sections, rows) = visited_from_widgets(widgets);
        let get = |c: Counter| reported.get(&c).copied();
        self.cells_visited = get(Counter::CellsVisited).unwrap_or(cells);
        self.sections_visited = get(Counter::SectionsVisited).unwrap_or(sections);
        self.tree_rows_visited = get(Counter::TreeRowsVisited).unwrap_or(rows);
        self.thumb_requests = get(Counter::ThumbRequests);
        self.ops_applied = get(Counter::OpsApplied);
        self.logic = get(Counter::LogicMicros).map(Duration::from_micros);
        self.preview_draft = get(Counter::PreviewDraftMicros).map(Duration::from_micros);
        self.preview_final = get(Counter::PreviewFinalMicros).map(Duration::from_micros);
        self.preview_refined = get(Counter::PreviewRefinedMicros).map(Duration::from_micros);
        self.preview_waiting = get(Counter::PreviewWaiting).is_some_and(|n| n > 0);
    }
}

/// The `perf` object of `ui.inspect` (`control-protocol.md` §6.6). Times in milliseconds; counters
/// the app does not report yet are `null`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Frames in the window.
    pub frames: usize,
    /// Mean UI time per frame (layout + tessellation) over the window.
    pub frame_ms: f64,
    pub frame_p95_ms: f64,
    /// The slowest frame since the last reset.
    pub max_frame_ms: f64,
    pub logic_ms: Option<f64>,
    pub update_ms: f64,
    pub fps: f64,
    pub cells_visited: u64,
    pub sections_visited: u64,
    pub tree_rows_visited: u64,
    pub thumb_requests: Option<u64>,
    pub ops_applied: Option<u64>,
    pub textures_uploaded: u64,
    /// Live egui textures and their bytes.
    pub textures: usize,
    pub texture_bytes: u64,
    /// Textures freed since the last reset.
    pub texture_evictions: u64,
    /// The edit view's preview: the latest draft, final preview and refined window in the window
    /// of frames, each from its request to the frame that showed it (`null` when none arrived).
    pub preview_draft_ms: Option<f64>,
    pub preview_final_ms: Option<f64>,
    pub preview_refined_ms: Option<f64>,
    /// Frames in the window that drew the scaled preview while a refined window was on its way.
    pub preview_waiting_frames: u64,
}

/// The rolling window, the live textures and the maxima.
#[derive(Debug, Clone, Default)]
pub struct Perf {
    samples: VecDeque<Sample>,
    max_frame: Duration,
    textures: HashMap<TextureId, u64>,
    evictions: u64,
    profile: bool,
    /// Slow frames logged since the last reset.
    slow_frames: u64,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

impl Perf {
    /// Counters that log slow frames when `profile` is on (see [`profile_from_env`]).
    pub fn new(profile: bool) -> Perf {
        Perf { profile, ..Perf::default() }
    }

    pub fn profiling(&self) -> bool {
        self.profile
    }

    /// Notes the textures a frame created, replaced and freed (call before handing the delta to a
    /// renderer); returns `(uploaded, freed)` for the frame's sample.
    pub fn observe_textures(&mut self, delta: &TexturesDelta) -> (u64, u64) {
        let mut uploaded = 0u64;
        for (id, updates) in &delta.set {
            for u in updates {
                if u.pos.is_none() {
                    let [w, h] = u.image.size();
                    let bytes = w.saturating_mul(h).saturating_mul(u.image.bytes_per_pixel());
                    self.textures.insert(*id, u64::try_from(bytes).unwrap_or(u64::MAX));
                    uploaded += 1;
                }
            }
        }
        let mut freed = 0u64;
        for id in &delta.free {
            if self.textures.remove(id).is_some() {
                freed += 1;
            }
        }
        self.evictions = self.evictions.saturating_add(freed);
        (uploaded, freed)
    }

    /// Adds a finished frame to the window; logs it when profiling and slow. Returns the log line
    /// if one was written.
    pub fn record(&mut self, sample: Sample) -> Option<String> {
        self.max_frame = self.max_frame.max(sample.frame);
        let line = if self.profile { slow_frame_line(&sample) } else { None };
        if let Some(l) = &line {
            self.slow_frames = self.slow_frames.saturating_add(1);
            log::info!(target: "dalica::profile", "{l}");
        }
        if self.samples.len() >= WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        line
    }

    /// Clears the maxima and the eviction count (`resetPerf: true`); the window and the live
    /// textures stay.
    pub fn reset(&mut self) {
        self.max_frame = self.samples.back().map_or(Duration::ZERO, |s| s.frame);
        self.evictions = 0;
        self.slow_frames = 0;
    }

    /// The frames in the window, oldest first.
    pub fn samples(&self) -> impl Iterator<Item = &Sample> {
        self.samples.iter()
    }

    pub fn last(&self) -> Option<&Sample> {
        self.samples.back()
    }

    pub fn slow_frames(&self) -> u64 {
        self.slow_frames
    }

    pub fn report(&self) -> Report {
        let n = self.samples.len();
        let mut frames: Vec<f64> = self.samples.iter().map(|s| ms(s.frame)).collect();
        frames.sort_by(f64::total_cmp);
        let mean = |f: &dyn Fn(&Sample) -> f64| if n == 0 { 0.0 } else { self.samples.iter().map(f).sum::<f64>() / n as f64 };
        // The nearest-rank 95th percentile.
        let p95 = match n {
            0 => 0.0,
            _ => frames.get((n * 95).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0),
        };
        let logic: Vec<f64> = self.samples.iter().filter_map(|s| s.logic.map(ms)).collect();
        let span = match (self.samples.front(), self.samples.back()) {
            (Some(a), Some(b)) => b.at - a.at,
            _ => 0.0,
        };
        let last = self.samples.back().cloned().unwrap_or_default();
        let latest = |f: &dyn Fn(&Sample) -> Option<Duration>| self.samples.iter().rev().find_map(f).map(ms);
        Report {
            frames: n,
            frame_ms: mean(&|s| ms(s.frame)),
            frame_p95_ms: p95,
            max_frame_ms: ms(self.max_frame),
            logic_ms: (!logic.is_empty()).then(|| logic.iter().sum::<f64>() / logic.len() as f64),
            update_ms: mean(&|s| ms(s.update)),
            fps: if span > 0.0 && span.is_finite() { (n.saturating_sub(1)) as f64 / span } else { 0.0 },
            cells_visited: last.cells_visited,
            sections_visited: last.sections_visited,
            tree_rows_visited: last.tree_rows_visited,
            thumb_requests: last.thumb_requests,
            ops_applied: last.ops_applied,
            textures_uploaded: last.textures_uploaded,
            textures: self.textures.len(),
            texture_bytes: self.textures.values().fold(0u64, |a, b| a.saturating_add(*b)),
            texture_evictions: self.evictions,
            preview_draft_ms: latest(&|s| s.preview_draft),
            preview_final_ms: latest(&|s| s.preview_final),
            preview_refined_ms: latest(&|s| s.preview_refined),
            preview_waiting_frames: self.samples.iter().filter(|s| s.preview_waiting).count() as u64,
        }
    }
}

/// The `DALICA_PROFILE=1` line for a frame slower than [`SLOW_FRAME_MS`], `None` for a fast one.
pub fn slow_frame_line(s: &Sample) -> Option<String> {
    let frame = ms(s.frame);
    if frame <= SLOW_FRAME_MS {
        return None;
    }
    let logic = s.logic.map_or_else(|| "-".to_string(), |l| format!("{:.1}", ms(l)));
    let opt = |v: Option<u64>| v.map_or_else(|| "-".to_string(), |v| v.to_string());
    Some(format!(
        "slow UI frame: {frame:.1} ms (update {:.1} ms, logic {logic} ms); cells visited {}, sections {}, tree rows {}, \
         thumbnail requests {}, ops applied {}, textures uploaded {}",
        ms(s.update),
        s.cells_visited,
        s.sections_visited,
        s.tree_rows_visited,
        opt(s.thumb_requests),
        opt(s.ops_applied),
        s.textures_uploaded,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_ui_kit::registry::Role;
    use egui::{Rect, pos2};

    fn widget(id: &str) -> WidgetInfo {
        WidgetInfo::new(id, Role::Other, "", Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), true, false)
    }

    fn frame(ms: u64, at: f64) -> Sample {
        Sample { at, frame: Duration::from_millis(ms), update: Duration::from_millis(ms + 1), ..Sample::default() }
    }

    #[test]
    fn items_laid_out_are_counted_from_their_ids_and_sub_widgets_are_not() {
        let ids = [
            "thumb:1",
            "thumb:22",
            "thumb:3/star",
            "thumb:3.star",
            "thumb:",
            "thumb:x",
            "section:folder:12",
            "section:folder:12/title",
            "section:",
            "tree:folder:1",
            "tree:album:starred",
            "tree:person:4",
            "tree:group:folders",
            "tree:disclosure:1",
            "button:star",
            "nocolon",
        ];
        let widgets: Vec<_> = ids.iter().map(|i| widget(i)).collect();
        assert_eq!(visited_from_widgets(&widgets), (2, 1, 4));
        assert_eq!(visited_from_widgets(&[]), (0, 0, 0));
    }

    #[test]
    fn reported_counts_win_over_the_registry_and_belong_to_one_pass() {
        let ctx = Context::default();
        let widgets = [widget("thumb:1"), widget("thumb:2"), widget("tree:folder:1")];
        let mut sample = Sample::default();
        let run = |f: &dyn Fn(&Context)| ctx.run_ui(egui::RawInput::default(), |ui| f(ui.ctx())).textures_delta.clear();
        run(&|ctx| {
            count(ctx, Counter::CellsVisited, 5);
            count(ctx, Counter::CellsVisited, 2);
            count(ctx, Counter::ThumbRequests, u64::MAX);
            count(ctx, Counter::ThumbRequests, 3);
            count(ctx, Counter::LogicMicros, 1500);
        });
        sample.count(&reported(&ctx), &widgets);
        assert_eq!(sample.cells_visited, 7, "reported");
        assert_eq!(sample.tree_rows_visited, 1, "from the registry");
        assert_eq!(sample.thumb_requests, Some(u64::MAX), "saturates");
        assert_eq!(sample.logic, Some(Duration::from_micros(1500)));
        assert_eq!(sample.ops_applied, None, "not reported is not zero");
        // The next pass starts from nothing.
        run(&|_| {});
        run(&|_| {});
        assert!(reported(&ctx).is_empty());
    }

    #[test]
    fn the_window_keeps_240_frames_with_mean_p95_max_and_fps() {
        let mut p = Perf::new(false);
        assert_eq!(p.report(), Report::default(), "empty");
        for i in 0..300u64 {
            // 1 ms frames, every 10th one 10 ms; one 50 ms frame that leaves the window.
            let ms = if i == 5 {
                50
            } else if i % 10 == 0 {
                10
            } else {
                1
            };
            assert_eq!(p.record(frame(ms, i as f64 / 60.0)), None, "not profiling");
        }
        let r = p.report();
        assert_eq!(r.frames, WINDOW);
        assert_eq!(r.max_frame_ms, 50.0, "the maximum outlives the window");
        assert_eq!(r.frame_p95_ms, 10.0);
        assert!((r.frame_ms - (24.0 * 10.0 + 216.0) / 240.0).abs() < 1e-9, "{}", r.frame_ms);
        assert!((r.update_ms - r.frame_ms - 1.0).abs() < 1e-9);
        assert!((r.fps - 60.0).abs() < 1e-6, "{}", r.fps);
        assert_eq!(r.logic_ms, None);
        p.reset();
        assert_eq!(p.report().max_frame_ms, 1.0, "reset keeps only the last frame");
        // A single frame has no rate; a clock that does not move neither.
        let mut one = Perf::new(false);
        one.record(frame(3, 1.0));
        one.record(frame(3, 1.0));
        assert_eq!(one.report().fps, 0.0);
        let mut nan = Perf::new(false);
        nan.record(frame(3, f64::NAN));
        nan.record(frame(3, 2.0));
        assert_eq!(nan.report().fps, 0.0);
    }

    #[test]
    fn textures_are_tracked_by_id_with_their_bytes() {
        let mut p = Perf::new(false);
        let image = |w, h| egui::epaint::ImageDelta::full(egui::ColorImage::filled([w, h], egui::Color32::RED), egui::TextureOptions::LINEAR);
        let mut delta = TexturesDelta::default();
        delta.set.insert(TextureId::User(1), [image(4, 2)].into_iter().collect());
        delta.set.insert(TextureId::User(2), [image(1, 1)].into_iter().collect());
        let mut partial = image(1, 1);
        partial.pos = Some([0, 0]);
        delta.set.insert(TextureId::Managed(0), [partial].into_iter().collect());
        assert_eq!(p.observe_textures(&delta), (2, 0));
        let mut gone = TexturesDelta::default();
        gone.free.insert(TextureId::User(1));
        gone.free.insert(TextureId::User(99));
        assert_eq!(p.observe_textures(&gone), (0, 1), "freeing an unknown texture is no eviction");
        // A delta must be emptied before it is dropped (a renderer would have drained it).
        delta.clear();
        gone.clear();
        p.record(Sample { textures_uploaded: 2, ..Sample::default() });
        let r = p.report();
        assert_eq!((r.textures, r.texture_bytes, r.texture_evictions, r.textures_uploaded), (1, 4, 1, 2));
        p.reset();
        assert_eq!(p.report().texture_evictions, 0);
    }

    #[test]
    fn profiling_logs_only_slow_frames() {
        let mut p = Perf::new(true);
        assert!(p.profiling());
        assert_eq!(p.record(frame(8, 0.0)), None, "the budget itself is not slow");
        let line = p.record(Sample { cells_visited: 48, thumb_requests: Some(3), ..frame(12, 0.1) }).unwrap();
        assert!(line.starts_with("slow UI frame: 12.0 ms"), "{line}");
        assert!(line.contains("cells visited 48") && line.contains("thumbnail requests 3") && line.contains("ops applied -"), "{line}");
        assert_eq!(p.slow_frames(), 1);
        let with_logic = slow_frame_line(&Sample { logic: Some(Duration::from_micros(2500)), ..frame(9, 0.0) }).unwrap();
        assert!(with_logic.contains("logic 2.5 ms"), "{with_logic}");
        assert!(!Perf::new(false).profiling());
    }

    #[test]
    fn the_report_serialises_with_the_protocol_names() {
        let v = serde_json::to_value(Perf::new(false).report()).unwrap();
        for k in ["frameMs", "frameP95Ms", "maxFrameMs", "logicMs", "updateMs", "fps", "cellsVisited", "sectionsVisited", "treeRowsVisited"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        for k in ["thumbRequests", "opsApplied", "texturesUploaded", "textures", "textureBytes", "textureEvictions"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        assert!(v["logicMs"].is_null() && v["thumbRequests"].is_null());
        for k in ["previewDraftMs", "previewFinalMs", "previewRefinedMs"] {
            assert!(v[k].is_null(), "{k}");
        }
        assert_eq!(v["previewWaitingFrames"], 0);
    }

    #[test]
    fn preview_times_keep_the_latest_in_the_window_and_waiting_frames_are_counted() {
        let ctx = Context::default();
        let mut p = Perf::new(false);
        let run = |f: &dyn Fn(&Context)| ctx.run_ui(egui::RawInput::default(), |ui| f(ui.ctx())).textures_delta.clear();
        for (i, (draft, fin, waiting)) in
            [(Some(4000), None, false), (None, Some(25_000), true), (Some(3000), None, true), (None, None, false)].into_iter().enumerate()
        {
            run(&|ctx| {
                if let Some(d) = draft {
                    count(ctx, Counter::PreviewDraftMicros, d);
                }
                if let Some(f) = fin {
                    count(ctx, Counter::PreviewFinalMicros, f);
                }
                if waiting {
                    count(ctx, Counter::PreviewWaiting, 1);
                }
            });
            let mut s = frame(1, i as f64 / 60.0);
            s.count(&reported(&ctx), &[]);
            p.record(s);
        }
        let r = p.report();
        assert_eq!((r.preview_draft_ms, r.preview_final_ms, r.preview_refined_ms), (Some(3.0), Some(25.0), None));
        assert_eq!(r.preview_waiting_frames, 2);
    }
}
