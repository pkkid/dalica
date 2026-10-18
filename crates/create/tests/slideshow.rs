//! Slideshow sequencing and transition geometry (M11.4, `viewing.md` §1): a virtual clock drives
//! shows through play, pause, a mouse move, the end and Loop; the transitions' frames, Pan and
//! Zoom, the fit and the caption plate are property-tested over sizes and progress; Pan and Zoom
//! moves toward the largest named face, else the largest face, else the centre (M13.23).

use std::sync::Arc;
use std::time::Duration;

use dalica_core::{Timestamp, VirtualClock};
use dalica_create::slideshow::geometry::{CAPTION_BOTTOM, CAPTION_MAX_WIDTH, MAX_UPSCALE};
use dalica_create::slideshow::transition::{PAN_ZOOM_MAX, PAN_ZOOM_MIN, pan_zoom};
use dalica_create::slideshow::{
    FaceBox, Point, Rect, Settings, Show, Size, Slide, State, Transition, caption_plate, face_focus, fit, focus_face, frame, pan_zoom_amount,
};
use proptest::prelude::*;

use crate::common::cases;

/// A show of `len` photos from `start` on a fresh virtual clock at 0.
fn show(len: usize, start: usize, settings: Settings) -> (Arc<VirtualClock>, Show) {
    let clock = Arc::new(VirtualClock::new(Timestamp::EPOCH));
    let show = Show::new(clock.clone(), len, start, settings);
    (clock, show)
}

/// Sets the clock to `ms` milliseconds after the start.
fn at(clock: &VirtualClock, ms: i64) {
    clock.set(Timestamp::from_millis(ms));
}

#[test]
fn a_five_photo_show_plays_pauses_and_stops_at_its_end() {
    let (clock, mut s) = show(5, 0, Settings::default());
    // It starts playing on the first photo, the bar shown for 2 s.
    let v = s.tick();
    assert_eq!((v.index, v.state, v.bar, v.change), (Some(0), State::Playing, true, None));
    assert!(!v.can_previous);
    at(&clock, 1_999);
    assert!(s.tick().bar);
    at(&clock, 2_000);
    assert!(!s.tick().bar);
    // After the 3 s display time the second photo dissolves in over 0.8 s.
    at(&clock, 2_999);
    assert_eq!(s.tick().index, Some(0));
    at(&clock, 3_000);
    let v = s.tick();
    assert_eq!((v.index, v.change, v.transition), (Some(1), Some((0, 0.0)), Transition::Dissolve));
    at(&clock, 3_400);
    assert_eq!(s.tick().change, Some((0, 0.5)));
    at(&clock, 3_800);
    assert_eq!(s.tick().change, None);
    // A mouse move shows the bar for 2.5 s without pausing.
    at(&clock, 4_000);
    s.mouse_moved();
    let v = s.tick();
    assert_eq!((v.bar, v.state), (true, State::Playing));
    at(&clock, 6_000);
    assert_eq!(s.tick().index, Some(2));
    at(&clock, 6_499);
    assert!(s.tick().bar);
    at(&clock, 6_500);
    assert!(!s.tick().bar);
    // Space pauses and shows the bar; the display time freezes.
    at(&clock, 7_000);
    assert!(s.space());
    let v = s.tick();
    assert_eq!((v.state, v.bar), (State::Paused, true));
    assert!((v.motion - 1.0 / 3.0).abs() < 1e-3);
    at(&clock, 20_000);
    let v = s.tick();
    assert_eq!((v.index, v.state, v.bar), (Some(2), State::Paused, false));
    assert!((v.motion - 1.0 / 3.0).abs() < 1e-3);
    // Space resumes: the remaining 2 s, then the rest of the show.
    assert!(s.space());
    at(&clock, 21_999);
    assert_eq!(s.tick().index, Some(2));
    at(&clock, 22_000);
    assert_eq!(s.tick().index, Some(3));
    at(&clock, 25_000);
    assert_eq!(s.tick().index, Some(4));
    // Without Loop the last photo stays and the bar appears, for good.
    at(&clock, 27_999);
    assert_eq!(s.tick().state, State::Playing);
    at(&clock, 28_000);
    let v = s.tick();
    assert_eq!((v.index, v.state, v.bar, v.motion), (Some(4), State::Ended, true, 1.0));
    at(&clock, 100_000);
    let v = s.tick();
    assert_eq!((v.index, v.state, v.bar), (Some(4), State::Ended, true));
    // A click does nothing while the bar shows; Play restarts from the first photo.
    assert!(!s.click());
    assert!(s.play_pause());
    let v = s.tick();
    assert_eq!((v.index, v.state, v.change), (Some(0), State::Playing, Some((4, 0.0))));
    at(&clock, 103_000);
    assert_eq!(s.tick().index, Some(1));
}

