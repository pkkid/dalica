//! Videos in the window (`roadmap.md` M15.21; `ui-layout.md` §5.3, `editing.md` §11, `viewing.md`
//! §4), headless and by id, in a folder of two photos and a clip (an MP4 built here from the
//! documented ISO/IEC 14496-12 boxes: 1 min 23.4 s of 640 × 360, no frames Dalica decodes): the
//! clip's cell is the video tile with its sprocket edge and its duration "1:23", the photos' are
//! not; the Videos filter leaves only the clip; Enter opens it in the edit view, where the photo
//! area shows the tile fitted with "This video can't be played" and Open With ▸, the panel the
//! video tab (rotate and Export Clip live, Save Frame greyed: its frames do not decode) with tabs
//! 1–5 greyed and why, and Save greyed with the reason; at 900 × 600 the tab and the area fit. Fingerprints at 1400 × 900:
//! the clip's cell (`video-cell-1400x900`) and the edit view on it (`video-edit-1400x900`, the
//! panel and the photo area). Renders go to `target/test-output/ui/`; `DALICA_BLESS=1` stores new
//! fingerprints.

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_rect, new_driver, put_jpeg, shot, try_widget, widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::menus::{self, Platform};
use dalica_ui::panels::edit::panel::TAB_ITEMS;
use dalica_ui::panels::edit::panel::VIDEO_TAB_ID;
use dalica_ui::panels::edit::video::{self, tab};
use dalica_ui::state::View;
use dalica_ui::{App, Driver};
use dalica_ui_kit::widgets::{Badges, Video};
use serde_json::{Value, json};

// --- An MP4 from its boxes ------------------------------------------------------------------------

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = u32::try_from(payload.len() + 8).unwrap().to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

/// Version 0 `mvhd`/`mdhd` times.
fn times(timescale: u32, duration: u32) -> Vec<u8> {
    let mut p = vec![0; 4];
    for v in [0, 0, timescale, duration] {
        p.extend(v.to_be_bytes());
    }
    p
}

fn matrix() -> Vec<u8> {
    [0x1_0000i32, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000].iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// `ms` of 640 × 360 AVC at 30 frames a second: the facts only (the `mdat` holds no frames).
fn clip(ms: u32) -> Vec<u8> {
    let mut mvhd = times(1000, ms);
    mvhd.extend(0x1_0000u32.to_be_bytes());
    mvhd.extend(0x100u16.to_be_bytes());
    mvhd.extend([0; 10]);
    mvhd.extend(matrix());
    mvhd.extend([0; 24]);
    mvhd.extend(2u32.to_be_bytes());
    let (w, h) = (640u16, 360u16);
    let mut tkhd = vec![0, 0, 0, 7];
    tkhd.extend([0; 8]);
    tkhd.extend(1u32.to_be_bytes());
    tkhd.extend([0; 4 + 4 + 8 + 8]);
    tkhd.extend(matrix());
    tkhd.extend((u32::from(w) << 16).to_be_bytes());
    tkhd.extend((u32::from(h) << 16).to_be_bytes());
    let mut entry = vec![0; 6];
    entry.extend(1u16.to_be_bytes());
    entry.extend([0; 16]);
    entry.extend(w.to_be_bytes());
    entry.extend(h.to_be_bytes());
    entry.extend([0; 50]);
    let mut stsd = vec![0; 4];
    stsd.extend(1u32.to_be_bytes());
    stsd.extend(bx(b"avc1", &entry));
    let frames = ms / 1000 * 30;
    let mut stsz = vec![0; 4];
    stsz.extend(1000u32.to_be_bytes());
    stsz.extend(frames.to_be_bytes());
    let stbl = bx(b"stbl", &[bx(b"stsd", &stsd), bx(b"stsz", &stsz)].concat());
    let mut mdhd = times(30_000, ms * 30);
    mdhd.extend([0; 4]);
    let mut hdlr = vec![0; 8];
    hdlr.extend(b"vide");
    hdlr.extend([0; 13]);
    let mdia = bx(b"mdia", &[bx(b"mdhd", &mdhd), bx(b"hdlr", &hdlr), bx(b"minf", &stbl)].concat());
    let moov = bx(b"moov", &[bx(b"mvhd", &mvhd), bx(b"trak", &[bx(b"tkhd", &tkhd), mdia].concat())].concat());
    let mut ftyp = b"mp42".to_vec();
    ftyp.extend(0x200u32.to_be_bytes());
    ftyp.extend(b"mp42isom");
    [bx(b"ftyp", &ftyp), bx(b"mdat", &[0x5a; 256]), moov].concat()
}

// --- The window -----------------------------------------------------------------------------------

/// A window at 1400 × 900 on a library watching `Trip`: a.jpg, b.jpg and clip.mp4 (1:23.4).
struct W {
    _dir: tempfile::TempDir,
    d: Driver,
    /// a.jpg, b.jpg, clip.mp4.
    ids: [PhotoId; 3],
}

impl W {
    fn new() -> W {
        let dir = tempfile::tempdir().unwrap();
        let trip = dir.path().join("Trip");
        put_jpeg(&trip.join("a.jpg"), 21);
        put_jpeg(&trip.join("b.jpg"), 22);
        std::fs::write(trip.join("clip.mp4"), clip(83_400)).unwrap();
        let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
        s.execute("folders.add", &json!({ "path": trip })).unwrap();
        s.wait_idle();
        let id = |name: &str| s.library().photos().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} listed")).id;
        let ids = [id("a.jpg"), id("b.jpg"), id("clip.mp4")];
        let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
        d.settle(FRAMES).unwrap();
        W { _dir: dir, d, ids }
    }

    fn clip(&self) -> PhotoId {
        self.ids[2]
    }

    fn badges(&self, id: PhotoId) -> Badges {
        self.d.app().panels().unwrap().light_table.cells.get(id).unwrap_or_else(|| panic!("{id} drawn")).badges
    }

    fn shown(&self) -> Vec<PhotoId> {
        let mut v: Vec<PhotoId> = self.d.app().panels().unwrap().light_table.cells.shown().iter().map(|s| s.photo).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Opens the clip in the edit view the way a person does: selected, then Enter.
    fn open_clip(&mut self) {
        let clip = self.clip();
        self.d.run("select.set", &json!({ "ids": [clip] })).unwrap();
        self.d.settle(FRAMES).unwrap();
        self.d.key("Enter").unwrap();
        self.d.settle(FRAMES).unwrap();
        assert_eq!((self.d.state().view, self.d.state().edit.photo), (View::Edit, Some(clip)), "Enter opens the clip");
    }

    fn enabled(&self, id: &str) -> bool {
        widget(&self.d, id).enabled
    }

    /// The menu bar's Save as it stands now.
    fn save(&self) -> menus::ItemNode {
        let bar = menus::bar(self.d.app(), Platform::current());
        menus::find_id(&bar.iter().flat_map(|m| m.children.clone()).collect::<Vec<_>>(), "photo.save", &Value::Null)
            .cloned()
            .expect("Save in the bar")
    }
}

/// The union of two `[x, y, w, h]` rects.
fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (x0, y0) = (a[0].min(b[0]), a[1].min(b[1]));
    let (x1, y1) = ((a[0] + a[2]).max(b[0] + b[2]), (a[1] + a[3]).max(b[1] + b[3]));
    [x0, y0, x1 - x0, y1 - y0]
}

