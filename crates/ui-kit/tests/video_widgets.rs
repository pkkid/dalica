//! The video player's widgets (`design-system.md` §6; `editing.md` §11, `viewing.md` §4):
//!
//! - the scrubber: dragging the playhead reports positions in milliseconds clamped to the clip, a
//!   click jumps and the arrows step; the start handle never passes the end (nor the end the
//!   start), by drag or by key; the parts trimmed away are dimmed; the readout formats 0, 59 999
//!   ms and 3 h 2 min right and adds the trimmed length only when trimmed;
//! - the volume: the speaker toggle mutes and restores the volume, the slider sets the level and
//!   unmutes;
//! - the gallery: a video under the edit view's bar (the scrubber trimmed, the volume, the
//!   player's and the video tab's glyphs) in a 1400 × 900 window
//!   (`fingerprints/gallery-video-1400x900.json`) and, checked for fit, 900 × 600.
//!
//! Every frame registers its ids in the grammar of `control-protocol.md` §7 and leaves no
//! interactive widget without one. The renders go to `target/test-output/ui-kit/`;
//! `DALICA_BLESS=1` stores new fingerprints (commit them only after looking at the PNGs).

use crate::common::{Canvas, check};
use dalica_ui_kit::icons::{Glyph, OsIcon, VideoIcon, paint_glyph};
use dalica_ui_kit::widgets::{
    PLAYHEAD_R, SCRUB_TRACK_ROW, Scrubber, clock_text, scrubber, scrubber_readout, text_button, trim_end_to, trim_start_to, volume, volume_toggled,
};
use dalica_ui_kit::{Image, Tokens, WidgetInfo, fonts, registry};
use egui::{Align2, Color32, Context, Event, Id, Key, Modifiers, PointerButton, Pos2, Rect, Ui, UiBuilder, pos2, vec2};

const WINDOWS: [(u32, u32); 2] = [(1400, 900), (900, 600)];

fn check_ids(what: &str, widgets: &[WidgetInfo], unregistered: &[registry::Unregistered]) {
    assert_eq!(unregistered, [], "{what}: interactive widgets without an id");
    let mut seen = std::collections::HashSet::new();
    for w in widgets {
        assert!(registry::is_valid_id(&w.id), "{what}: {:?} breaks the id grammar", w.id);
        assert!(seen.insert(w.id.clone()), "{what}: {} registered twice", w.id);
    }
}

fn area(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)), add);
}

fn rect_of(w: &WidgetInfo) -> Rect {
    Rect::from_min_size(pos2(w.rect[0], w.rect[1]), vec2(w.rect[2], w.rect[3]))
}

/// A scene drawn each frame with real input.
struct Rig<S> {
    ctx: Context,
    t: Tokens,
    size: (u32, u32),
    clock: f64,
    state: S,
    scene: fn(&mut Ui, &Tokens, &mut S),
    widgets: Vec<WidgetInfo>,
    canvas: Canvas,
}

impl<S> Rig<S> {
    fn new(size: (u32, u32), state: S, scene: fn(&mut Ui, &Tokens, &mut S)) -> Rig<S> {
        let t = Tokens::default();
        let ctx = Context::default();
        t.apply(&ctx);
        let mut rig = Rig { ctx, t, size, clock: 0.0, state, scene, widgets: vec![], canvas: Canvas::new(size, t.chrome) };
        rig.settle();
        rig
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.clock += 1.0 / 60.0;
        let mut raw = dalica_ui_kit::raw_input(self.size.0, self.size.1, 1.0);
        raw.time = Some(self.clock);
        raw.events = std::iter::once(Event::ModifiersChanged(Modifiers::NONE)).chain(events).collect();
        let (t, scene) = (self.t, self.scene);
        let state = &mut self.state;
        let out = self.ctx.run_ui(raw, |ui| scene(ui, &t, state));
        self.widgets = registry::widgets(&self.ctx);
        self.canvas.keep(out);
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.frame(vec![]);
        }
    }

    fn find(&self, id: &str) -> Option<&WidgetInfo> {
        self.widgets.iter().rev().find(|w| w.id == id)
    }

    fn rect(&self, id: &str) -> Rect {
        let w = self.find(id).unwrap_or_else(|| panic!("no {id} on screen: {:?}", self.widgets.iter().map(|w| &w.id).collect::<Vec<_>>()));
        rect_of(w)
    }

    fn press(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// A click at `at`, a second after the last (never a double-click).
    fn click_at(&mut self, at: Pos2) {
        self.clock += 1.0;
        self.frame(vec![Event::PointerMoved(at), Self::press(at, true)]);
        self.frame(vec![Self::press(at, false)]);
    }

    fn click(&mut self, id: &str) {
        let at = self.rect(id).center();
        self.click_at(at);
    }

    /// A drag through `path` (the pointer pressed at the first point, let go at the last).
    fn drag_through(&mut self, path: &[Pos2]) {
        let (Some(&from), Some(&to)) = (path.first(), path.last()) else { return };
        self.clock += 1.0;
        self.frame(vec![Event::PointerMoved(from), Self::press(from, true)]);
        for &p in path.iter().skip(1) {
            self.frame(vec![Event::PointerMoved(p)]);
        }
        self.frame(vec![Self::press(to, false)]);
        self.frame(vec![]);
    }

    fn key(&mut self, key: Key) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![ev(true), ev(false)]);
    }

    fn focus(&mut self, id: &str) {
        self.ctx.memory_mut(|m| m.request_focus(Id::new(id)));
        self.settle();
    }

    fn pixel(&mut self, at: Pos2) -> [u8; 4] {
        self.canvas.image(&self.ctx).pixel(at.x as u32, at.y as u32).unwrap()
    }

    fn image(&mut self) -> Image {
        self.canvas.image(&self.ctx).clone()
    }
}

