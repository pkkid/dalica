//! IPTC-IIM writing (`xmp-interop.md` §2–§3 and §8 tests 1, 2, 3, 6 and 10): 1:90 written as
//! UTF-8, 2:25 one dataset per tag, 2:120 cut at a character boundary past the IPTC limit, every
//! other dataset and image resource kept byte for byte, the digest resource 0x0425 recomputed, a
//! Latin-1 legacy stream still reading the same, and hostile input that never panics.

mod build;

use build::*;
use dalica_meta::iptc::read::{Charset, datasets, read, resources};
use dalica_meta::iptc::write::{IptcEdit, MAX_CAPTION, MAX_KEYWORD, Truncated, digest, write_resources, write_stream};
use dalica_meta::iptc::{RESOURCE_IPTC, RESOURCE_IPTC_DIGEST, UTF8_ESCAPE};
use dalica_meta::{Changes, Error, Limits};
use proptest::prelude::*;

fn l() -> Limits {
    Limits::default()
}

fn tags(t: &[&str]) -> Vec<String> {
    t.iter().map(|s| (*s).to_owned()).collect()
}

/// Every dataset of a stream as `(record, number, data)`.
fn all(stream: &[u8]) -> Vec<(u8, u8, Vec<u8>)> {
    datasets(stream, &l()).unwrap().into_iter().map(|d| (d.record, d.number, d.data.to_vec())).collect()
}

/// A news agency's stream: envelope, object name, three keywords, by-line, a caption, copyright,
/// caption writer and a binary preview, in dataset order.
fn agency() -> Vec<u8> {
    iim(&[
        (1, 0, &[0, 4]),
        (1, 90, b"\x1b%G"),
        (2, 0, &[0, 4]),
        (2, 5, b"Harbour"),
        (2, 25, b"sea"),
        (2, 25, b"boats"),
        (2, 25, b"dusk"),
        (2, 80, b"J. Doe"),
        (2, 116, "© 2026 Agency".as_bytes()),
        (2, 120, b"Old caption"),
        (2, 122, b"Desk"),
        (2, 202, &[0xff, 0xd8, 0x80, 0xff, 0xd9]),
    ])
}

/// §8 test 1 and 6: a new stream declares UTF-8 and reads back Unicode captions and tags with `/`,
/// `,` and non-ASCII exactly, one 2:25 per tag.
#[test]
fn a_new_stream_declares_utf8() {
    let caption = "Fête de la musique — 東京 «quoted»\nsecond line";
    let t = tags(&["a/b", "x, y", "Zürich", "東京", "naïve café"]);
    let w = write_stream(None, &IptcEdit { caption: Some(Some(caption)), keywords: Some(&t) }, &l()).unwrap();
    assert_eq!(w.truncated, Truncated::default());
    let ds = all(&w.bytes);
    assert_eq!(ds.first(), Some(&(1, 90, UTF8_ESCAPE.to_vec())));
    assert_eq!(ds.get(1), Some(&(2, 0, vec![0, 4])), "record 2 starts with its version");
    assert_eq!(ds.iter().filter(|d| (d.0, d.1) == (2, 25)).count(), t.len());
    let r = read(&w.bytes, &l()).unwrap();
    assert_eq!(r.caption.as_deref(), Some(caption));
    assert_eq!(r.keywords, t);
    assert_eq!(Charset::of(&datasets(&w.bytes, &l()).unwrap()), Charset::Utf8);
}

