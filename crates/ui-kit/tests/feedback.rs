//! The toast (`design-system.md` §6): its kinds and how its text fits.
//!
//! - kinds: info stays 1.4 s with no bar; caution and error stay 6 s with a 4 pt `caution` or
//!   `danger` bar at the plate's left edge (checked on rendered pixels);
//! - fitting: a short text is one line; a longer one wraps to at most three lines and shows whole;
//!   only past three lines is it cut short, in the middle, filling the three lines, and the
//!   characters it keeps whole (a file name) stay whole; hostile widths and huge texts stay bounded.

use dalica_ui_kit::widgets::{TOAST_ERROR_SECS, TOAST_LINES, TOAST_SECS, ToastKind, ToastText, elide_middle, toast, toast_galley, toast_text_width};
use dalica_ui_kit::{Rasterizer, Tokens, registry};
use egui::{Color32, Context, LayerId, Painter, Rect, pos2, vec2};

const NAME: &str = "IMG_0041_from_the_long_trip_to_lisbon.jpg";

/// A context with the tokens and fonts, after one pass (for layout only: its textures are dropped).
fn context() -> (Context, Tokens) {
    let t = Tokens::default();
    let ctx = Context::default();
    t.apply(&ctx);
    ctx.run_ui(dalica_ui_kit::raw_input(400, 300, 1.0), |_| {}).textures_delta.clear();
    (ctx, t)
}

fn galley(ctx: &Context, t: &Tokens, text: ToastText<'_>, area_width: f32) -> (String, usize) {
    let p = Painter::new(ctx.clone(), LayerId::debug(), Rect::EVERYTHING);
    let g = toast_galley(&p, t, text, area_width);
    (g.job.text.clone(), g.rows.len())
}

#[test]
fn kinds_stay_their_time() {
    assert_eq!((ToastKind::Info.life(), ToastKind::Caution.life(), ToastKind::Error.life()), (TOAST_SECS, TOAST_ERROR_SECS, TOAST_ERROR_SECS));
    assert_eq!(ToastKind::default(), ToastKind::Info);
}

#[test]
fn eliding_in_the_middle_keeps_the_start_whole() {
    assert_eq!(elide_middle("abcdef", 0, 6), "abcdef");
    assert_eq!(elide_middle("abcdef", 0, 60), "abcdef");
    assert_eq!(elide_middle("abcdefgh", 0, 4), "ab…gh");
    assert_eq!(elide_middle("abcdefgh", 3, 4), "abc…h", "the kept start stays whole");
    assert_eq!(elide_middle("abcdefgh", 6, 4), "ab…gh", "a start longer than what fits: the plain middle");
    assert_eq!(elide_middle("abcdefgh", 0, 0), "…");
    assert_eq!(elide_middle("ab cd ef", 0, 5), "ab…ef", "no spaces next to the …");
    assert_eq!(elide_middle("ééééé", 0, 2), "é…é", "characters, not bytes");
    assert_eq!(elide_middle("", 4, 0), "");
}

#[test]
fn a_toast_wraps_to_three_lines_then_cuts_in_the_middle() {
    let (ctx, t) = context();
    // Short: one line, as before.
    assert_eq!(galley(&ctx, &t, "Starred 12 photos".into(), 900.0), ("Starred 12 photos".to_string(), 1));
    // Longer: wrapped, whole, at most three lines, at both of the light table's usual widths.
    let msg = format!("Could not write the caption into {NAME}: the file is read-only; the folder sidecar keeps the value");
    for width in [1100.0, 560.0] {
        let (text, rows) = galley(&ctx, &t, msg.as_str().into(), width);
        assert_eq!(text, msg, "whole at {width}");
        assert!((2..=TOAST_LINES).contains(&rows), "{rows} lines at {width}");
    }
    // Too long for three lines: cut in the middle, the file name whole, as much shown as fits.
    let long = format!("Could not write the caption into {NAME}: {}", "the volume reported an error it could not explain; ".repeat(8));
    let keep = long.find(NAME).unwrap() + NAME.len();
    for width in [1100.0, 600.0, 300.0] {
        let (text, rows) = galley(&ctx, &t, ToastText::new(&long, keep), width);
        assert_eq!(rows, TOAST_LINES, "{text}");
        assert!(text.starts_with(&long[..keep]) && text.contains('…'), "{text}");
        assert!(long.ends_with(text.split('…').nth(1).unwrap()), "the end of the text follows the …: {text}");
    }
    // Hostile: a huge text and widths that are not numbers stay bounded.
    let huge = "x".repeat(100_000);
    for width in [f32::NAN, f32::INFINITY, -5.0, 0.0] {
        assert!(toast_text_width(&t, width) >= 40.0);
        let (text, rows) = galley(&ctx, &t, huge.as_str().into(), width);
        assert!(rows <= TOAST_LINES && text.chars().count() < 200, "{width}: {rows} lines");
    }
}

/// Renders one toast of `kind` in a 600 × 200 window; the plate's rect and the image.
fn render(kind: ToastKind) -> (Rect, dalica_ui_kit::Image) {
    let t = Tokens::default();
    let ctx = Context::default();
    t.apply(&ctx);
    let mut raster = Rasterizer::new();
    let mut clock = 0.0;
    let mut pass = || {
        // Time passes, so the toast's area fades in as it does in the app.
        clock += 0.5;
        let mut raw = dalica_ui_kit::raw_input(600, 200, 1.0);
        raw.time = Some(clock);
        ctx.run_ui(raw, |ui| {
            let _ = toast(
                ui,
                &t,
                "toast:1",
                Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 200.0)),
                "Could not write the caption into a.jpg",
                kind,
                0.0,
                None,
            );
        })
    };
    let mut img = None;
    for _ in 0..4 {
        let out = pass();
        img = Some(dalica_ui_kit::paint(&ctx, &mut raster, out, 600, 200, t.chrome).unwrap());
    }
    let w = registry::widgets(&ctx).into_iter().find(|w| w.id == "toast:1").expect("the toast is registered");
    let plate = Rect::from_min_size(pos2(w.rect[0], w.rect[1]), vec2(w.rect[2], w.rect[3]));
    let img = img.unwrap();
    (plate, img)
}

#[test]
fn caution_and_error_toasts_have_their_bar() {
    let t = Tokens::default();
    let near = |a: [u8; 4], b: Color32| (0..3).all(|i| a[i].abs_diff(b.to_array()[i]) <= 6);
    for (kind, bar) in [(ToastKind::Caution, Some(t.caution)), (ToastKind::Error, Some(t.danger)), (ToastKind::Info, None)] {
        let (plate, img) = render(kind);
        let edge = img.pixel((plate.min.x + 2.0) as u32, plate.center().y as u32).unwrap();
        match bar {
            Some(c) => assert!(near(edge, c), "{kind:?}: {edge:?}"),
            None => assert!(!near(edge, t.caution) && !near(edge, t.danger), "{kind:?}: {edge:?}"),
        }
    }
}