// ---- The scrubber ----

const SCRUB: &str = "slider:video.position";
const SCRUB_AT: Pos2 = pos2(100.0, 100.0);
const SCRUB_W: f32 = 600.0;

struct ScrubScene {
    clip: Scrubber,
    started: usize,
    stopped: usize,
    readout: String,
}

fn scrub_scene(ui: &mut Ui, t: &Tokens, s: &mut ScrubScene) {
    area(ui, Rect::from_min_size(SCRUB_AT, vec2(SCRUB_W, 80.0)), |ui| {
        let out = scrubber(ui, t, SCRUB, &s.clip, SCRUB_W);
        if let Some(p) = out.position {
            s.clip.position_ms = p;
        }
        if let Some(v) = out.start {
            s.clip.start_ms = v;
        }
        if let Some(v) = out.end {
            s.clip.end_ms = v;
        }
        s.started += usize::from(out.drag_started);
        s.stopped += usize::from(out.drag_stopped);
        s.readout = out.readout;
    });
}

/// A 41-second clip, stepping 40 ms.
fn scrub_rig() -> Rig<ScrubScene> {
    let clip = Scrubber { step_ms: 40, ..Scrubber::new(41_000) };
    Rig::new((900, 300), ScrubScene { clip, started: 0, stopped: 0, readout: String::new() }, scrub_scene)
}

/// The track's two ends on the screen.
fn track_ends() -> (f32, f32) {
    (SCRUB_AT.x + PLAYHEAD_R + 1.0, SCRUB_AT.x + SCRUB_W - PLAYHEAD_R - 1.0)
}

fn track_x(ms: u64) -> f32 {
    let (x0, x1) = track_ends();
    x0 + ms as f32 / 41_000.0 * (x1 - x0)
}

#[test]
fn dragging_the_playhead_reports_milliseconds_clamped_to_the_clip() {
    let mut rig = scrub_rig();
    check_ids("scrubber", &rig.widgets, &registry::unregistered(&rig.ctx));
    let y = SCRUB_AT.y + SCRUB_TRACK_ROW / 2.0;
    let (x0, x1) = track_ends();
    // Half way along: half the clip, to the millisecond the pixel stands for.
    rig.drag_through(&[pos2(x0, y), pos2(x0 + 100.0, y), pos2((x0 + x1) / 2.0, y)]);
    assert!(rig.state.clip.position_ms.abs_diff(20_500) <= 1, "{}", rig.state.clip.position_ms);
    assert_eq!((rig.state.started, rig.state.stopped), (1, 1), "one drag");
    // Past the end and past the start: the clip's ends.
    rig.drag_through(&[pos2((x0 + x1) / 2.0, y), pos2(x1 + 200.0, y + 60.0)]);
    assert_eq!(rig.state.clip.position_ms, 41_000);
    rig.drag_through(&[pos2(x1, y), pos2(x0 - 300.0, y)]);
    assert_eq!(rig.state.clip.position_ms, 0);
    // A click jumps; the arrows step by the clip's step, never past an end.
    rig.click_at(pos2(track_x(9_000), y));
    assert!(rig.state.clip.position_ms.abs_diff(9_000) <= 40, "{}", rig.state.clip.position_ms);
    rig.state.clip.position_ms = 9_000;
    rig.focus(SCRUB);
    rig.key(Key::ArrowRight);
    rig.key(Key::ArrowRight);
    assert_eq!(rig.state.clip.position_ms, 9_080);
    rig.key(Key::ArrowLeft);
    assert_eq!(rig.state.clip.position_ms, 9_040);
    rig.state.clip.position_ms = 20;
    rig.settle();
    rig.key(Key::ArrowLeft);
    assert_eq!(rig.state.clip.position_ms, 0);
    assert_eq!(rig.find(&format!("{SCRUB}/readout")).map(|w| w.label.clone()), Some("00:00:00 / 00:00:41".into()));
}

