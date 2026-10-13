//! The edit view's zoom maths (`editing.md` §3): the slider's ends and midpoint (fit, 100 %,
//! 400 %), monotonic, never below fit, the two half formulas at sample points and their inverse;
//! the steps snap to 100 % when crossing it and end at fit and 400 %; a photo smaller than the
//! area fits at 100 %.

use dalica_ui::commands::zoom::{effective, fit_scale, is_zoomed, relative_at, scale_at, slider_at, step, zoom_for};
use dalica_ui::state::{ZOOM_MAX, Zoom};

const FITS: [f32; 6] = [0.05, 0.125, 0.25, 0.4, 0.9, 1.0];

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-4 * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn the_ends_and_the_midpoint() {
    for fit in FITS {
        assert!(close(scale_at(0.0, fit), fit), "v 0 is fit ({fit})");
        assert!(close(scale_at(0.5, fit), 1.0), "v ½ is 100 % ({fit})");
        assert!(close(scale_at(1.0, fit), 4.0), "v 1 is 400 % ({fit})");
        assert_eq!(slider_at(fit, fit), 0.0);
        assert!(close(slider_at(1.0, fit), 0.5) || fit == 1.0, "{fit}");
        assert!(close(slider_at(4.0, fit), 1.0));
    }
    assert_eq!(ZOOM_MAX, 4.0);
}

#[test]
fn the_formulas_at_sample_points() {
    // Lower half: 1 + (2^(2v) − 1)(r − 1); upper half: r · 2^(4(v − ½)); relative to fit.
    for (v, r, want) in [
        (0.0, 4.0, 1.0),
        (0.25, 4.0, 1.0 + (2f32.sqrt() - 1.0) * 3.0),
        (0.5, 4.0, 4.0),
        (0.75, 4.0, 8.0),
        (1.0, 4.0, 16.0),
        (0.25, 10.0, 1.0 + (2f32.sqrt() - 1.0) * 9.0),
        (0.625, 2.0, 2.0 * 2f32.powf(0.5)),
        (0.1, 1.0, 1.0),
    ] {
        assert!(close(relative_at(v, r), want), "v {v} r {r}: {} ≠ {want}", relative_at(v, r));
    }
    // The absolute scale is fit times that.
    assert!(close(scale_at(0.25, 0.25), 0.25 * (1.0 + (2f32.sqrt() - 1.0) * 3.0)));
    assert!(close(scale_at(0.75, 0.25), 2.0));
}

#[test]
fn monotonic_never_below_fit_and_inverted_by_slider_at() {
    for fit in FITS {
        let mut last = 0.0;
        for i in 0..=1000 {
            let v = i as f32 / 1000.0;
            let s = scale_at(v, fit);
            assert!(s >= last, "monotonic at v {v}, fit {fit}");
            assert!(s >= fit && s <= 4.0, "{s} at v {v}, fit {fit}");
            // A photo that fits at 100 % has no lower half: 100 % reads as fit.
            if fit < 1.0 || v > 0.5 {
                assert!((slider_at(s, fit) - v).abs() < 2e-3, "inverse at v {v}, fit {fit}: {}", slider_at(s, fit));
            }
            last = s;
        }
    }
    // Hostile inputs stay in range.
    for v in [f32::NAN, -1.0, 2.0, f32::INFINITY] {
        let s = scale_at(v, 0.3);
        assert!((0.3..=4.0).contains(&s), "{v}: {s}");
        assert!((0.0..=1.0).contains(&slider_at(v, 0.3)), "{v}");
    }
    for fit in [f32::NAN, 0.0, -1.0, 7.0] {
        assert!(scale_at(0.5, fit).is_finite());
    }
}

#[test]
fn a_photo_smaller_than_the_area_fits_at_100_percent() {
    assert_eq!(fit_scale((400.0, 300.0), (1200.0, 800.0)), 1.0);
    assert!(close(fit_scale((4000.0, 3000.0), (1200.0, 800.0)), 0.26666));
    assert_eq!(fit_scale((0.0, 300.0), (1200.0, 800.0)), 1.0, "unknown size");
    assert_eq!(fit_scale((f32::NAN, 300.0), (1200.0, 800.0)), 1.0);
    // Its lower half is all 100 %; 1:1 is not zoomed; the upper half zooms.
    for v in [0.0, 0.2, 0.5] {
        assert!(close(scale_at(v, 1.0), 1.0));
    }
    assert!(!is_zoomed(Zoom::Scale(1.0), 1.0));
    assert!(is_zoomed(Zoom::Scale(2.0), 1.0));
    assert_eq!(effective(Zoom::Scale(0.1), 0.5), 0.5, "below fit shows fitted");
    assert_eq!(effective(Zoom::Scale(f32::NAN), 0.5), 0.5);
}

#[test]
fn steps_snap_to_100_percent_and_stop_at_the_ends() {
    let fit = 0.3;
    assert_eq!(step(0.95, 1.1, fit), Zoom::Scale(1.0), "crossing 100 % going in");
    assert_eq!(step(1.05, 1.0 / 1.1, fit), Zoom::Scale(1.0), "crossing 100 % going out");
    assert_eq!(step(1.0, 1.1, fit), Zoom::Scale(1.1), "from 100 % it moves on");
    assert_eq!(step(0.32, 1.0 / 1.1, fit), Zoom::Fit, "at fit it is Fit");
    assert_eq!(step(fit, 1.0 / 1.1, fit), Zoom::Fit);
    assert_eq!(step(3.9, 1.1, fit), Zoom::Scale(4.0), "at most 400 %");
    assert_eq!(step(0.5, f32::NAN, fit), Zoom::Scale(0.5), "no factor, no change");
    assert_eq!(step(0.5, 0.0, fit), Zoom::Scale(0.5));
    assert_eq!(zoom_for(0.2, fit), Zoom::Fit);
    assert_eq!(zoom_for(f32::INFINITY, fit), Zoom::Fit);
    // Notch by notch from fit to 400 % and back passes 100 % exactly.
    let mut s = fit;
    let mut hit = false;
    for _ in 0..40 {
        s = effective(step(s, 1.1, fit), fit);
        hit |= s == 1.0;
    }
    assert!(hit && s == 4.0, "{s}");
    for _ in 0..40 {
        s = effective(step(s, 1.0 / 1.1, fit), fit);
    }
    assert_eq!(s, fit);
}