#[test]
fn with_loop_the_show_wraps_around() {
    let looped = Settings { looped: true, ..Settings::default() };
    let (clock, mut s) = show(5, 3, looped);
    at(&clock, 3_000);
    assert_eq!(s.tick().index, Some(4));
    at(&clock, 6_000);
    let v = s.tick();
    assert_eq!((v.index, v.state, v.change), (Some(0), State::Playing, Some((4, 0.0))));
    assert!(v.can_previous);
    // A long gap (a sleeping machine) moves on by whole display times, still wrapping.
    at(&clock, 6_000 + 7 * 3_000);
    let v = s.tick();
    assert_eq!((v.index, v.change.map(|c| c.0)), (Some(2), Some(1)));
    // Previous before the first and Next after the last wrap too.
    assert!(s.first_photo());
    assert!(s.previous_photo());
    assert_eq!(s.index(), Some(4));
    assert!(s.next_photo());
    assert_eq!(s.index(), Some(0));
}

#[test]
fn stepping_through_the_show_by_key_wheel_and_click() {
    let (clock, mut s) = show(5, 0, Settings { transition: Transition::Push, ..Settings::default() });
    assert!(!s.previous_photo(), "nothing before the first photo without Loop");
    assert!(s.next_photo());
    let v = s.tick();
    assert_eq!((v.index, v.change, v.transition), (Some(1), Some((0, 0.0)), Transition::Push));
    assert!(s.wheel(-1.0));
    assert_eq!(s.index(), Some(2));
    assert!(s.wheel(3.0));
    assert_eq!(s.index(), Some(1));
    assert!(!s.wheel(0.0) && !s.wheel(f32::NAN));
    assert!(s.last_photo() && !s.last_photo());
    assert_eq!(s.index(), Some(4));
    assert!(s.first_photo() && !s.first_photo());
    // A click advances only while the bar is hidden.
    assert!(!s.click());
    at(&clock, 2_500);
    assert!(s.click());
    assert_eq!(s.index(), Some(1));
    // A step restarts the photo's display time.
    at(&clock, 5_000);
    assert_eq!(s.tick().index, Some(1));
    at(&clock, 5_500);
    assert_eq!(s.tick().index, Some(2));
    // Next on the last photo without Loop ends the show as its timer would.
    assert!(s.last_photo() && s.next_photo());
    assert_eq!(s.tick().state, State::Ended);
    assert!(!s.next_photo());
    // Stepping back from the end shows that photo, paused.
    assert!(s.previous_photo());
    let v = s.tick();
    assert_eq!((v.index, v.state), (Some(3), State::Paused));
    at(&clock, 60_000);
    assert_eq!(s.tick().index, Some(3));
}

#[test]
fn zooming_pauses_and_zooming_out_resumes() {
    let (clock, mut s) = show(5, 0, Settings::default());
    at(&clock, 1_000);
    assert!(s.set_zoomed(true) && !s.set_zoomed(true));
    assert_eq!(s.state(), State::Zoomed);
    at(&clock, 30_000);
    assert_eq!(s.tick().index, Some(0));
    assert!(s.set_zoomed(false) && !s.set_zoomed(false));
    assert_eq!(s.state(), State::Playing);
    at(&clock, 31_999);
    assert_eq!(s.tick().index, Some(0));
    at(&clock, 32_000);
    assert_eq!(s.tick().index, Some(1));
    // Zooming a paused show leaves it paused; a step ends the zoom.
    assert!(s.play_pause());
    assert!(s.set_zoomed(true));
    assert!(s.next_photo());
    assert_eq!(s.state(), State::Paused);
}

#[test]
fn the_display_time_is_clamped_and_a_shorter_one_skips_no_photo() {
    let (clock, mut s) = show(5, 0, Settings { display_secs: 0, ..Settings::default() });
    assert_eq!(s.display(), Duration::from_secs(1));
    s.set_display_secs(1_000);
    assert_eq!(s.display(), Duration::from_secs(60));
    at(&clock, 50_000);
    s.set_display_secs(1);
    assert_eq!(s.tick().index, Some(1), "50 s into a photo, a 1 s display time moves on by one");
    at(&clock, 51_000);
    assert_eq!(s.tick().index, Some(2));
}