#[test]
fn the_start_handle_never_passes_the_end() {
    let mut rig = scrub_rig();
    rig.state.clip.end_ms = 30_000;
    rig.settle();
    let start = rig.rect(&format!("{SCRUB}/start"));
    let end = rig.rect(&format!("{SCRUB}/end"));
    // Grabbed where it is, dragged from there: the start moves by what the pointer moved.
    let from = start.center();
    rig.drag_through(&[from, from + vec2(20.0, 0.0), from + vec2(track_x(12_000) - track_x(0), 0.0)]);
    assert!(rig.state.clip.start_ms.abs_diff(12_000) <= 1, "{}", rig.state.clip.start_ms);
    // Dragged far past the end, it stops a step before it.
    let from = rig.rect(&format!("{SCRUB}/start")).center();
    rig.drag_through(&[from, from + vec2(300.0, 0.0), from + vec2(800.0, 0.0)]);
    assert_eq!(rig.state.clip.start_ms, 29_960);
    // The end, dragged far past the start, stops a step after it.
    let from = end.center();
    rig.drag_through(&[from, from - vec2(400.0, 0.0), from - vec2(900.0, 0.0)]);
    assert_eq!(rig.state.clip.end_ms, 30_000, "the start is at 29 960: the end stays a step after it");
    rig.state.clip.start_ms = 5_000;
    rig.settle();
    let from = rig.rect(&format!("{SCRUB}/end")).center();
    rig.drag_through(&[from, from - vec2(900.0, 0.0)]);
    assert_eq!(rig.state.clip.end_ms, 5_040);
    // By key: the end steps right, the start steps right until it meets the end's step.
    rig.focus(&format!("{SCRUB}/end"));
    rig.key(Key::ArrowRight);
    assert_eq!(rig.state.clip.end_ms, 5_080);
    rig.focus(&format!("{SCRUB}/start"));
    for _ in 0..3 {
        rig.key(Key::ArrowRight);
        rig.settle();
    }
    assert_eq!(rig.state.clip.start_ms, 5_040);
    // The rule itself, on hostile clips: a reversed trim, a clip shorter than a step, none at all.
    let reversed = Scrubber { start_ms: 9_000, end_ms: 2_000, ..Scrubber::new(10_000) };
    assert_eq!((trim_start_to(&reversed, 9_500), trim_end_to(&reversed, 0)), (8_967, 9_033));
    let short = Scrubber { step_ms: 40, ..Scrubber::new(10) };
    assert_eq!((trim_start_to(&short, 10), trim_end_to(&short, 0)), (0, 10));
    let empty = Scrubber::new(0);
    assert_eq!((trim_start_to(&empty, 5), trim_end_to(&empty, 5)), (0, 0));
}

#[test]
fn the_parts_trimmed_away_are_dimmed() {
    let mut rig = scrub_rig();
    rig.state.clip = Scrubber { start_ms: 10_000, end_ms: 30_000, position_ms: 0, ..rig.state.clip };
    rig.settle();
    let y = SCRUB_AT.y + SCRUB_TRACK_ROW / 2.0;
    let (cut, kept, after) = (rig.pixel(pos2(track_x(5_000), y)), rig.pixel(pos2(track_x(20_000), y)), rig.pixel(pos2(track_x(35_000), y)));
    assert_eq!(kept, rig.t.track.to_array(), "the kept part is the track");
    for px in [cut, after] {
        assert!(px[0] < kept[0] && px != kept, "trimmed away {px:?} is dimmer than {kept:?}");
    }
}

