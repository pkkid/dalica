//! The 27 effects' renders (`development.md` §8, `roadmap.md` M12.21). Run it with
//!
//! ```sh
//! cargo xtask bench effects                                        # release, a 24 MP photo
//! cargo bench -p dalica-edit --bench effects -- --json
//! cargo bench -p dalica-edit --bench effects -- --only glow,zoom --check   # some effects only
//! cargo bench -p dalica-edit --bench effects -- --check --plant-ms 400     # must fail
//! ```
//!
//! A `scenes` photo of `--width × --height` (6000 × 4000, 24 MP) is made in memory (not timed).
//! Each effect renders at its defaults (`OpInfo::default_entry`), alone in the recipe, the way the
//! engine renders it:
//!
//! - **On screen** (the edit view's preview fitted to `--fit`, 2560 × 1600, as the `edit_preview`
//!   bench): the photo halved to between one and two screen sizes (3000 × 2000, the preview's
//!   source), then `--samples` (21) times a **draft** and a **final**, as a slider drag renders
//!   them. The final renders at the largest power-of-two scale of the source that still covers the
//!   box (`Target::Full` on that base; a frame that grows the picture renders from the source at
//!   that scale); the draft at the scale the engine picks from the last final's time (full size
//!   when it took at most 8 ms, half when a quarter of it does, else a quarter: `preview.rs`'s
//!   `DRAFT_TARGET_MS` and `MIN_DRAFT_SCALE`). Only the renderer is timed (the engine's histogram
//!   and the wait for a worker are not).
//! - **Save**: `--saves` (3) renders of the whole photo at `Target::Full`, as Save renders it.
//!
//! Rows (p50/p95/max by nearest rank): `<effect> draft` and `<effect> final` (printed, not
//! budgeted: the slider's 16 ms draft and 100 ms final at p95 are per-effect speed tasks of M13, so
//! `cargo xtask bench` tracks them meanwhile), `<effect> save` (judged on the median, with
//! `budget_ms`), and an empty recipe's `no effect final` (judged on p95) and `no effect save`
//! (judged on the median). `cargo xtask budgets` judges the report; with `--check` the bench judges
//! it itself and exits non-zero when a budget is missed (in release, at 24 MP and the 2560 × 1600
//! screen, every effect measured); `--plant-ms N` adds `N` ms to every judged time. `--only a,b` measures only the effects named. Without `--bench` (under `cargo test
//! --benches`) it runs a smoke pass: a 600 × 400 photo on a 256 × 160 screen, one sample each.

use std::time::Instant;

use dalica_edit::{Geometry, Recipe, Source, Target, catalog, render};
use dalica_image::{Orientation, Rgba8};
use dalica_scenes::Scene;

/// The budgets (`development.md` §8): per effect, its 24 MP Save render's median, in ms, 1.5 × the
/// first measurement (release on two cores, `taskset -c 0,1`, 2026-10-18, the median of three runs),
/// rounded up to 5 ms. A draft's and a final's speed on the screen are printed, not budgeted (the
/// maintainer at M12.R: they are per-effect speed tasks of M13).
const BUDGETS: [(&str, f64); 27] = [
    ("sharpen", 1225.0),
    ("sepia", 485.0),
    ("bw", 275.0),
    ("warmify", 1485.0),
    ("grain", 870.0),
    ("tint", 460.0),
    ("saturation", 2060.0),
    ("soft_focus", 1280.0),
    ("glow", 2565.0),
    ("filtered_bw", 240.0),
    ("focal_bw", 505.0),
    ("graduated_tint", 370.0),
    ("infrared", 245.0),
    ("plastic_lens", 2295.0),
    ("hdr", 3775.0),
    ("widescreen", 765.0),
    ("dream_glow", 2250.0),
    ("invert", 240.0),
    ("duotone", 500.0),
    ("boost", 2460.0),
    ("soften", 2700.0),
    ("vignette", 780.0),
    ("pixelate", 670.0),
    ("focal_zoom", 22355.0),
    ("border", 470.0),
    ("drop_shadow", 340.0),
    ("museum_matte", 390.0),
];
/// An empty recipe's final on the screen (p95) and its Save render (median), 1.5 × the first
/// measurement, rounded up to 1 and 5 ms.
const EMPTY_FINAL_BUDGET_MS: f64 = 12.0;
const EMPTY_SAVE_BUDGET_MS: f64 = 55.0;
/// The photo and the screen the budgets hold for.
const FULL: (u32, u32) = (6000, 4000);
const SCREEN: (u32, u32) = (2560, 1600);
/// The engine's draft rule (`dalica_engine::preview::DRAFT_TARGET_MS`): a draft renders smaller
/// when the last final took longer than this.
const DRAFT_TARGET_MS: f64 = 8.0;

