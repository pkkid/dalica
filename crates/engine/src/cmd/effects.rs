//! The effect tabs' commands (`editing.md` §8.1, `menus-and-shortcuts.md` §1.9; M12.5):
//!
//! - `effect.list {tab?}` (query): the effects of tabs 3–5 (or of one tab) in each tab's grid
//!   order, straight from `edit`'s catalogue: name, label, tab, place, one-click, crosshair, whether
//!   it changes the picture's size, whether this build renders it, its tooltip, its default entry
//!   and its parameters (kind, range, default, logarithmic slider; the controls a panel draws).
//! - `photo.effect.pick {id, x, y}` starts an `analysis` job (answered through the jobs registry,
//!   `Jobs::start_answered`): a worker reads the photo small, renders its recipe so far (the photo
//!   as shown before the effect being set: cropped, straightened and turned) and answers the mean
//!   colour of a small disc ([`PICK_RADIUS`] of the shorter side) around the point, given
//!   normalised in the stored frame (`data-model.md` §3.3) as every effect point is, as
//!   `"#rrggbb"` through `core::color`: the eyedropper and Vignette's pick from the photo.
//!
//! The tile previews themselves are `Session::request_effect_tiles` (`effect_tiles.rs`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use dalica_core::PhotoId;
use dalica_edit::catalog::{self, Crosshair, Effect as Tile, Kind, OpInfo, Param};
use dalica_edit::{Geometry, Point, Recipe, Target};
use dalica_image::{DecodeOptions, Limits, Orientation, Rgba8};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::jobs::{Answer, JobKind, Outcome};
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

/// The longer side of the picture a pick samples.
pub const PICK_EDGE: u32 = 512;
/// The disc a pick averages: its radius, of the picture's shorter side.
pub const PICK_RADIUS: f64 = 0.01;
/// The most picks running at once.
pub const MAX_PICKS: usize = 8;
/// The pick job's label while it runs.
pub const PICKING: &str = "Picking a colour from the photo";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "effect.list",
            label: "Effects",
            menu: "",
            keys: &[],
            params: r##"{"tab"?:3|4|5}: the effects of the edit panel's tabs 3–5 (or of one tab) in each tab's grid order: {effects: [{name, label, tab, place, oneClick, crosshair: "none"|"circle"|"gradient", changesSize, renders, tooltip, defaultEntry, params: [{name, label, kind: "number"|"choice"|"colour"|"flag"|"strokes", min?, max?, choices?, none?, log, default}]}]}"##,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "photo.effect.pick",
            label: "Pick a Colour from the Photo",
            menu: "",
            keys: &[],
            params: r##"{"id":PhotoId, "x":0…1, "y":0…1 (normalised to the photo in the stored frame, as an effect's points are)}: starts an analysis job (answers {job, pending:true}); its result is {id, x, y, color:"#rrggbb"|null}: the mean colour of a small disc around the point of the photo as shown before the effect being set (its recipe so far), for the eyedropper and Vignette's pick from the photo"##,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: pick,
        },
    ]
}

// --- effect.list ------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    tab: Option<u8>,
}

fn list(_: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let List { tab } = params(p)?;
    let tabs: Vec<u8> = match tab {
        None => catalog::TABS.to_vec(),
        Some(t) if catalog::TABS.contains(&t) => vec![t],
        Some(t) => return Err(CmdError::BadParams(format!("tab: one of the effect tabs 3, 4 and 5, not {t}"))),
    };
    let effects: Vec<Value> = tabs.into_iter().flat_map(catalog::effects).map(|(op, e)| effect_json(op, e)).collect();
    Ok(json!({ "effects": effects }))
}

/// One effect as `effect.list` gives it.
pub fn effect_json(op: &OpInfo, e: Tile) -> Value {
    let crosshair = match e.crosshair {
        Crosshair::None => "none",
        Crosshair::Circle => "circle",
        Crosshair::Gradient => "gradient",
    };
    json!({
        "name": op.name,
        "label": op.label,
        "tab": e.tab,
        "place": e.place,
        "oneClick": e.one_click,
        "crosshair": crosshair,
        "changesSize": op.changes_size,
        "renders": op.renders(),
        "tooltip": e.tooltip,
        "defaultEntry": op.default_entry(),
        "params": op.params.iter().map(param_json).collect::<Vec<_>>(),
    })
}

