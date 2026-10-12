//! Fuzz target `exif`: an EXIF/TIFF structure through the IFD walk, the owned fields and summary, and the listing; then the same bytes as a JPEG APP1, PNG `eXIf` and WebP `EXIF` block.

#![no_main]

use dalica_meta::{Limits, exif, listing, read};
use libfuzzer_sys::fuzz_target;

/// One container around an EXIF block: JPEG APP1 (when it fits a segment), PNG `eXIf`, WebP `EXIF`.
fn wrapped(block: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if block.len() + 8 <= 0xffff {
        let mut j = vec![0xff, 0xd8, 0xff, 0xe1];
        j.extend(((block.len() + 8) as u16).to_be_bytes());
        j.extend(b"Exif\0\0");
        j.extend(block);
        j.extend([0xff, 0xda, 0, 2, 0xff, 0xd9]);
        out.push(j);
    }
    let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
    p.extend((block.len() as u32).to_be_bytes());
    p.extend(b"eXIf");
    p.extend(block);
    p.extend([0; 4]);
    p.extend([0, 0, 0, 0, b'I', b'E', b'N', b'D', 0, 0, 0, 0]);
    out.push(p);
    let mut w = b"RIFF".to_vec();
    let size = 4 + 8 + block.len() + (block.len() & 1);
    w.extend((size as u32).to_le_bytes());
    w.extend(b"WEBPEXIF");
    w.extend((block.len() as u32).to_le_bytes());
    w.extend(block);
    if block.len() & 1 == 1 {
        w.push(0);
    }
    out.push(w);
    out
}

fuzz_target!(|data: &[u8]| {
    let l = Limits::default();
    let _ = exif::read::read(data, &l);
    let _ = read(data, &l);
    let _ = listing(data, &l);
    for file in wrapped(data) {
        let _ = read(&file, &l);
        let _ = listing(&file, &l);
    }
});