/// §8 test 2: other datasets keep their bytes and their order; the caption takes the old one's
/// place and the keywords the first old keyword's.
#[test]
fn other_datasets_are_kept_byte_for_byte() {
    let before = agency();
    let t = tags(&["harbour", "night"]);
    let w = write_stream(Some(&before), &IptcEdit { caption: Some(Some("New caption")), keywords: Some(&t) }, &l()).unwrap();
    let mut expected = all(&before);
    expected.retain(|d| (d.0, d.1) != (2, 25));
    let at = expected.iter().position(|d| (d.0, d.1) == (2, 80)).unwrap();
    expected.insert(at, (2, 25, b"night".to_vec()));
    expected.insert(at, (2, 25, b"harbour".to_vec()));
    for d in &mut expected {
        if (d.0, d.1) == (2, 120) {
            d.2 = b"New caption".to_vec();
        }
    }
    assert_eq!(all(&w.bytes), expected);
    // Only caption and keywords: the rest of the old stream is a byte-identical prefix and suffix.
    let first_kw = datasets(&before, &l()).unwrap().iter().find(|d| (d.record, d.number) == (2, 25)).unwrap().range.start;
    assert_eq!(w.bytes[..first_kw], before[..first_kw]);
    // §8 test 3: writing the same values again gives identical bytes.
    let again = write_stream(Some(&w.bytes), &IptcEdit { caption: Some(Some("New caption")), keywords: Some(&t) }, &l()).unwrap();
    assert_eq!(again.bytes, w.bytes);
}

/// Datasets the old stream lacks go in dataset order; clearing removes every copy; `None` leaves a
/// field alone.
#[test]
fn inserting_and_clearing() {
    let before = iim(&[(1, 0, &[0, 4]), (2, 0, &[0, 4]), (2, 80, b"J. Doe"), (2, 122, b"Desk")]);
    let t = tags(&["one"]);
    let w = write_stream(Some(&before), &IptcEdit { caption: Some(Some("Cap")), keywords: Some(&t) }, &l()).unwrap();
    let ids: Vec<(u8, u8)> = all(&w.bytes).iter().map(|d| (d.0, d.1)).collect();
    assert_eq!(ids, [(1, 0), (1, 90), (2, 0), (2, 25), (2, 80), (2, 120), (2, 122)]);

    let doubled = iim(&[(2, 120, b"one"), (2, 25, b"a"), (2, 120, b"two"), (2, 25, b"b")]);
    let cleared = write_stream(Some(&doubled), &IptcEdit { caption: Some(None), keywords: Some(&[]) }, &l()).unwrap();
    assert_eq!(all(&cleared.bytes), [(1, 90, UTF8_ESCAPE.to_vec())]);
    let blank = write_stream(Some(&doubled), &IptcEdit { caption: Some(Some(" \u{7} ")), keywords: None }, &l()).unwrap();
    assert_eq!(read(&blank.bytes, &l()).unwrap().caption, None, "a caption that cleans to nothing clears it");
    assert_eq!(read(&blank.bytes, &l()).unwrap().keywords, ["a", "b"], "keywords left alone");

    let caption_only = write_stream(Some(&agency()), &IptcEdit { caption: Some(Some("C")), keywords: None }, &l()).unwrap();
    assert_eq!(read(&caption_only.bytes, &l()).unwrap().keywords, ["sea", "boats", "dusk"]);
    // Duplicate tags (case-insensitive) are written once, the first spelling kept.
    let dup = tags(&["Sea", "sea", " SEA ", "sky"]);
    let w = write_stream(None, &IptcEdit { caption: None, keywords: Some(&dup) }, &l()).unwrap();
    assert_eq!(read(&w.bytes, &l()).unwrap().keywords, ["Sea", "sky"]);
}

