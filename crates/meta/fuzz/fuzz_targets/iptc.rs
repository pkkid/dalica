//! Fuzz target `iptc`: Photoshop image resources and IPTC-IIM datasets as given, then inside a JPEG APP13 and a TIFF tag 33723.

#![no_main]

use dalica_meta::{Limits, iptc, listing, read};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let l = Limits::default();
    let _ = iptc::read::resources(data, &l);
    let _ = iptc::read::datasets(data, &l);
    let _ = iptc::read::read(data, &l);
    if data.len() + 16 <= 0xffff {
        let mut j = vec![0xff, 0xd8, 0xff, 0xed];
        j.extend(((data.len() + 16) as u16).to_be_bytes());
        j.extend(b"Photoshop 3.0\0");
        j.extend(data);
        j.extend([0xff, 0xda, 0, 2, 0xff, 0xd9]);
        let _ = read(&j, &l);
        let _ = listing(&j, &l);
    }
    // A little-endian TIFF whose IFD0 holds one entry: tag 33723 (UNDEFINED) pointing at the data.
    let mut t = b"II*\0\x08\0\0\0\x01\0\xbb\x83\x07\0".to_vec();
    t.extend((data.len() as u32).to_le_bytes());
    t.extend(26u32.to_le_bytes());
    t.extend(0u32.to_le_bytes());
    t.extend(data);
    let _ = read(&t, &l);
    let _ = listing(&t, &l);
});