#[test]
fn an_empty_show_is_at_its_end_and_ignores_every_input() {
    let (clock, mut s) = show(0, 7, Settings::default());
    let v = s.tick();
    assert_eq!((v.index, v.state, v.bar, v.can_previous), (None, State::Ended, true, false));
    assert!(!s.next_photo() && !s.previous_photo() && !s.first_photo() && !s.last_photo());
    assert!(!s.play_pause() && !s.space() && !s.click() && !s.set_zoomed(true) && !s.wheel(1.0));
    at(&clock, i64::MAX);
    assert_eq!(s.tick().index, None);
    // A start past the end is the last photo; a clock at its limit answers without a panic.
    let (clock, mut s) = show(3, 99, Settings { looped: true, ..Settings::default() });
    assert_eq!(s.index(), Some(2));
    at(&clock, i64::MAX);
    assert!(s.tick().index.is_some());
}

/// Rounding room for a check on a screen of `screen`.
fn eps(screen: Size) -> f32 {
    1e-3 * screen.width.abs().max(screen.height.abs()).clamp(1.0, 1e7)
}

fn screen_rect(screen: Size) -> Rect {
    Rect::screen(screen)
}

/// Every check that holds for any layer: clip inside the screen, opacity in 0..1, finite numbers.
fn check_layer(screen: Size, layer: &dalica_create::slideshow::Layer) {
    let full = screen_rect(screen);
    assert!((0.0..=1.0).contains(&layer.opacity), "opacity {}", layer.opacity);
    assert!(full.contains_rect(&layer.clip.rect, eps(screen)), "clip {:?} outside {full:?}", layer.clip.rect);
    for v in [layer.rect.x, layer.rect.y, layer.rect.width, layer.rect.height] {
        assert!(v.is_finite());
    }
    if let Some(c) = layer.clip.circle {
        assert!(c.radius.is_finite() && c.radius > 0.0);
        assert!(full.contains_rect(&Rect::new(c.centre.x, c.centre.y, 0.0, 0.0), eps(screen)));
    }
}

fn transition() -> impl Strategy<Value = Transition> {
    proptest::sample::select(Transition::ALL.to_vec())
}

fn photo() -> impl Strategy<Value = (u32, u32)> {
    (1u32..20_000, 1u32..20_000)
}

fn screen() -> impl Strategy<Value = Size> {
    (1.0f32..8_000.0, 1.0f32..8_000.0).prop_map(|(w, h)| Size::new(w, h))
}

/// Regression (M11.R, proptest seed `cc 9e4e109d…`): the eased progress saturated to exactly 1 in
/// `f32` just short of the end (and underflowed to 0 just after the start), so a Wipe on a 1 × 1
/// screen at 0.9999187 dropped the outgoing photo early. Between 0 and 1 both photos show, on any
/// screen, however close to either end.
#[test]
fn every_transition_keeps_both_photos_until_its_very_end() {
    let near_ends = [1e-30f32, 1e-20, 1e-7, 0.9999187, 1.0 - f32::EPSILON / 2.0];
    for t in Transition::ALL.into_iter().filter(|t| *t != Transition::Cut) {
        for sc in [Size::new(1.0, 1.0), Size::new(7_999.0, 1.0), Size::new(1_000_000.0, 1_000_000.0)] {
            for progress in near_ends {
                let f = frame(t, sc, 1.0, Some(&Slide::new((1, 1))), &Slide::new((1, 1)), progress);
                assert!(f.outgoing.is_some() && f.incoming.is_some(), "{t:?} on {sc:?} at {progress}: {f:?}");
                for layer in [f.outgoing, f.incoming].iter().flatten() {
                    check_layer(sc, layer);
                }
            }
        }
    }
}

/// The part of the photo a Pan and Zoom frame shows, by its centre in the photo (normalised).
fn shown_centre(slide: &Slide, screen: Size) -> (f32, f32) {
    let layer = frame(Transition::PanAndZoom, screen, 1.0, None, slide, 1.0).incoming.expect("the photo shows");
    let c = layer.clip.rect.centre();
    ((c.x - layer.rect.x) / layer.rect.width, (c.y - layer.rect.y) / layer.rect.height)
}

