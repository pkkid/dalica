//! Fuzz target `recipe`: the edit-operation lists a sidecar holds (`security.md` §4.2, §7.1): every
//! photo's `edits` and `redo`, every saved copy's `recipe` and every originals entry's `recipe`
//! when the bytes read as a `.dalica.json` or an originals record, and the bytes' own lines as one
//! list (so operation text is reached without JSON around it). The first four bytes give a picture
//! size (a side of 0 included) and an EXIF orientation. Whatever the input:
//!
//! - a list reads as a recipe (or is refused past `MAX_OPERATIONS`) and prints back exactly;
//! - every operation prints its own text, and typed parameters print to text that reads back as
//!   the same parameters, inside or outside their ranges, clamped or not;
//! - the pipeline, the orientation and the shown size (crop counted and not) return; appending
//!   each operation with the rotation and flip rules keeps the recipe within its cap.
//!
//! Never a panic, a hang or an allocation far past the input.

#![no_main]

use dalica_edit::{Cropping, MAX_OPERATIONS, Op, Recipe, Typed};
use dalica_image::Orientation;
use dalica_sidecar::{Field, Limits, Originals, Sidecar};
use libfuzzer_sys::fuzz_target;

fn typed_round_trip(t: &Typed) {
    let again = Typed::parse(t.name(), &t.params());
    assert_eq!(again.as_ref().map(|r| r.as_ref().ok()), Some(Some(t)), "{}({}) does not read back", t.name(), t.params());
    let op = Op::from(t.clone());
    assert_eq!(Op::parse(&op.to_string()), op);
}

fn list(edits: &[String], exif: Orientation, width: u32, height: u32) {
    let Ok(recipe) = Recipe::parse(edits) else {
        assert!(edits.len() > MAX_OPERATIONS);
        return;
    };
    assert_eq!(recipe.print(), edits, "a recipe does not print back");
    let mut pushed = Recipe::new();
    for op in recipe.ops() {
        assert_eq!(Op::parse(&op.to_string()), *op);
        let _ = (op.name(), op.stage(), op.label(), op.version(), op.orientation());
        if let Some(Ok(t)) = op.typed() {
            typed_round_trip(&t);
            typed_round_trip(&t.clamped());
        }
        if let Op::Listed { name, params } = op
            && let Some(Ok(t)) = Typed::parse(name, params)
        {
            typed_round_trip(&t);
        }
        let _ = pushed.push(op.clone());
        assert!(pushed.len() <= MAX_OPERATIONS);
    }
    let _ = recipe.pipeline().count();
    let _ = recipe.display_size(exif, width, height);
    let _ = recipe.shown_size(exif, width, height, Cropping::Counted);
    let _ = recipe.shown_size(exif, width, height, Cropping::Ignored);
    let _ = pushed.shown_size(exif, width, height, Cropping::Counted);
}

fuzz_target!(|data: &[u8]| {
    let Some((head, rest)) = data.split_first_chunk::<4>() else { return };
    let width = u32::from(u16::from_le_bytes([head[0], head[1]]));
    let height = u32::from(head[2]) << (head[3] & 7);
    let exif = Orientation::new((head[3] >> 3) % 8 + 1).unwrap_or_default();
    let lines: Vec<String> = String::from_utf8_lossy(rest).split('\n').map(str::to_owned).collect();
    list(&lines, exif, width, height);
    let limits = Limits::default();
    if let Ok(s) = Sidecar::parse(rest, &limits) {
        let names: Vec<String> = s.photo_names().map(str::to_owned).collect();
        for name in &names {
            let p = s.photo(name);
            for l in [p.edits, p.redo] {
                if let Field::Value(l) = l {
                    list(&l, exif, width, height);
                }
            }
            if let Field::Value(saved) = s.saved_as(name) {
                list(&saved.recipe, exif, width, height);
            }
        }
    }
    if let Ok(o) = Originals::parse(rest, &limits) {
        for (_, e) in o.entries() {
            list(&e.recipe, exif, width, height);
        }
    }
});