/// The IPTC limits: a caption past 2,000 bytes and a keyword past 64 are cut at a character
/// boundary and reported; nothing else is lost.
#[test]
fn truncation_at_the_iptc_limits() {
    let caption = "é".repeat(1500); // 3,000 bytes
    let long_tag = format!("{}Ω", "k".repeat(63)); // 65 bytes; Ω would be cut in half
    let t = vec![long_tag.clone(), "short".to_owned()];
    let w = write_stream(None, &IptcEdit { caption: Some(Some(&caption)), keywords: Some(&t) }, &l()).unwrap();
    assert_eq!(w.truncated, Truncated { caption: true, keywords: 1 });
    assert!(w.truncated.any());
    let ds = all(&w.bytes);
    let cap = &ds.iter().find(|d| (d.0, d.1) == (2, 120)).unwrap().2;
    assert_eq!(cap.len(), MAX_CAPTION);
    assert!(std::str::from_utf8(cap).is_ok());
    let kws: Vec<&Vec<u8>> = ds.iter().filter(|d| (d.0, d.1) == (2, 25)).map(|d| &d.2).collect();
    assert_eq!(kws, [&"k".repeat(63).into_bytes(), &b"short".to_vec()]);
    assert!(kws.iter().all(|k| k.len() <= MAX_KEYWORD));
    // At the limit exactly, nothing is cut.
    let exact = "a".repeat(MAX_CAPTION);
    let w = write_stream(None, &IptcEdit { caption: Some(Some(&exact)), keywords: None }, &l()).unwrap();
    assert_eq!(w.truncated, Truncated::default());
    assert_eq!(read(&w.bytes, &l()).unwrap().caption.as_deref(), Some(exact.as_str()));
    // Two long tags that share their first 64 bytes are written once.
    let twins = vec![format!("{}one", "x".repeat(64)), format!("{}two", "x".repeat(64))];
    let w = write_stream(None, &IptcEdit { caption: None, keywords: Some(&twins) }, &l()).unwrap();
    assert_eq!(read(&w.bytes, &l()).unwrap().keywords, ["x".repeat(64)]);
    assert_eq!(w.truncated.keywords, 2);
}

/// §8 test 6, Latin-1 legacy: a stream without 1:90 whose text is Latin-1 reads correctly, and after
/// a write declares UTF-8 every other text dataset still reads the same (re-encoded); binary
/// datasets and text that was already UTF-8 keep their bytes.
#[test]
fn latin1_legacy_streams() {
    let before = iim(&[
        (1, 0, &[0, 4]),
        (2, 0, &[0, 4]),
        (2, 25, b"caf\xe9"),
        (2, 80, b"J\xfcrgen M\xfcller"),
        (2, 90, "Zürich".as_bytes()),
        (2, 120, b"\xc7a va, Fran\xe7oise ?"),
        (2, 202, &[0xff, 0xd8, 0xe9, 0xff, 0xd9]),
    ]);
    let old = read(&before, &l()).unwrap();
    assert_eq!(old.caption.as_deref(), Some("Ça va, Françoise ?"));
    assert_eq!(old.keywords, ["café"]);

    let t = tags(&["été"]);
    let w = write_stream(Some(&before), &IptcEdit { caption: None, keywords: Some(&t) }, &l()).unwrap();
    let ds = datasets(&w.bytes, &l()).unwrap();
    assert_eq!(Charset::of(&ds), Charset::Utf8);
    let text = |id: (u8, u8), b: &[u8]| {
        let ds = datasets(b, &l()).unwrap();
        let c = Charset::of(&ds);
        ds.iter().find(|d| (d.record, d.number) == id).map(|d| c.decode(d.data))
    };
    for id in [(2, 80), (2, 90), (2, 120)] {
        assert_eq!(text(id, &w.bytes), text(id, &before), "{id:?} reads the same");
    }
    let new = read(&w.bytes, &l()).unwrap();
    assert_eq!(new.caption, old.caption);
    assert_eq!(new.keywords, ["été"]);
    let get = |id: (u8, u8)| ds.iter().find(|d| (d.record, d.number) == id).unwrap().data.to_vec();
    assert_eq!(get((2, 202)), [0xff, 0xd8, 0xe9, 0xff, 0xd9], "binary data is never re-encoded");
    assert_eq!(get((2, 90)), "Zürich".as_bytes(), "UTF-8 text keeps its bytes");
    assert_eq!(get((2, 0)), [0, 4]);
}

