//! Property tests for the rest of `dalica-core` (`architecture.md` §11): collision naming,
//! geometry round trips, id and date round trips, and no panic on any input.

use dalica_core::geom::round6;
use dalica_core::names::{self, MAX_NAME_BYTES};
use dalica_core::{AlbumId, Date, DateTime, FolderId, PersonId, PhotoId, PixelRect, Rect01};
use proptest::prelude::*;

/// Names with the awkward parts over-represented: dots, empty stems, non-ASCII, great length.
fn name() -> impl Strategy<Value = String> {
    prop_oneof![Just(String::new()), "[.]{1,4}", "\\PC{0,20}", "[a-z]{0,8}(\\.[a-zé東]{0,4}){0,3}", "[é東🌊a.]{100,300}",]
}

proptest! {
    #![proptest_config(crate::common::cases(2048, 32))]

    #[test]
    fn collision_names_are_distinct_bounded_and_keep_the_extension(name in name(), a in 1u32..=99_999, b in 1u32..=99_999) {
        let na = names::numbered(&name, a);
        prop_assert!(!na.is_empty());
        prop_assert!(na.len() <= MAX_NAME_BYTES, "{} bytes", na.len());
        prop_assert_ne!(&na, &name);
        if a != b {
            prop_assert_ne!(&na, &names::numbered(&name, b));
        }
        let number = format!("{a:03}");
        match names::split_ext(&name) {
            (_, Some(ext)) if ext.len() + number.len() + 2 < MAX_NAME_BYTES => {
                let tail = format!("{number}.{ext}");
                prop_assert!(na.ends_with(&tail), "{na:?} should end with {tail:?}");
            }
            _ => prop_assert!(na.ends_with(&number), "{na:?} should end with {number:?}"),
        }
        if name.len() + number.len() < MAX_NAME_BYTES {
            let (stem, _) = names::split_ext(&name);
            prop_assert!(na.starts_with(stem), "{na:?} should start with {stem:?}");
        }
    }

    #[test]
    fn next_free_finds_the_first_gap(name in "[a-z]{1,6}\\.jpg", taken in 0usize..50) {
        let used: Vec<String> = names::candidates(&name).take(taken).collect();
        let free = names::next_free(&name, |c| used.iter().any(|u| u == c)).unwrap();
        prop_assert_eq!(free, names::candidates(&name).nth(taken).unwrap());
    }

    #[test]
    fn rects_round_trip_through_json(x0 in 0.0f64..1.0, y0 in 0.0f64..1.0, w in 0.000002f64..1.0, h in 0.000002f64..1.0) {
        let r = Rect01::new(x0, y0, (x0 + w).min(1.0), (y0 + h).min(1.0));
        prop_assume!(r.is_ok());
        let r = r.unwrap();
        let json = serde_json::to_string(&r).unwrap();
        prop_assert_eq!(serde_json::from_str::<Rect01>(&json).unwrap(), r);
        for v in <[f64; 4]>::from(r) {
            prop_assert_eq!(round6(v), v);
        }
    }

    #[test]
    fn rects_round_trip_through_pixels(x0 in 0.0f64..1.0, y0 in 0.0f64..1.0, w in 0.000002f64..1.0, h in 0.000002f64..1.0, iw in 1u32..20_000, ih in 1u32..20_000) {
        let r = Rect01::new(x0, y0, (x0 + w).min(1.0), (y0 + h).min(1.0));
        prop_assume!(r.is_ok());
        let r = r.unwrap();
        let px = r.to_pixels(iw, ih).unwrap();
        prop_assert!(px.width >= 1 && px.height >= 1 && px.x + px.width <= iw && px.y + px.height <= ih, "{px:?} in {iw}x{ih}");
        let back = Rect01::from_pixels(px, iw, ih).unwrap();
        // Each edge moves by at most one pixel, also for rects narrower than a pixel.
        let (tx, ty) = (1.0 / f64::from(iw) + 1e-6, 1.0 / f64::from(ih) + 1e-6);
        prop_assert!((back.x0() - r.x0()).abs() <= tx && (back.x1() - r.x1()).abs() <= tx, "{r:?} -> {px:?} -> {back:?}");
        prop_assert!((back.y0() - r.y0()).abs() <= ty && (back.y1() - r.y1()).abs() <= ty, "{r:?} -> {px:?} -> {back:?}");
    }

    #[test]
    fn pixel_rects_round_trip_exactly(iw in 1u32..100_000, ih in 1u32..100_000, a in any::<prop::sample::Index>(), b in any::<prop::sample::Index>(), c in any::<prop::sample::Index>(), d in any::<prop::sample::Index>()) {
        let (x, y) = (a.index(iw as usize) as u32, b.index(ih as usize) as u32);
        let width = c.index((iw - x) as usize) as u32 + 1;
        let height = d.index((ih - y) as usize) as u32 + 1;
        let px = PixelRect { x, y, width, height };
        let r = Rect01::from_pixels(px, iw, ih).unwrap();
        prop_assert_eq!(r.to_pixels(iw, ih).unwrap(), px);
    }

    #[test]
    fn rect_json_never_panics(json in "\\[[-0-9.eE, ]{0,40}\\]|\\PC{0,30}") {
        let _ = serde_json::from_str::<Rect01>(&json);
    }

    #[test]
    fn ids_round_trip(n in any::<u64>(), hex in "[0-9a-f]{4,16}") {
        let id = PhotoId::new(n);
        prop_assert_eq!(serde_json::from_str::<PhotoId>(&serde_json::to_string(&id).unwrap()).unwrap(), id);
        prop_assert_eq!(id.to_string().parse::<PhotoId>().unwrap(), id);
        prop_assert_eq!(FolderId::new(n).to_string().parse::<FolderId>().unwrap().get(), n);
        let album = AlbumId::parse(&format!("a{}", hex)).unwrap();
        prop_assert_eq!(serde_json::from_str::<AlbumId>(&serde_json::to_string(&album).unwrap()).unwrap(), album);
        let person = PersonId::parse(&format!("p{}", hex));
        prop_assert!(person.is_ok());
    }

    #[test]
    fn id_parsing_never_panics(s in "\\PC{0,24}") {
        let _ = s.parse::<PhotoId>();
        let _ = AlbumId::parse(&s);
        let _ = PersonId::parse(&s);
    }

    #[test]
    fn dates_round_trip(secs in -62_135_596_800i64..253_402_300_800) {
        let dt = DateTime::from_unix_secs(secs).unwrap();
        prop_assert_eq!(dt.to_string().parse::<DateTime>().unwrap(), dt);
        prop_assert_eq!(dt.date().to_string().parse::<Date>().unwrap(), dt.date());
        let json = serde_json::to_string(&dt).unwrap();
        prop_assert_eq!(serde_json::from_str::<DateTime>(&json).unwrap(), dt);
        // Later seconds never give an earlier date-time.
        if let Some(next) = DateTime::from_unix_secs(secs + 1) {
            prop_assert!(next > dt);
        }
    }

    #[test]
    fn date_parsing_never_panics(s in "\\PC{0,24}|[0-9:T -]{0,24}") {
        let _ = s.parse::<DateTime>();
        let _ = s.parse::<Date>();
    }
}