#[test]
fn the_readout_formats_times_and_says_the_trimmed_length_only_when_trimmed() {
    assert_eq!(clock_text(0), "00:00:00");
    assert_eq!(clock_text(59_999), "00:00:59");
    assert_eq!(clock_text((3 * 60 + 2) * 60_000), "03:02:00");
    assert_eq!(clock_text(u64::MAX), "5124095576030:25:51", "no overflow");
    let clip = Scrubber { position_ms: 9_400, ..Scrubber::new(41_000) };
    assert_eq!(scrubber_readout(&clip), "00:00:09 / 00:00:41");
    let trimmed = Scrubber { start_ms: 12_000, end_ms: 33_000, ..clip };
    assert_eq!(scrubber_readout(&trimmed), "00:00:09 / 00:00:41 (trimmed to 00:00:21)");
    assert_eq!(scrubber_readout(&Scrubber { end_ms: 41_000, start_ms: 0, ..trimmed }), "00:00:09 / 00:00:41", "a trim of the whole clip is none");
    // The widget paints and registers the same text, and follows a change in the same frame.
    let mut rig = scrub_rig();
    rig.state.clip = Scrubber { position_ms: 9_400, start_ms: 12_000, end_ms: 33_000, ..rig.state.clip };
    rig.settle();
    let label = rig.find(&format!("{SCRUB}/readout")).map(|w| w.label.clone());
    assert_eq!(label.as_deref(), Some("00:00:09 / 00:00:41 (trimmed to 00:00:21)"));
    assert_eq!(rig.state.readout, "00:00:09 / 00:00:41 (trimmed to 00:00:21)");
}

// ---- The volume ----

const VOLUME: &str = "slider:video.volume";

struct VolumeScene {
    level: f32,
    muted: bool,
}

fn volume_scene(ui: &mut Ui, t: &Tokens, s: &mut VolumeScene) {
    area(ui, Rect::from_min_size(pos2(40.0, 40.0), vec2(200.0, 40.0)), |ui| {
        let out = volume(ui, t, VOLUME, s.level, s.muted);
        if let Some(l) = out.level {
            s.level = l;
        }
        if let Some(m) = out.muted {
            s.muted = m;
        }
    });
}

#[test]
fn the_toggle_mutes_and_restores_the_volume() {
    let mut rig = Rig::new((300, 120), VolumeScene { level: 0.7, muted: false }, volume_scene);
    check_ids("volume", &rig.widgets, &registry::unregistered(&rig.ctx));
    let mute = format!("{VOLUME}/mute");
    assert_eq!(rig.find(&mute).map(|w| w.active), Some(false));
    rig.click(&mute);
    assert_eq!((rig.state.level, rig.state.muted), (0.7, true), "muting keeps the level");
    assert_eq!(rig.find(&mute).map(|w| w.active), Some(true));
    rig.click(&mute);
    assert_eq!((rig.state.level, rig.state.muted), (0.7, false), "unmuting gives it back");
    // The slider sets the level and unmutes.
    rig.click(&mute);
    let bar = rig.rect(VOLUME);
    rig.drag_through(&[bar.center(), pos2(bar.min.x - 50.0, bar.center().y)]);
    assert_eq!((rig.state.level, rig.state.muted), (0.0, false));
    // At 0 the speaker reads off, and a click on it brings some sound back.
    rig.click(&mute);
    assert!(rig.state.level > 0.0 && !rig.state.muted, "{} {}", rig.state.level, rig.state.muted);
    // The arrows with the slider's focus.
    rig.state.level = 0.5;
    rig.focus(VOLUME);
    rig.key(Key::ArrowRight);
    assert!((rig.state.level - 0.6).abs() < 1e-5, "{}", rig.state.level);
    // The rule itself.
    assert_eq!(volume_toggled(0.4, false), (0.4, true));
    assert_eq!(volume_toggled(0.4, true), (0.4, false));
    assert!(!volume_toggled(f32::NAN, true).1);
    assert_eq!(VideoIcon::for_volume(0.4, true), VideoIcon::VolumeOff);
}

// ---- The gallery ----

/// The video tab's and the system's glyphs, shown with their names.
const TAB_GLYPHS: [(Glyph, &str); 7] = [
    (Glyph::Video(VideoIcon::SaveFrame), "Save Frame as Picture"),
    (Glyph::Video(VideoIcon::ExportClip), "Export Clip"),
    (Glyph::Video(VideoIcon::VideoFrames), "Video Frames"),
    (Glyph::Os(OsIcon::OpenWith), "Open With"),
    (Glyph::Os(OsIcon::LocateOnDisk), "Locate on Disk"),
    (Glyph::Os(OsIcon::ExternalEditor), "Edit in external editor"),
    (Glyph::Os(OsIcon::Camera), "Camera"),
];