#[test]
fn a_clip_is_the_video_tile_with_its_sprocket_edge_and_its_duration() {
    let w = W::new();
    assert_eq!(w.badges(w.clip()).video, Some(Video { seconds: Some(83) }), "the sprocket edge and 1:23");
    for photo in &w.ids[..2] {
        assert_eq!(w.badges(*photo).video, None, "a photo carries no video marks");
    }
    assert_eq!(dalica_ui_kit::widgets::duration_text(83), "1:23");
    check_rect(&w.d, "video-cell-1400x900", widget(&w.d, &format!("thumb:{}", w.clip())).rect);
}

#[test]
fn the_videos_filter_leaves_only_the_clip() {
    let mut w = W::new();
    assert_eq!(w.shown().len(), 3);
    w.d.click("button:filter.videos").unwrap();
    w.d.settle(FRAMES).unwrap();
    assert_eq!(w.shown(), [w.clip()], "only the clip");
    assert_eq!(w.badges(w.clip()).video, Some(Video { seconds: Some(83) }), "with its marks");
    w.d.click("button:filter.videos").unwrap();
    w.d.settle(FRAMES).unwrap();
    assert_eq!(w.shown().len(), 3, "the filter off shows them all again");
}

#[test]
fn enter_opens_the_clip_with_the_video_tab_and_the_tools_greyed() {
    let mut w = W::new();
    let clip = w.clip();
    // A double-click opens it too.
    w.d.click_with(&format!("thumb:{clip}"), &dalica_ui::headless::Click { count: 2, ..Default::default() }).unwrap();
    w.d.settle(FRAMES).unwrap();
    assert_eq!((w.d.state().view, w.d.state().edit.photo), (View::Edit, Some(clip)), "a double-click opens the clip");
    w.d.run("view.library", &json!({})).unwrap();
    w.d.settle(FRAMES).unwrap();
    w.open_clip();
    // The photo area: the tile fitted, why it does not play, and Open With ▸.
    let area = widget(&w.d, video::REGION_ID).rect;
    let picture = widget(&w.d, video::PICTURE_ID).rect;
    assert!(picture[0] >= area[0] && picture[0] + picture[2] <= area[0] + area[2] && picture[1] >= area[1], "{picture:?} in {area:?}");
    assert!((picture[2] / picture[3] - 16.0 / 9.0).abs() < 0.02, "640 × 360 keeps its shape: {picture:?}");
    assert_eq!(widget(&w.d, video::CANNOT_PLAY_ID).label, "This video can't be played");
    assert!(w.enabled(video::OPEN_WITH_ID));
    w.d.click(video::OPEN_WITH_ID).unwrap();
    assert!(
        w.d.widgets().iter().any(|wi| wi.id.starts_with("menuitem:photo.openWith") || wi.id.starts_with("menuitem:openWith.list/")),
        "Open With ▸ lists the photo menu's rows: {:?}",
        w.d.widgets().iter().map(|wi| wi.id.clone()).collect::<Vec<_>>()
    );
    w.d.click(video::OPEN_WITH_ID).unwrap();
    w.d.settle(FRAMES).unwrap();
    assert!(!w.d.widgets().iter().any(|wi| wi.id.starts_with("menuitem:")), "a second click closes it");
    // The panel: the video tab shown, tabs 1–5 greyed with why, no tool drawn.
    let video_tab = widget(&w.d, VIDEO_TAB_ID);
    assert!(video_tab.enabled && video_tab.active, "{video_tab:?}");
    for (_, id, ..) in TAB_ITEMS {
        assert!(!w.enabled(id), "{id} greyed on a video");
    }
    for id in ["tool:crop", "tool:straighten", "tool:retouch", "tool:autofix", "button:edit.showOriginal"] {
        assert!(try_widget(&w.d, id).is_none(), "{id} is not drawn on a video");
    }
    assert!(!w.enabled(tab::SAVE_FRAME_ID), "no frame to save: the clip does not decode");
    assert!(w.enabled(tab::EXPORT_CLIP_ID), "Export Clip works on the container");
    assert!(w.enabled(tab::ROTATE_CW_ID) && w.enabled(tab::ROTATE_CCW_ID));
    let save = w.save();
    assert!(!save.enabled, "nothing to save: {save:?}");
    // Rotate is live: one step of the clip's history; the tile turns with it; Save stays greyed.
    w.d.click(tab::ROTATE_CW_ID).unwrap();
    w.d.settle(FRAMES).unwrap();
    assert_eq!(w.d.app().session().unwrap().library().info(clip).edits.len(), 1, "one rotation");
    let turned = widget(&w.d, video::PICTURE_ID).rect;
    assert!(turned[3] > turned[2], "a quarter turn shows it upright: {turned:?}");
    let save = w.save();
    assert!(!save.enabled && save.reason.as_deref().is_some_and(|r| !r.is_empty()), "Save is greyed with the reason: {save:?}");
    w.d.run("edit.undo", &json!({})).ok();
    w.d.settle(FRAMES).unwrap();
}

