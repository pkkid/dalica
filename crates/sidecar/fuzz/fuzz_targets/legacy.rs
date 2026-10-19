//! Fuzz target `legacy`: any bytes as a legacy sidecar (`security.md` §4.2, §7.1), under the
//! default limits and under small ones (so every limit is reached). Whatever the input: refused
//! by a named limit, or read; then every typed read the scan makes (the folder, every file, the
//! albums, the contacts) returns, every file listed is found, every rectangle is inside the
//! picture with its corners in order, every contact id and album token has its fixed shape.
//! Never a panic, a hang or an allocation far past the input.

#![no_main]

use dalica_sidecar::legacy::{LegacyLimits, LegacyPerson, LegacySidecar};
use libfuzzer_sys::fuzz_target;

fn rect_ok(r: [f64; 4]) -> bool {
    r.iter().all(|v| (0.0..=1.0).contains(v)) && r[0] <= r[2] && r[1] <= r[3]
}

fn hex_of(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fuzz_target!(|data: &[u8]| {
    let small = LegacyLimits { max_bytes: 1 << 16, max_line: 256, max_sections: 16, max_keys: 16 };
    for limits in [LegacyLimits::default(), small] {
        let Ok(s) = LegacySidecar::parse(data, &limits) else { continue };
        assert!(s.sections().len() <= limits.max_sections);
        let _ = s.folder();
        for name in s.file_names() {
            let f = s.file(name).expect("a listed file is found");
            assert!(f.crop.is_none_or(rect_ok));
            assert!(f.rotation.is_none_or(|r| r <= 3));
            for face in &f.faces {
                assert!(rect_ok(face.rect));
                if let LegacyPerson::Contact(id) = &face.person {
                    assert!(hex_of(id, 16));
                }
            }
            assert!(f.albums.iter().all(|a| hex_of(a, 32)));
        }
        for album in s.albums() {
            assert!(hex_of(&album.token, 32));
            assert!(!album.name.text.trim().is_empty());
        }
        for id in s.contacts().keys() {
            assert!(hex_of(id, 16));
        }
    }
});