struct Args {
    width: u32,
    height: u32,
    fit: (u32, u32),
    samples: usize,
    saves: usize,
    only: Option<Vec<String>>,
    json: bool,
    check: bool,
    plant: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        width: if full { FULL.0 } else { 600 },
        height: if full { FULL.1 } else { 400 },
        fit: if full { SCREEN } else { (256, 160) },
        samples: if full { 21 } else { 1 },
        saves: if full { 3 } else { 1 },
        only: None,
        json: false,
        check: false,
        plant: 0.0,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--width" => a.width = value()?.parse().map_err(|e| format!("--width: {e}"))?,
            "--height" => a.height = value()?.parse().map_err(|e| format!("--height: {e}"))?,
            "--fit" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--fit takes WxH")?;
                a.fit = (w.parse().map_err(|e| format!("--fit: {e}"))?, h.parse().map_err(|e| format!("--fit: {e}"))?);
            }
            "--samples" => a.samples = value()?.parse().map_err(|e| format!("--samples: {e}"))?,
            "--saves" => a.saves = value()?.parse().map_err(|e| format!("--saves: {e}"))?,
            "--only" => a.only = Some(value()?.split(',').map(str::to_string).collect()),
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--plant-ms" => a.plant = value()?.parse().ok().filter(|p: &f64| p.is_finite() && *p >= 0.0).ok_or("--plant-ms takes milliseconds")?,
            "--bench" => {}
            other => {
                return Err(format!(
                    "unknown argument `{other}` (use --width PX, --height PX, --fit WxH, --samples N, --saves N, --only a,b, --check, --plant-ms N, --json)"
                ));
            }
        }
    }
    if !(16..=12_000).contains(&a.width) || !(16..=12_000).contains(&a.height) {
        return Err("--width and --height must be 16..=12000".into());
    }
    if !(16..=8192).contains(&a.fit.0) || !(16..=8192).contains(&a.fit.1) {
        return Err("--fit must be 16..=8192 each way".into());
    }
    if !(1..=1000).contains(&a.samples) || !(1..=100).contains(&a.saves) {
        return Err("--samples must be 1..=1000 and --saves 1..=100".into());
    }
    if let Some(only) = &a.only {
        for name in only {
            if !BUDGETS.iter().any(|(n, ..)| n == name) {
                return Err(format!("--only: no effect `{name}`"));
            }
        }
    }
    Ok(a)
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

/// The picture halved: each output pixel the rounded mean of a 2 × 2 block (an odd last row or
/// column dropped), as the engine's preview halves its source.
fn halve(image: &Rgba8) -> Rgba8 {
    let (w, h) = (image.width as usize, image.height as usize);
    let (ow, oh) = (w / 2, h / 2);
    let mut data = vec![0u8; ow * oh * 4];
    for y in 0..oh {
        for x in 0..ow {
            for c in 0..4 {
                let at = |yy: usize, xx: usize| u16::from(image.data[(yy * w + xx) * 4 + c]);
                let sum = at(2 * y, 2 * x) + at(2 * y, 2 * x + 1) + at(2 * y + 1, 2 * x) + at(2 * y + 1, 2 * x + 1);
                data[(y * ow + x) * 4 + c] = ((sum + 2) / 4) as u8;
            }
        }
    }
    Rgba8 { width: ow as u32, height: oh as u32, data }
}

/// The preview's source of `photo` and its halved copies (`bases[n]`: halved `n` times).
struct Screen {
    bases: Vec<Rgba8>,
}

impl Screen {
    /// `photo` halved to between one and two screen sizes (the longer side of `fit`), then halved
    /// twice more for the drafts.
    fn new(photo: &Rgba8, fit: (u32, u32)) -> Screen {
        let screen = fit.0.max(fit.1);
        let mut source = halve(photo);
        if photo.width.max(photo.height) <= screen.saturating_mul(2) {
            source = photo.clone();
        }
        while source.width.max(source.height) > screen.saturating_mul(2) {
            source = halve(&source);
        }
        let mut bases = vec![source];
        for _ in 0..4 {
            let next = bases.last().map(halve);
            bases.extend(next.filter(|b| b.width >= 1 && b.height >= 1));
        }
        Screen { bases }
    }