fn param_json(p: &Param) -> Value {
    let mut v = json!({ "name": p.name, "label": p.label, "default": p.default, "log": p.log });
    let kind = match p.kind {
        Kind::Number { min, max } => json!({ "kind": "number", "min": min, "max": max }),
        Kind::Open => json!({ "kind": "number" }),
        Kind::Choice(choices) => json!({ "kind": "choice", "choices": choices }),
        Kind::Colour { none } => json!({ "kind": "colour", "none": none }),
        Kind::Flag => json!({ "kind": "flag" }),
        Kind::Seconds => json!({ "kind": "seconds" }),
        Kind::Strokes { max, fields } => json!({ "kind": "strokes", "max": max, "fields": fields.iter().map(param_json).collect::<Vec<_>>() }),
    };
    if let (Some(o), Value::Object(k)) = (v.as_object_mut(), kind) {
        o.extend(k);
    }
    v
}

// --- photo.effect.pick --------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pick {
    id: PhotoId,
    x: f64,
    y: f64,
}

/// What the worker needs, read from the library on the owner thread.
struct Request {
    id: PhotoId,
    file: dalica_thumbs::ladder::Source,
    edits: Vec<String>,
    at: Point,
    options: DecodeOptions,
    limits: Limits,
}

fn pick(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Pick { id, x, y } = params(p)?;
    let inside = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
    if !(inside(x) && inside(y)) {
        return Err(CmdError::BadParams("x and y: numbers from 0 to 1 (the photo in the stored frame)".into()));
    }
    let photo = s.library().photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    let folder = s.library().folder(photo.folder).ok_or_else(|| CmdError::NotFound(format!("no folder for photo {id}")))?;
    let file = dalica_thumbs::ladder::Source { path: folder.path.join(&photo.name), file_size: photo.size, mtime_ms: photo.mtime.as_millis() };
    let request = Request {
        id,
        file,
        edits: s.library().info(id).edits.clone(),
        at: Point { x, y },
        options: s.thumbs.render().decode_options(),
        limits: s.thumbs.screen_source().3,
    };
    if s.jobs.answered_running(JobKind::Analysis, PICKING) >= MAX_PICKS {
        return Err(CmdError::Limit(format!("at most {MAX_PICKS} colour picks at once")));
    }
    let now = s.clock.now();
    let (job, cancel, answer) = s.jobs.start_answered(JobKind::Analysis, PICKING, true, now);
    let spawned = std::thread::Builder::new().name("dalica-effect-pick".into()).spawn(move || work(&request, &cancel, &answer));
    if let Err(e) = spawned {
        // The worker's sender went with the closure: the job ends as failed at the next poll.
        return Err(CmdError::Limit(format!("cannot start a worker: {e}")));
    }
    Ok(json!({ "job": job, "pending": true }))
}

/// The worker: the answer, or why there is none, sent once.
fn work(request: &Request, cancel: &AtomicBool, answer: &Sender<Answer>) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| colour(request, cancel)))
        .unwrap_or_else(|_| Err("the colour pick stopped unexpectedly".to_string()));
    let answered = match outcome {
        Ok(Some(v)) => (Outcome::Done(v), "Picked a colour".to_string()),
        Ok(None) => (Outcome::Cancelled(json!({})), "Colour pick cancelled".to_string()),
        Err(e) => (Outcome::Failed(e), "Could not pick a colour".to_string()),
    };
    // The session may have closed meanwhile; nobody is waiting then.
    let _ = answer.send(answered);
}

/// The answer of `request` (`None` when cancelled on the way).
fn colour(request: &Request, cancel: &AtomicBool) -> Result<Option<Value>, String> {
    let bytes = match dalica_fs::read(&request.file.path, crate::effect_tiles::MAX_FILE) {
        Ok(Some(r)) => r.bytes,
        Ok(None) => return Err("the file is missing".into()),
        Err(e) => return Err(e.to_string()),
    };
    if cancel.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let small = dalica_image::decode_thumbnail_with(&bytes, PICK_EDGE, &request.options, &request.limits).map_err(|e| e.reason())?;
    if cancel.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let found = shown_colour(&small, &request.edits, request.at)?;
    let Point { x, y } = request.at;
    let color = found.map(dalica_core::color::hex_u8);
    Ok(Some(json!({ "id": request.id, "x": x, "y": y, "color": color })))
}

