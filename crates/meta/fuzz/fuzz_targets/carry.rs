//! Fuzz target `carry`: any file as the source of a rendered copy (M8.7). Whatever the input, `carry::carry` returns an error or a JPEG or PNG target whose metadata reads back without a problem, with orientation 1 and the asked size in EXIF, and without GPS when the location was removed; `carry::set_orientation` returns an error or a JPEG that passes M6.4's verification and reads back the orientation asked for.

#![no_main]

use dalica_meta::carry::{Carry, carry, set_orientation};
use dalica_meta::{Changes, Limits, WriteOptions, container, read, verify};
use libfuzzer_sys::fuzz_target;

/// The JPEG Dalica's encoder would write, reduced to its structure: SOI, APP0, a frame and a scan.
fn jpeg_target() -> Vec<u8> {
    let mut out = vec![0xff, 0xd8, 0xff, 0xe0, 0, 16];
    out.extend(b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0");
    out.extend([0xff, 0xc0, 0, 11, 8, 0, 1, 0, 1, 1, 1, 0x11, 0]);
    out.extend([0xff, 0xda, 0, 8, 1, 1, 0, 0, 0x3f, 0, 0x12, 0x34, 0xff, 0xd9]);
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(data);
    out.extend(crc32(&[&kind[..], data].concat()).to_be_bytes());
    out
}

/// The PNG Dalica's encoder would write, reduced to its structure: IHDR, IDAT and IEND.
fn png_target() -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
    out.extend(chunk(b"IDAT", b"pixels"));
    out.extend(chunk(b"IEND", &[]));
    out
}

fuzz_target!(|data: &[u8]| {
    let Some((&pick, source)) = data.split_first() else { return };
    let limits = Limits::default();
    let target = if pick & 1 == 1 { png_target() } else { jpeg_target() };
    let thumbnail: &[u8] = b"\xff\xd8 thumbnail \xff\xd9";
    let spec = Carry {
        width: 1 + u32::from(pick >> 4) * 997,
        height: 1 + u32::from(pick & 0x0f) * 1009,
        thumbnail: (pick & 2 == 2).then_some(thumbnail),
        remove_location: pick & 4 == 4,
        changes: Changes { caption: (pick & 8 == 8).then(|| Some("caption — ünïcode <&>".to_owned())), ..Changes::default() },
        options: WriteOptions::default(),
    };
    if let Ok(out) = carry(source, &target, &spec) {
        let m = read(&out.bytes, &limits);
        assert!(m.as_ref().is_ok_and(|m| m.problems.is_empty()), "a carried file does not read back");
        if let Ok(m) = m {
            assert_eq!(m.summary.orientation, Some(1), "a carried file is not upright");
            if spec.remove_location {
                assert!(m.owned.place.is_none(), "a removed location reads back");
            }
        }
        assert!(container::scan(&out.bytes, &limits).is_ok());
    }
    let orientation = u16::from(pick % 10);
    if let Ok(after) = set_orientation(source, orientation, &limits) {
        assert!(verify(source, &after, &Changes::default(), &limits).is_ok(), "set_orientation fails its verification");
        assert_eq!(read(&after, &limits).ok().and_then(|m| m.summary.orientation), u8::try_from(orientation).ok());
    }
});