#[test]
fn pan_and_zooms_end_frame_is_centred_nearer_the_largest_named_face_than_the_centre() {
    let named = FaceBox { rect: Rect::new(0.7, 0.15, 0.1, 0.15), named: true };
    let larger = FaceBox { rect: Rect::new(0.1, 0.55, 0.2, 0.25), named: false };
    let smaller_named = FaceBox { rect: Rect::new(0.45, 0.6, 0.05, 0.08), named: true };
    // The largest named face, else the largest face, else the centre; faces off the photo count for
    // nothing.
    assert_eq!(focus_face(&[larger, smaller_named, named]), Some(2));
    assert_eq!(focus_face(&[larger, FaceBox { named: false, ..named }]), Some(0));
    let off = FaceBox { rect: Rect::new(2.0, f32::NAN, 0.5, 0.5), named: true };
    assert_eq!((focus_face(&[]), focus_face(&[off])), (None, None));
    assert_eq!(face_focus(&[off]), Point::CENTRE);
    let focus = face_focus(&[larger, named]);
    assert!((focus.x - 0.75).abs() < 1e-6 && (focus.y - 0.225).abs() < 1e-6, "{focus:?}");

    let face = (0.75f32, 0.225f32);
    let from_face = |(u, v): (f32, f32)| (u - face.0).hypot(v - face.1);
    let start = from_face((0.5, 0.5));
    for (size, screen) in
        [((4000, 3000), Size::new(1600.0, 900.0)), ((3000, 4000), Size::new(1920.0, 1080.0)), ((1200, 800), Size::new(800.0, 600.0))]
    {
        for index in 0..6 {
            let slide = Slide { focus, zoom: pan_zoom_amount(index), motion: 1.0, ..Slide::new(size) };
            let end = shown_centre(&slide, screen);
            // Each photo grows by 5 % or more about the face: its view moves at least 1/21 of the way.
            assert!(from_face(end) < start * (1.0 - 0.047), "{size:?} #{index}: the end frame is centred at {end:?}");
            // Without a face it closes in on the centre.
            let centred = shown_centre(&Slide { focus: face_focus(&[]), ..slide }, screen);
            assert!((centred.0 - 0.5).abs() < 1e-4 && (centred.1 - 0.5).abs() < 1e-4, "{centred:?}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(1024)))]

    #[test]
    fn every_transition_starts_on_the_outgoing_photo_and_ends_on_the_incoming_one(
        t in transition(), sc in screen(), a in photo(), b in photo(), progress in 0.0f32..=1.0, motion in 0.0f32..=1.0,
    ) {
        let out = Slide { motion, ..Slide::new(a) };
        let inc = Slide { motion, zoom: 0.08, focus: Point::new(0.2, 0.7), ..Slide::new(b) };
        let start = frame(t, sc, 1.0, Some(&out), &inc, 0.0);
        prop_assert!(start.outgoing.is_some() && start.incoming.is_none(), "{t:?} at 0: {start:?}");
        let end = frame(t, sc, 1.0, Some(&out), &inc, 1.0);
        prop_assert!(end.outgoing.is_none() && end.incoming.is_some(), "{t:?} at 1: {end:?}");
        let mid = frame(t, sc, 1.0, Some(&out), &inc, progress);
        for layer in [mid.outgoing, mid.incoming, start.outgoing, end.incoming].iter().flatten() {
            check_layer(sc, layer);
        }
        if t != Transition::Cut && progress > 0.0 && progress < 1.0 {
            prop_assert!(mid.outgoing.is_some() && mid.incoming.is_some(), "{t:?} at {progress}: {mid:?}");
        }
    }

    #[test]
    fn pushs_two_photos_never_overlap(sc in screen(), a in photo(), b in photo(), progress in 0.0f32..=1.0) {
        let f = frame(Transition::Push, sc, 1.0, Some(&Slide::new(a)), &Slide::new(b), progress);
        if let (Some(o), Some(i)) = (f.outgoing, f.incoming) {
            let e = eps(sc);
            let rects = o.rect.intersect(&i.rect);
            let clips = o.clip.rect.intersect(&i.clip.rect);
            prop_assert!(rects.width <= e || rects.height <= e, "{:?} and {:?} overlap", o.rect, i.rect);
            prop_assert!(clips.width <= e || clips.height <= e);
        }
    }

    #[test]
    fn pan_and_zoom_stays_within_five_to_ten_percent(
        sc in screen(), a in photo(), zoom in -1.0f32..1.0, fx in -1.0f32..2.0, fy in -1.0f32..2.0, motion in 0.0f32..=1.0,
    ) {
        let fitted = fit(a, sc, 1.0);
        let (rect, clip) = pan_zoom(fitted, Point::new(fx, fy), zoom, motion);
        let grown = rect.width / fitted.width - 1.0;
        prop_assert!((-1e-4..=PAN_ZOOM_MAX + 1e-4).contains(&grown), "grew {grown}");
        prop_assert!(rect.contains_rect(&fitted, eps(sc)), "the zoomed photo always covers its frame");
        prop_assert_eq!(clip, fitted);
        let (end, _) = pan_zoom(fitted, Point::new(fx, fy), zoom, 1.0);
        let full = end.width / fitted.width - 1.0;
        prop_assert!((PAN_ZOOM_MIN - 1e-4..=PAN_ZOOM_MAX + 1e-4).contains(&full), "at the end grew {full}");
        // The move closes in on the focus point: it stays where it was on the screen.
        let (cx, cy) = (fx.clamp(0.0, 1.0), fy.clamp(0.0, 1.0));
        let before = (fitted.x + cx * fitted.width, fitted.y + cy * fitted.height);
        let after = (end.x + cx * end.width, end.y + cy * end.height);
        prop_assert!((before.0 - after.0).abs() <= eps(sc) && (before.1 - after.1).abs() <= eps(sc));
    }

    #[test]
    fn the_fit_is_centred_inside_the_screen_and_never_past_twice_the_pixels(
        sc in screen(), a in photo(), ppp in prop_oneof![Just(1.0f32), Just(2.0), 0.5f32..3.0],
    ) {
        let r = fit(a, sc, ppp);
        let e = eps(sc);
        prop_assert!(screen_rect(sc).contains_rect(&r, e));
        let scale = f64::from(r.width) * f64::from(ppp) / f64::from(a.0);
        prop_assert!(scale <= MAX_UPSCALE * (1.0 + 1e-5), "scale {scale}");
        prop_assert!((r.x - (sc.width - r.right())).abs() <= e && (r.y - (sc.height - r.bottom())).abs() <= e, "not centred: {r:?}");
        // It is as large as it may be: it touches two sides of the screen or reaches 2×.
        let touches = (r.width - sc.width).abs() <= e || (r.height - sc.height).abs() <= e;
        prop_assert!(touches || scale >= MAX_UPSCALE * (1.0 - 1e-4));
    }

    #[test]
    fn the_caption_plate_is_centred_low_and_inside_the_screen(sc in screen(), tw in 0.0f32..20_000.0, th in 0.0f32..400.0) {
        let p = caption_plate(sc, Size::new(tw, th));
        let e = eps(sc);
        prop_assert!(screen_rect(sc).contains_rect(&p, e));
        prop_assert!(p.width <= sc.width * CAPTION_MAX_WIDTH + e);
        prop_assert!((p.centre().x - sc.width / 2.0).abs() <= e);
        // Its bottom sits CAPTION_BOTTOM of the height above the edge, unless the plate is too tall.
        let low = sc.height * (1.0 - CAPTION_BOTTOM);
        prop_assert!((p.bottom() - low).abs() <= e || (p.y <= e && p.bottom() >= low - e));
    }
}

#[test]
fn hostile_sizes_answer_without_a_panic() {
    let sides = [0.0, -5.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, 1e-30, 1.0, 1920.0];
    let photos = [(0, 0), (0, 100), (100, 0), (1, 1), (u32::MAX, u32::MAX), (u32::MAX, 1), (1, u32::MAX)];
    let progresses = [f32::NAN, -1.0, 0.0, 0.5, 1.0, 2.0, f32::INFINITY];
    for w in sides {
        for h in sides {
            let sc = Size::new(w, h);
            let sane = sc.sane();
            for &p in &photos {
                let r = fit(p, sc, f32::NAN);
                assert!(screen_rect(sane).contains_rect(&r, eps(sane)), "{sc:?} {p:?}: {r:?}");
                let plate = caption_plate(sc, Size::new(f32::INFINITY, f32::NAN));
                assert!(plate.width.is_finite() && plate.height.is_finite());
                for t in Transition::ALL {
                    for &progress in &progresses {
                        let slide = Slide { zoom: f32::NAN, motion: f32::NAN, focus: Point::new(f32::NAN, f32::INFINITY), ..Slide::new(p) };
                        let f = frame(t, sc, 0.0, Some(&slide), &slide, progress);
                        for layer in [f.outgoing, f.incoming].iter().flatten() {
                            check_layer(sane, layer);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn transitions_have_their_names_ids_and_times() {
    assert_eq!(Transition::default(), Transition::Dissolve);
    let labels: Vec<_> = Transition::ALL.iter().map(|t| t.label()).collect();
    assert_eq!(labels, ["Cut", "Pan and Zoom", "Dissolve", "Wipe", "Circle", "Push", "Rectangle"]);
    for t in Transition::ALL {
        assert_eq!(Transition::from_id(t.id()), Some(t));
        let expected = if t == Transition::Cut { Duration::ZERO } else { Duration::from_millis(800) };
        assert_eq!(t.duration(), expected);
    }
    assert_eq!(Transition::from_id("fade"), None);
}