/// The Photoshop resources around the stream: other resources byte for byte and in place, the
/// IPTC resource's name kept, the digest set to the MD5 of the new stream.
#[test]
fn resources_and_the_digest() {
    let thumb = vec![7u8; 33];
    let stale = [0u8; 16];
    // A named IPTC resource (odd name length: no pad byte) between others, and a stale digest.
    let mut block = irb(&[(0x03ed, &[0, 72, 0, 1, 0, 1, 0, 72, 0, 1, 0, 1]), (0x040c, &thumb)]);
    let stream = agency();
    block.extend(b"8BIM");
    block.extend(RESOURCE_IPTC.to_be_bytes());
    block.extend([3, b'I', b'I', b'M']);
    block.extend((stream.len() as u32).to_be_bytes());
    block.extend(&stream);
    if stream.len() % 2 == 1 {
        block.push(0);
    }
    block.extend(irb(&[(RESOURCE_IPTC_DIGEST, &stale), (0x0421, &[1, 2, 3])]));
    block.extend([0, 0, 0, 0]);

    let t = tags(&["new"]);
    let w = write_resources(Some(&block), &IptcEdit { caption: Some(Some("Fresh")), keywords: Some(&t) }, &l()).unwrap();
    let before = resources(&block, &l()).unwrap();
    let after = resources(&w.bytes, &l()).unwrap();
    assert_eq!(after.iter().map(|r| r.id).collect::<Vec<_>>(), before.iter().map(|r| r.id).collect::<Vec<_>>(), "same resources, same order");
    for (a, b) in after.iter().zip(&before) {
        if a.id != RESOURCE_IPTC && a.id != RESOURCE_IPTC_DIGEST {
            assert_eq!(w.bytes[a.range.clone()], block[b.range.clone()], "resource {:#06x} byte for byte", a.id);
        }
    }
    let iptc = after.iter().find(|r| r.id == RESOURCE_IPTC).unwrap();
    assert_eq!(iptc.name, b"IIM");
    let r = read(iptc.data, &l()).unwrap();
    assert_eq!((r.caption.as_deref(), r.keywords.as_slice()), (Some("Fresh"), t.as_slice()));
    let d = after.iter().find(|r| r.id == RESOURCE_IPTC_DIGEST).unwrap();
    assert_eq!(d.data, digest(iptc.data));
    assert_ne!(d.data, stale);
    assert!(w.bytes.ends_with(&[0, 0, 0, 0]), "trailing padding kept at the end");

    // No IPTC and no digest yet: both appended after the last resource.
    let plain = irb(&[(0x03ed, &[0; 16])]);
    let w = write_resources(Some(&plain), &IptcEdit { caption: Some(Some("Cap")), keywords: None }, &l()).unwrap();
    let after = resources(&w.bytes, &l()).unwrap();
    assert_eq!(after.iter().map(|r| r.id).collect::<Vec<_>>(), [0x03ed, RESOURCE_IPTC, RESOURCE_IPTC_DIGEST]);
    assert_eq!(w.bytes[..plain.len()], plain[..]);
    assert_eq!(after[2].data, digest(after[1].data));
    // No resources at all.
    let w = write_resources(None, &IptcEdit { caption: Some(Some("Cap")), keywords: None }, &l()).unwrap();
    assert_eq!(resources(&w.bytes, &l()).unwrap().iter().map(|r| r.id).collect::<Vec<_>>(), [RESOURCE_IPTC, RESOURCE_IPTC_DIGEST]);
    // §8 test 3.
    let again = write_resources(Some(&w.bytes), &IptcEdit { caption: Some(Some("Cap")), keywords: None }, &l()).unwrap();
    assert_eq!(again.bytes, w.bytes);
}

/// The written resources inside a JPEG's APP13 read back through the whole-file reader.
#[test]
fn reads_back_from_a_jpeg() {
    let t = tags(&["Sea", "東京"]);
    let w = write_resources(None, &IptcEdit { caption: Some(Some("Bay, at night")), keywords: Some(&t) }, &l()).unwrap();
    let file = jpeg(&[app13(&w.bytes)]);
    let m = dalica_meta::read(&file, &l()).unwrap();
    assert!(m.problems.is_empty());
    assert_eq!(m.owned.caption.as_deref(), Some("Bay, at night"));
    assert_eq!(m.owned.tags, t);
}

/// `IptcEdit::of` takes the caption and the tags of a write.
#[test]
fn edit_of_changes() {
    let c = Changes { caption: Some(Some("c".into())), tags: Some(tags(&["t"])), star: Some(true), ..Changes::default() };
    let e = IptcEdit::of(&c);
    assert_eq!(e.caption, Some(Some("c")));
    assert_eq!(e.keywords, Some(&tags(&["t"])[..]));
    assert!(IptcEdit::of(&Changes { star: Some(true), ..Changes::default() }).is_empty());
}

