//! Fuzz target `xmp`: the XMP entry points with a packet as given (read, rows, and the merge writer, whose result must read back), and the JPEG walk with the input split into a standard packet and extended-XMP chunks (reassembly by GUID, bounded), that file then written through `write_owned` (the merge and the JPEG writer), whose result must pass its own verification.

#![no_main]

use dalica_meta::{Changes, DateTaken, Limits, Place, WriteOptions, container, listing, read, verify, write_owned, xmp};
use libfuzzer_sys::fuzz_target;

fn segment(out: &mut Vec<u8>, payload: &[u8]) {
    if payload.len() + 2 <= 0xffff {
        out.extend([0xff, 0xe1]);
        out.extend(((payload.len() + 2) as u16).to_be_bytes());
        out.extend(payload);
    }
}

/// Every owned property, written as text that needs escaping.
fn changes() -> Changes {
    Changes {
        caption: Some(Some("fuzz <caption> & \u{fc}n\u{ef}code \u{fffe}".to_owned())),
        tags: Some(vec!["a/b".to_owned(), "c, d".to_owned(), "\u{65e5}\u{672c}".to_owned()]),
        star: Some(true),
        place: Some(Place::new(-33.5, -151.25)),
        date_taken: DateTaken::parse_iso("2026-06-04T19:42:10.250+10:00"),
        faces: None,
    }
}

fuzz_target!(|data: &[u8]| {
    let l = Limits { max_xmp: 1 << 20, ..Limits::default() };
    let _ = xmp::read(data, None, &l);
    let _ = xmp::rows(data, Some(data), &l);
    let changes = changes();
    if let Some(date) = DateTaken::parse_iso("2026-10-12T09:30:00+02:00")
        && let Ok(Some(merged)) = xmp::merge(Some(data), &changes, &date, &l)
    {
        assert!(xmp::read(merged.bytes(), None, &l).is_ok(), "a merged packet does not read back");
    }
    // First byte: how many chunks (low three bits) and whether their headers are well formed (high
    // bit); the rest split between the packet and the chunks. A hostile chunk takes its declared
    // length and offset from its first eight bytes. The GUID is the MD5 of the chunk bodies
    // joined, so a well-formed set reassembles and reaches the XMP reader.
    let Some((&n, rest)) = data.split_first() else { return };
    let parts: Vec<&[u8]> = rest.chunks(rest.len() / (usize::from(n % 8) + 1) + 1).collect();
    let well_formed = n & 0x80 != 0;
    let bodies: Vec<&[u8]> = parts.iter().skip(1).map(|p| if well_formed { *p } else { p.get(8..).unwrap_or_default() }).collect();
    let guid = container::jpeg::extended_guid(&bodies.concat());
    let total = bodies.iter().map(|b| b.len()).sum::<usize>() as u32;
    let mut file = vec![0xff, 0xd8];
    let main =
        [&b"http://ns.adobe.com/xap/1.0/\0<x xmpNote:HasExtendedXMP=\""[..], &guid, b"\"/>", parts.first().copied().unwrap_or_default()].concat();
    segment(&mut file, &main);
    let mut at = 0u32;
    for (part, body) in parts.iter().skip(1).zip(&bodies) {
        let (full, offset) = if well_formed {
            (total.to_be_bytes(), at.to_be_bytes())
        } else {
            let mut head = [0u8; 8];
            for (d, s) in head.iter_mut().zip(part.iter()) {
                *d = *s;
            }
            ([head[0], head[1], head[2], head[3]], [head[4], head[5], head[6], head[7]])
        };
        at = at.wrapping_add(body.len() as u32);
        let chunk = [&b"http://ns.adobe.com/xmp/extension/\0"[..], &guid, &full, &offset, body].concat();
        segment(&mut file, &chunk);
    }
    file.extend([0xff, 0xda, 0, 2, 0xff, 0xd9]);
    if let Ok(scan) = container::scan(&file, &l) {
        let _ = xmp::read(scan.xmp.as_deref().unwrap_or_default(), scan.extended_xmp.as_deref(), &l);
    }
    let _ = read(&file, &l);
    let _ = listing(&file, &l);
    let options = WriteOptions { stars_as_ratings: true, limits: l, ..WriteOptions::default() };
    if let Ok(after) = write_owned(&file, &changes, &options) {
        assert!(verify(&file, &after, &changes.allowed(&options), &l).is_ok(), "a returned write fails its own verification");
    }
});
