//! Fuzz target `regions`: MWG face regions (`xmp-interop.md` §6). The input is an XMP packet,
//! read (bounded, never panicking) and then written into with named faces taken from its first
//! bytes: a merge that succeeds must read back exactly the faces written, and writing them again
//! must change nothing. The same packet in a JPEG goes through `write_owned`, whose result must
//! pass its own verification.

#![no_main]

use dalica_core::Rect01;
use dalica_meta::write::{NamedFace, NamedFaces};
use dalica_meta::xmp::{self, regions};
use dalica_meta::{Changes, DateTaken, Limits, WriteOptions, verify, write_owned};
use libfuzzer_sys::fuzz_target;

/// Up to four faces, each from five bytes: a box (corners in tenths) and a name.
fn faces(head: &[u8]) -> NamedFaces {
    let names = ["Ana", "Ben Ödön <&>", "", "Cy\"'"];
    let faces = head
        .chunks_exact(5)
        .take(4)
        .filter_map(|b| {
            let c = |i: usize| f64::from(b.get(i).copied().unwrap_or(0) % 11) / 10.0;
            let rect = Rect01::new(c(0).min(c(2)), c(1).min(c(3)), c(0).max(c(2)), c(1).max(c(3))).ok()?;
            Some(NamedFace { name: names.get(usize::from(b.get(4).copied().unwrap_or(0) % 4)).copied().unwrap_or_default().to_owned(), rect })
        })
        .collect();
    NamedFaces { width: u32::from(head.first().copied().unwrap_or(0)), height: 3000, faces }
}

fuzz_target!(|data: &[u8]| {
    let l = Limits { max_xmp: 1 << 20, ..Limits::default() };
    let Some((&n, rest)) = data.split_first() else { return };
    let split = (usize::from(n) % 21).min(rest.len());
    let (head, packet) = rest.split_at(split);
    let _ = xmp::read(packet, None, &l);
    let named = faces(head);
    let want = regions::wanted(&named, &l);
    let changes = Changes { faces: Some(named), ..Changes::default() };
    if let Some(date) = DateTaken::parse_iso("2026-10-12T09:30:00+02:00")
        && let Ok(Some(merged)) = xmp::merge(Some(packet), &changes, &date, &l)
    {
        let back = xmp::read(merged.bytes(), None, &l).map(|x| x.faces);
        assert!(back.is_ok_and(|f| regions::same_faces(&f, &want)), "a merged packet does not read back its faces");
        assert!(matches!(xmp::merge(Some(merged.bytes()), &changes, &date, &l), Ok(None)), "writing the same faces twice changed the packet");
    }
    let mut file = vec![0xff, 0xd8];
    let payload = [&b"http://ns.adobe.com/xap/1.0/\0"[..], packet].concat();
    if payload.len() + 2 <= 0xffff {
        file.extend([0xff, 0xe1]);
        file.extend(((payload.len() + 2) as u16).to_be_bytes());
        file.extend(payload);
    }
    file.extend([0xff, 0xda, 0, 2, 0xff, 0xd9]);
    let options = WriteOptions { people_names: true, limits: l, ..WriteOptions::default() };
    if let Ok(after) = write_owned(&file, &changes, &options) {
        assert!(verify(&file, &after, &changes.allowed(&options), &l).is_ok(), "a returned write fails its own verification");
    }
});