    fn source(&self) -> &Rgba8 {
        &self.bases[0]
    }

    /// The engine's preview render (`preview/jobs.rs`): `halvings` to cover the box, plus one or
    /// two more for a draft at ½ or ¼; `Target::Full` on that base, or the source at that scale
    /// when the recipe changes the picture's size.
    fn render(&self, recipe: &Recipe, fit: (u32, u32), draft_scale: f64) -> Rgba8 {
        let source = self.source();
        let geometry = Geometry::new(recipe, Orientation::default(), source.width, source.height);
        let (w, h) = geometry.size();
        let (dw, dh) = if geometry.orientation().swaps() { (h, w) } else { (w, h) };
        let scale = (f64::from(fit.0) / dw).min(f64::from(fit.1) / dh).min(1.0);
        let mut n = 0u32;
        while n < 16 && scale * f64::from(1u32 << (n + 1)) <= 1.0 {
            n += 1;
        }
        n += if draft_scale >= 1.0 {
            0
        } else if draft_scale >= 0.5 {
            1
        } else {
            2
        };
        let unwarped = (w - f64::from(source.width)).abs() < 1e-9 && (h - f64::from(source.height)).abs() < 1e-9;
        if unwarped && let Some(base) = self.bases.get(n as usize) {
            render(Source::upright(base), recipe, Target::Full).expect("a render")
        } else {
            render(Source::upright(source), recipe, Target::Scale(1.0 / f64::from(1u32 << n))).expect("a render")
        }
    }
}

/// The engine's draft scale after a final that took `ms` (`Preview::draft_scale`).
fn draft_scale(ms: Option<f64>) -> f64 {
    match ms {
        Some(ms) if ms.is_finite() && ms > DRAFT_TARGET_MS => {
            if ms * 0.25 <= DRAFT_TARGET_MS {
                0.5
            } else {
                0.25
            }
        }
        _ => 1.0,
    }
}

fn ms_since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// One row as JSON: p50/p95/max of `times`, its samples, and `extra` fields (already JSON).
fn row(name: &str, mut times: Vec<f64>, extra: &str) -> String {
    times.sort_by(f64::total_cmp);
    format!(
        "{{\"name\":\"{name}\",\"samples\":{},\"p50_ms\":{:.4},\"p95_ms\":{:.4},\"max_ms\":{:.4}{extra}}}",
        times.len(),
        rank(&times, 50),
        rank(&times, 95),
        times.last().copied().unwrap_or(0.0)
    )
}

struct Measured {
    name: &'static str,
    drafts: Vec<f64>,
    finals: Vec<f64>,
    saves: Vec<f64>,
    draft_scale: f64,
}

