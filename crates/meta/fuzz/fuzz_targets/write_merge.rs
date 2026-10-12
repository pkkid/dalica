//! Fuzz target `write_merge`: parse → merge-write → re-parse. Whatever the input, `write_owned` returns an error or bytes that passed its own verification; those bytes must read back, keep every byte outside the rewritable metadata, and hold the requested fields.

#![no_main]

use dalica_meta::{Changes, DateTaken, Limits, Place, WriteOptions, container, read, verify, write_owned};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&pick, file)) = data.split_first() else { return };
    let options = WriteOptions { stars_as_ratings: pick & 1 == 1, limits: Limits::default(), ..WriteOptions::default() };
    let changes = Changes {
        caption: (pick & 2 == 2).then(|| Some("caption — ünïcode <&>".to_owned())),
        tags: (pick & 4 == 4).then(|| vec!["beach".to_owned(), "a/b, c".to_owned()]),
        star: (pick & 8 == 8).then_some(pick & 16 == 16),
        place: (pick & 32 == 32).then(|| Place::new(-33.5, 151.25)),
        date_taken: (pick & 64 == 64).then(|| DateTaken::parse_iso("2026-06-04T19:42:10.250+10:00")).flatten(),
    };
    let Ok(after) = write_owned(file, &changes, &options) else { return };
    let allowed = changes.allowed(&options);
    assert!(verify(file, &after, &allowed, &options.limits).is_ok(), "a returned write fails its own verification");
    let before = container::scan(file, &options.limits).ok();
    let again = container::scan(&after, &options.limits);
    assert!(again.is_ok(), "a returned write does not read back");
    if let (Some(b), Ok(a)) = (before, again) {
        assert_eq!(b.container, a.container);
    }
    assert!(read(&after, &options.limits).is_ok());
});