#[test]
fn the_edit_view_on_an_unplayable_clip_fits_at_900_by_600() {
    let mut w = W::new();
    w.open_clip();
    let rect = union(widget(&w.d, "region:editPanel").rect, widget(&w.d, video::REGION_ID).rect);
    check_rect(&w.d, "video-edit-1400x900", rect);
    w.d.resize(SMALL.0, SMALL.1);
    w.d.settle(FRAMES).unwrap();
    check_fits(&w.d, "the edit view on a video");
    shot(&w.d, "video-edit-900x600");
    let panel = widget(&w.d, "region:editPanel").rect;
    let inside = |r: [f32; 4]| {
        r[0] >= panel[0] - 0.01 && r[1] >= panel[1] - 0.01 && r[0] + r[2] <= panel[0] + panel[2] + 0.01 && r[1] + r[3] <= panel[1] + panel[3] + 0.01
    };
    let mut ids: Vec<&str> = vec![VIDEO_TAB_ID, tab::SAVE_FRAME_ID, tab::EXPORT_CLIP_ID, tab::ROTATE_CCW_ID, tab::ROTATE_CW_ID];
    ids.extend(TAB_ITEMS.iter().map(|t| t.1));
    for id in ids {
        let r = widget(&w.d, id).rect;
        assert!(inside(r), "{id} at {r:?} fits the panel {panel:?}");
    }
    let area = widget(&w.d, video::REGION_ID).rect;
    for id in [video::PICTURE_ID, video::CANNOT_PLAY_ID, video::OPEN_WITH_ID] {
        let r = widget(&w.d, id).rect;
        assert!(
            r[0] >= area[0] && r[1] >= area[1] && r[0] + r[2] <= area[0] + area[2] + 0.01 && r[1] + r[3] <= area[1] + area[3] + 0.01,
            "{id} at {r:?} in {area:?}"
        );
    }
}