/// Limits: a write that would pass the dataset cap fails rather than produce a stream the reader
/// refuses; damaged input is `Err`.
#[test]
fn limits_and_damage() {
    let small = Limits { max_iptc_datasets: 4, ..Limits::default() };
    let t = tags(&["a", "b", "c", "d"]);
    assert_eq!(write_stream(None, &IptcEdit { caption: None, keywords: Some(&t) }, &small), Err(Error::TooMany("IPTC datasets")));
    let e = IptcEdit { caption: Some(Some("x")), keywords: None };
    assert!(write_stream(Some(b"\x1c\x02\x78\x00\x09ab"), &e, &l()).is_err());
    assert!(write_stream(Some(b"garbage"), &e, &l()).is_err());
    assert!(write_resources(Some(b"8BIM\x04\x04\x00\x00\x00\x00\x00\x09ab"), &e, &l()).is_err());
    assert!(write_resources(Some(b"XXXX"), &e, &l()).is_err());
    // A Latin-1 dataset that would pass the dataset size limit once re-encoded.
    let big = vec![0xe9u8; 40_000];
    let tight = Limits { max_dataset: 50_000, ..Limits::default() };
    assert_eq!(write_stream(Some(&iim(&[(2, 80, &big)])), &e, &tight), Err(Error::TooMany("IPTC dataset bytes")));
}

fn sample_resources() -> Vec<u8> {
    let stream = agency();
    irb(&[(0x03ed, &[0; 16]), (RESOURCE_IPTC, &stream), (RESOURCE_IPTC_DIGEST, &[0; 16])])
}

/// §8 test 10: resources and streams cut at every length are an `Err` or a clean write.
#[test]
fn truncation_never_panics() {
    let t = tags(&["x"]);
    let e = IptcEdit { caption: Some(Some("c")), keywords: Some(&t) };
    let block = sample_resources();
    for len in 0..block.len() {
        if let Ok(w) = write_resources(Some(&block[..len]), &e, &l()) {
            assert!(resources(&w.bytes, &l()).is_ok());
        }
    }
    let stream = agency();
    for len in 0..stream.len() {
        if let Ok(w) = write_stream(Some(&stream[..len]), &e, &l()) {
            assert_eq!(read(&w.bytes, &l()).unwrap().caption.as_deref(), Some("c"));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    /// §8 test 10: random bytes never panic.
    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let t = tags(&["x", "y"]);
        let e = IptcEdit { caption: Some(Some("c")), keywords: Some(&t) };
        let _ = write_stream(Some(&bytes), &e, &l());
        let _ = write_resources(Some(&bytes), &e, &l());
    }

    /// Random captions and tags read back exactly (when within the limits) from a written stream.
    #[test]
    fn values_round_trip(caption in "\\PC{0,80}", t in proptest::collection::vec("\\PC{0,16}", 0..8)) {
        let w = write_stream(Some(&agency()), &IptcEdit { caption: Some(Some(&caption)), keywords: Some(&t) }, &l()).unwrap();
        let r = read(&w.bytes, &l()).unwrap();
        let limits = l();
        prop_assert_eq!(r.caption, dalica_meta::fields::clean_text(&caption, limits.max_string));
        prop_assert_eq!(r.keywords, dalica_meta::fields::clean_tags(t.iter().map(String::as_str), limits.max_string, limits.max_tags));
    }

    /// Random single-byte changes to real resources never panic.
    #[test]
    fn mutations_never_panic(at in any::<prop::sample::Index>(), value in any::<u8>()) {
        let mut block = sample_resources();
        let i = at.index(block.len());
        block[i] = value;
        let t = tags(&["x"]);
        if let Ok(w) = write_resources(Some(&block), &IptcEdit { caption: Some(Some("c")), keywords: Some(&t) }, &l()) {
            prop_assert!(resources(&w.bytes, &l()).is_ok());
        }
    }
}