/// The player's glyphs, left to right on the bar.
const BAR_GLYPHS: [VideoIcon; 5] = [VideoIcon::FrameBack, VideoIcon::Play, VideoIcon::Pause, VideoIcon::FrameForward, VideoIcon::SetStart];

fn gallery(ui: &mut Ui, t: &Tokens, _: &mut ()) {
    let screen = ui.max_rect();
    let p = ui.painter().clone();
    p.rect_filled(screen, 0, t.canvas);
    // The edit panel on the right with the video tab's glyphs; the video and its bar on the left.
    let panel = Rect::from_min_max(pos2(screen.max.x - 280.0, screen.min.y), screen.max);
    p.rect_filled(panel, 0, t.chrome);
    for (i, (glyph, name)) in TAB_GLYPHS.iter().enumerate() {
        let y = panel.min.y + 24.0 + i as f32 * 30.0;
        paint_glyph(&p, Rect::from_min_size(pos2(panel.min.x + 16.0, y), vec2(20.0, 20.0)), *glyph, t.icon);
        p.text(pos2(panel.min.x + 46.0, y + 10.0), Align2::LEFT_CENTER, *name, fonts::regular(t.body), t.text_label);
    }
    let room = Rect::from_min_max(screen.min + vec2(16.0, 16.0), pos2(panel.min.x - 16.0, screen.max.y - 120.0));
    let h = (room.width() * 9.0 / 16.0).min(room.height());
    let video = Rect::from_center_size(room.center(), vec2(h * 16.0 / 9.0, h));
    p.rect_filled(video, 0, Color32::from_rgb(70, 110, 150));
    p.rect_filled(Rect::from_min_max(video.min, pos2(video.max.x, video.center().y)), 0, Color32::from_rgb(140, 170, 200));
    let bar = Rect::from_min_max(pos2(screen.min.x, screen.max.y - 104.0), pos2(panel.min.x, screen.max.y));
    p.rect_filled(bar, 0, t.chrome);
    let mut x = bar.min.x + 16.0;
    for (i, icon) in BAR_GLYPHS.into_iter().enumerate() {
        let color = if i == 2 { t.text_disabled } else { t.icon };
        paint_glyph(&p, Rect::from_min_size(pos2(x, bar.min.y + 14.0), vec2(20.0, 20.0)), Glyph::Video(icon), color);
        x += 32.0;
    }
    area(ui, Rect::from_min_max(pos2(x + 8.0, bar.min.y + 8.0), pos2(bar.max.x - 16.0, bar.max.y)), |ui| {
        let clip = Scrubber { position_ms: 9_400, start_ms: 12_000, end_ms: 33_000, step_ms: 40, duration_ms: 41_000 };
        let w = ui.available_width() - 120.0;
        ui.horizontal_top(|ui| {
            scrubber(ui, t, SCRUB, &clip, w);
            ui.add_space(12.0);
            volume(ui, t, VOLUME, 0.7, false);
        });
        ui.horizontal(|ui| {
            text_button(ui, t, "button:video.setStart", "Set Start", false);
            text_button(ui, t, "button:video.setEnd", "Set End", false);
            ui.add_enabled_ui(false, |ui| volume(ui, t, "slider:video.disabled", 0.3, true));
        });
    });
}

#[test]
fn the_gallery_fits_both_windows_and_matches_its_fingerprint() {
    let mut failures = Vec::new();
    for size in WINDOWS {
        let what = format!("gallery {size:?}");
        let mut one = Rig::new(size, (), gallery);
        check_ids(&what, &one.widgets, &registry::unregistered(&one.ctx));
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32));
        for w in &one.widgets {
            assert!(screen.expand(0.01).contains_rect(rect_of(w)), "{what}: {} at {:?} leaves the window", w.id, w.rect);
        }
        for id in
            [SCRUB, "slider:video.position/start", "slider:video.position/end", "slider:video.position/readout", VOLUME, "slider:video.volume/mute"]
        {
            assert!(one.find(id).is_some(), "{what}: no {id}");
        }
        if size == (1400, 900) {
            let a = one.image();
            let mut two = Rig::new(size, (), gallery);
            let b = two.image();
            assert!(a.rgba == b.rgba, "{what}: two renders of the same frame differ");
            check(&format!("gallery-video-{}x{}", size.0, size.1), &a, &mut failures);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