fn measure(name: &'static str, entry: &str, photo: &Rgba8, screen: &Screen, a: &Args) -> Measured {
    let recipe = Recipe::parse([entry]).expect("an effect at its defaults");
    // One final first, untimed: what the engine's draft rule reads (and the caches warmed).
    let t = Instant::now();
    let _ = screen.render(&recipe, a.fit, 1.0);
    let mut last = Some(ms_since(t));
    let (mut drafts, mut finals, mut scale) = (Vec::new(), Vec::new(), 1.0);
    for _ in 0..a.samples {
        scale = draft_scale(last);
        let t = Instant::now();
        let draft = screen.render(&recipe, a.fit, scale);
        drafts.push(ms_since(t));
        drop(draft);
        let t = Instant::now();
        let final_ = screen.render(&recipe, a.fit, 1.0);
        let ms = ms_since(t);
        drop(final_);
        finals.push(ms);
        last = Some(ms);
    }
    let mut saves = Vec::new();
    for _ in 0..a.saves {
        let t = Instant::now();
        let saved = render(Source::upright(photo), &recipe, Target::Full).expect("a render");
        saves.push(ms_since(t));
        drop(saved);
    }
    Measured { name, drafts, finals, saves, draft_scale: scale }
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("effects: {e}");
            std::process::exit(2);
        }
    };
    let made = Instant::now();
    let rgb = Scene::new(1, a.width, a.height).render().expect("a scene");
    let photo = Rgba8 { width: rgb.width, height: rgb.height, data: rgb.data.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() };
    drop(rgb);
    let screen = Screen::new(&photo, a.fit);
    eprintln!(
        "effects: a {} × {} photo made in {:.1} s; the screen source {} × {}",
        a.width,
        a.height,
        made.elapsed().as_secs_f64(),
        screen.source().width,
        screen.source().height
    );

    let mut rows = Vec::new();
    let mut missed = Vec::new();
    // An empty recipe: what the pipeline costs without an effect.
    let empty = Recipe::parse(Vec::<String>::new()).expect("an empty recipe");
    let (mut finals, mut saves) = (Vec::new(), Vec::new());
    for _ in 0..a.samples {
        let t = Instant::now();
        drop(render(Source::upright(screen.source()), &empty, Target::Full).expect("a render"));
        finals.push(ms_since(t));
    }
    for _ in 0..a.saves {
        let t = Instant::now();
        drop(render(Source::upright(&photo), &empty, Target::Full).expect("a render"));
        saves.push(ms_since(t));
    }
    for (what, judged, budget) in
        [("no effect final p95", rank_of(&finals, 95), EMPTY_FINAL_BUDGET_MS), ("no effect save median", rank_of(&saves, 50), EMPTY_SAVE_BUDGET_MS)]
    {
        if judged + a.plant > budget {
            missed.push(format!("{what}: {:.2} ms over the {budget} ms budget", judged + a.plant));
        }
    }
    rows.push(row("no effect final", finals, &format!(",\"budget_p95_ms\":{EMPTY_FINAL_BUDGET_MS:?}")));
    rows.push(row("no effect save", saves, &format!(",\"budget_ms\":{EMPTY_SAVE_BUDGET_MS:?}")));

    let mut measured = 0usize;
    for tab in catalog::TABS {
        for (op, _) in catalog::effects(tab) {
            if a.only.as_ref().is_some_and(|only| !only.iter().any(|n| n == op.name)) {
                continue;
            }
            let Some(entry) = op.default_entry() else {
                eprintln!("effects: {} has no default entry", op.name);
                std::process::exit(1);
            };
            let Some(&(_, save_budget)) = BUDGETS.iter().find(|(n, ..)| *n == op.name) else {
                eprintln!("effects: {} has no budget", op.name);
                std::process::exit(1);
            };
            let m = measure(op.name, &entry, &photo, &screen, &a);
            measured += 1;
            let (d, f, s) = (rank_of(&m.drafts, 95), rank_of(&m.finals, 95), rank_of(&m.saves, 50));
            eprintln!("  {:<16} draft p95 {d:7.2} ms (at {}), final p95 {f:7.2} ms, save p50 {s:8.1} ms", m.name, m.draft_scale);
            if s + a.plant > save_budget {
                missed.push(format!("{} save median: {:.2} ms over the {save_budget} ms budget", m.name, s + a.plant));
            }
            rows.push(row(&format!("{} draft", m.name), m.drafts, &format!(",\"draft_scale\":{}", m.draft_scale)));
            rows.push(row(&format!("{} final", m.name), m.finals, ""));
            rows.push(row(&format!("{} save", m.name), m.saves, &format!(",\"budget_ms\":{save_budget:?}")));
        }
    }

    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    if a.json {
        println!(
            "{{\"bench\":\"effects\",\"mode\":\"{mode}\",\"photo_px\":[{},{}],\"source_px\":[{},{}],\"fit\":[{},{}],\"effects\":{measured},\"plant_ms\":{:?},\"rows\":[{}]}}",
            a.width,
            a.height,
            screen.source().width,
            screen.source().height,
            a.fit.0,
            a.fit.1,
            a.plant,
            rows.join(",")
        );
    } else {
        for r in &rows {
            println!("{r}");
        }
    }
    if !a.check {
        return;
    }
    if mode != "release" {
        eprintln!("effects: the budgets hold in release only; a {mode} build judges nothing");
        return;
    }
    if (a.width, a.height) < FULL || a.fit != SCREEN || measured != BUDGETS.len() {
        eprintln!(
            "effects: the budgets hold for every effect on a {} × {} photo and a {} × {} screen; nothing judged",
            FULL.0, FULL.1, SCREEN.0, SCREEN.1
        );
        std::process::exit(1);
    }
    for m in &missed {
        eprintln!("over budget: {m}");
    }
    if !missed.is_empty() {
        std::process::exit(1);
    }
    eprintln!("effects: every budget held");
}

fn rank_of(times: &[f64], p: usize) -> f64 {
    let mut t = times.to_vec();
    t.sort_by(f64::total_cmp);
    rank(&t, p)
}