/// The mean colour around the stored-frame point `at` of `stored` (the photo upright, small) as
/// `edits` show it: the recipe rendered, the point carried to where it shows. `None` when the
/// point does not show (cropped away) or nothing visible lies under it.
pub fn shown_colour(stored: &Rgba8, edits: &[String], at: Point) -> Result<Option<[u8; 3]>, String> {
    let recipe = Recipe::parse(edits).map_err(|e| e.to_string())?;
    let shown = dalica_edit::render(dalica_edit::Source::upright(stored), &recipe, Target::Full).map_err(|e| e.to_string())?;
    let geometry = Geometry::new(&recipe, Orientation::default(), stored.width, stored.height);
    let Some(p) = geometry.to_display(at) else { return Ok(None) };
    if !((0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y)) {
        return Ok(None);
    }
    Ok(disc_mean(&shown, p, PICK_RADIUS))
}

/// The mean colour of the visible pixels of `img` within `radius` (of its shorter side, at least
/// one pixel) of the normalised point `p`.
fn disc_mean(img: &Rgba8, p: Point, radius: f64) -> Option<[u8; 3]> {
    let (w, h) = (img.width, img.height);
    let pixels = u64::from(w).checked_mul(u64::from(h))?.checked_mul(4)?;
    if w == 0 || h == 0 || u64::try_from(img.data.len()).ok()? != pixels {
        return None;
    }
    let (cx, cy) = (p.x * f64::from(w), p.y * f64::from(h));
    let r = (f64::from(w.min(h)) * radius).max(1.0);
    let span = |c: f64, n: u32| -> (u32, u32) {
        let lo = (c - r).floor().max(0.0) as u32;
        let hi = ((c + r).ceil() as u32).min(n);
        (lo, hi)
    };
    let ((x0, x1), (y0, y1)) = (span(cx, w), span(cy, h));
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for y in y0..y1 {
        for x in x0..x1 {
            let (dx, dy) = (f64::from(x) + 0.5 - cx, f64::from(y) + 0.5 - cy);
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let i = (y as usize * w as usize + x as usize) * 4;
            if let Some(&[red, green, blue, alpha]) = img.data.get(i..i + 4)
                && alpha > 0
            {
                sum[0] += u64::from(red);
                sum[1] += u64::from(green);
                sum[2] += u64::from(blue);
                n += 1;
            }
        }
    }
    (n > 0).then(|| sum.map(|s| ((s + n / 2) / n).min(255) as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 40 × 20 picture, red on its left half and blue on its right.
    fn halves() -> Rgba8 {
        let data = (0..40 * 20).flat_map(|i| if i % 40 < 20 { [200, 10, 10, 255] } else { [10, 10, 200, 255] }).collect();
        Rgba8 { width: 40, height: 20, data }
    }

    #[test]
    fn a_pick_follows_the_point_through_the_recipes_turns_and_crop() {
        let img = halves();
        let left = Point { x: 0.2, y: 0.5 };
        assert_eq!(shown_colour(&img, &[], left).unwrap(), Some([200, 10, 10]));
        // Turned and flipped, the stored point still names the red half.
        let turned = ["rotate(90)".to_string(), "flip_h()".to_string()];
        assert_eq!(shown_colour(&img, &turned, left).unwrap(), Some([200, 10, 10]));
        assert_eq!(shown_colour(&img, &turned, Point { x: 0.8, y: 0.5 }).unwrap(), Some([10, 10, 200]));
        // Cropped to the right half, the left half no longer shows.
        assert_eq!(shown_colour(&img, &["crop(0.5,0,1,1)".to_string()], left).unwrap(), None);
    }

    #[test]
    fn the_disc_averages_only_what_is_visible_and_inside() {
        let mut img = halves();
        assert_eq!(disc_mean(&img, Point { x: 0.5, y: 0.5 }, 0.2).map(|c| c[0] > 10 && c[2] > 10), Some(true), "both halves");
        for px in img.data.chunks_mut(4) {
            px[3] = 0;
        }
        assert_eq!(disc_mean(&img, Point { x: 0.5, y: 0.5 }, 0.2), None, "nothing visible");
        assert_eq!(disc_mean(&Rgba8 { width: 2, height: 2, data: vec![0; 3] }, Point { x: 0.5, y: 0.5 }, 0.1), None);
    }
}
