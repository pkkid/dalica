//! The legacy sidecar reader (`data-model.md` §9, `security.md` §4.2). Every fixture is written
//! here from the documented format; the file and section names come from `legacy`'s constant.

use dalica_sidecar::legacy::{LegacyError, LegacyLimits, LegacyPerson, LegacySidecar, Text, folder_section, originals_dir_name, sidecar_names};
use proptest::prelude::*;

use crate::common::{cases, full};

fn parse(text: &str) -> LegacySidecar {
    LegacySidecar::parse(text.as_bytes(), &LegacyLimits::default()).unwrap()
}

fn t(s: &str) -> Text {
    Text { text: s.into(), lossy: false }
}

fn close(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

const ALBUM: &str = "f07f69266e7760526d5b1553c7f451fa";
const ALBUM2: &str = "0123456789abcdef0123456789abcdef";

/// A folder written as the legacy organizer writes one: CRLF, sections in any order.
fn sample() -> String {
    let lines = [
        "[photo01.jpg]".to_string(),
        "star=yes".into(),
        "hidden=yes".into(),
        "caption=At the lake; mother said \"a=b\"".into(),
        "keywords=beer,empanadas,keyword with spaces".into(),
        "rotate=rotate(3)".into(),
        "flipped=flipped(1)".into(),
        "crop=rect64(6800600097ff9fff)".into(),
        "filters=crop64=1,6800600097ff9fff;enhance=1;finetune2=1,0.000000,0.000000,0.190000,00000000,0.000000;".into(),
        "redo=sepia=1;".into(),
        "textactive=1".into(),
        "faces=rect64(4a8e8e6b),632e71e2ffd6c6d;rect64(6800600097ff9fff),ffffffffffffffff;".into(),
        format!("albums={ALBUM},{ALBUM2}"),
        "geotag=33.770556,-84.293055".into(),
        "backuphash=29250".into(),
        format!("[{}]", folder_section()),
        "name=2015-03-15 Garden".into(),
        "description=Planting the beds".into(),
        "date=42078.416667".into(),
        "location=Atlanta, GA".into(),
        "P2category=Hidden Folders".into(),
        "[encoding]".into(),
        "utf8=1".into(),
        "[Contacts2]".into(),
        "632e71e2ffd6c6d=Zoë Ann;;".into(),
        "ca5c88ca60f42c0b=user_lh,abc".into(),
        "0000000000000001=Two;Fields".into(),
        format!("[.album:{ALBUM}]"),
        "name=Synthetic Album A".into(),
        format!("token={ALBUM}"),
        "date=2015-03-15T09:59:59-07:00".into(),
        "description=Summer".into(),
        "location=Lake Lanier".into(),
        format!("[.album:{ALBUM2}]"),
        format!("token={ALBUM2}"),
        "[.album:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa]".into(),
        "name=By day count".into(),
        "date=42078.5".into(),
    ];
    lines.join("\r\n") + "\r\n"
}

#[test]
fn the_names_come_from_the_constant() {
    let [current, older] = sidecar_names();
    let stem = current.strip_prefix('.').and_then(|s| s.strip_suffix(".ini")).unwrap();
    assert!(stem.len() == 6 && stem.bytes().all(|b| b.is_ascii_lowercase()));
    assert_eq!(older, format!("{}.ini", folder_section()));
    assert_eq!(folder_section().to_lowercase(), stem);
    assert_eq!(originals_dir_name(), format!(".{stem}originals"));
}

#[test]
fn every_key_reads_back_from_a_sample_folder() {
    let s = parse(&sample());
    assert!(s.is_utf8());

    let folder = s.folder();
    assert_eq!(folder.description, Some(t("Planting the beds")));
    assert_eq!(folder.date.unwrap().to_string(), "2015-03-15T10:00:00");
    assert_eq!(folder.place, Some(t("Atlanta, GA")));
    assert!(folder.hidden);

    assert_eq!(s.file_names(), ["photo01.jpg"]);
    let f = s.file("photo01.jpg").unwrap();
    assert!(f.star && f.hidden);
    assert_eq!(f.caption, Some(t("At the lake; mother said \"a=b\"")));
    assert_eq!(f.keywords, [t("beer"), t("empanadas"), t("keyword with spaces")]);
    assert_eq!((f.rotation, f.flip), (Some(3), Some(1)));
    // (650, 450)–(950, 750) on a 1600 × 1200 picture.
    let want = [650.0 / 1600.0, 450.0 / 1200.0, 950.0 / 1600.0, 750.0 / 1200.0];
    assert!(f.crop.unwrap().iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-4), "{:?}", f.crop);
    assert_eq!(f.edits, ["crop64=1,6800600097ff9fff", "enhance=1", "finetune2=1,0.000000,0.000000,0.190000,00000000,0.000000"]);
    assert_eq!(f.redo, ["sepia=1"]);
    assert_eq!(f.text_active, Some(true));
    assert_eq!(f.faces.len(), 2);
    assert_eq!(f.bad_faces, 0);
    assert_eq!(f.faces[0].person, LegacyPerson::Contact("0632e71e2ffd6c6d".into()));
    assert!(close(f.faces[0].rect, [0.0, 0.0, f64::from(0x4a8e) / 65535.0, f64::from(0x8e6b) / 65535.0]));
    assert_eq!(f.faces[1].person, LegacyPerson::Ignored);
    assert_eq!(f.albums, [ALBUM, ALBUM2]);
    let geo = f.geotag.unwrap();
    assert_eq!((geo.lat(), geo.lon()), (33.770556, -84.293055));
    // Lookups ignore case, as the files' names on the old program's system did.
    assert_eq!(s.file("PHOTO01.JPG"), Some(f));
    assert_eq!(s.file("missing.jpg"), None);

    // The face's contact is named by the three-field form only, the id padded to 16 digits.
    let contacts = s.contacts();
    assert_eq!(contacts.len(), 1, "{contacts:?}");
    assert_eq!(contacts.get("0632e71e2ffd6c6d"), Some(&t("Zoë Ann")));

    // Album definitions; the tombstone (no name) is skipped; both date forms read.
    let albums = s.albums();
    assert_eq!(albums.len(), 2, "{albums:?}");
    assert_eq!(albums[0].token, ALBUM);
    assert_eq!(albums[0].name, t("Synthetic Album A"));
    assert_eq!(albums[0].date.unwrap().to_string(), "2015-03-15T09:59:59");
    assert_eq!(albums[0].description, Some(t("Summer")));
    assert_eq!(albums[0].place, Some(t("Lake Lanier")));
    assert_eq!(albums[1].token, "a".repeat(32));
    assert_eq!(albums[1].date.unwrap().to_string(), "2015-03-15T12:00:00");
    // Keys Dalica has no use for stay readable as raw values.
    assert_eq!(s.section("photo01.jpg").unwrap().get("backuphash"), Some(t("29250")));
}

#[test]
fn the_originals_folder_sidecar_reads_with_the_same_reader() {
    let text = "[photo03.jpg]\r\nfilters=crop64=1,dc3369dc570a51e;\r\ncrop=rect64(dc3369dc570a51e)\r\nmoddate=5ec020afbcf9dc01\r\nwidth=800\r\nheight=600\r\ntextactive=0\r\n";
    let s = parse(text);
    let f = s.file("photo03.jpg").unwrap();
    assert_eq!(f.edits, ["crop64=1,dc3369dc570a51e"]);
    assert!(f.crop.is_some());
    assert_eq!(f.text_active, Some(false));
    assert_eq!(s.section("photo03.jpg").unwrap().get("width"), Some(t("800")));
}

#[test]
fn a_file_of_the_older_encoding_reads_its_accented_names() {
    // No encoding section: Windows-1252. 0xE9 is é, 0x80 the euro sign, 0x81 has no character.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"[Ren\xE9e.jpg]\r\ncaption=Caf\xE9 \x80 5\r\nkeywords=Andr\xE9,x\x81y\r\n");
    bytes.extend_from_slice(b"[Contacts2]\r\n1a=Jos\xE9 M\xFCller;;\r\n");
    bytes.extend_from_slice(format!("[.album:{ALBUM}]\r\n").as_bytes());
    bytes.extend_from_slice(b"name=No\xEBl\r\n");
    let s = LegacySidecar::parse(&bytes, &LegacyLimits::default()).unwrap();
    assert!(!s.is_utf8());
    assert_eq!(s.file_names(), ["Renée.jpg"]);
    let f = s.file("Renée.jpg").unwrap();
    assert_eq!(f.caption, Some(t("Café € 5")));
    assert_eq!(f.keywords[0], t("André"));
    // A byte that does not decode is shown as U+FFFD and marked, never written back.
    assert_eq!(f.keywords[1], Text { text: "x\u{FFFD}y".into(), lossy: true });
    assert_eq!(s.contacts().get("000000000000001a"), Some(&t("José Müller")));
    assert_eq!(s.albums()[0].name, t("Noël"));

    // The same bytes as UTF-8 that does not decode: marked lossy.
    let s = LegacySidecar::parse(b"[encoding]\nutf8=1\n[a.jpg]\ncaption=Caf\xE9\n", &LegacyLimits::default()).unwrap();
    assert_eq!(s.file("a.jpg").unwrap().caption, Some(Text { text: "Caf\u{FFFD}".into(), lossy: true }));
}

#[test]
fn values_holding_separators_and_the_text_overlay_survive() {
    // The text holds `;`, `=` and a line break; its length prefixes keep it whole.
    let text_value = "1;137;16;one; two=3\r\nfour;Times;0.342500,0.685625,0.033333,0.000000;v1,4294967295,4278190080,128.000000,1.000000,0.000000,1.000000,400,0,49152;;";
    let block = text_value.split_once("137;").unwrap().1.strip_suffix(';').unwrap();
    assert_eq!(block.len(), 137);
    let file = format!("[a.jpg]\nfilters=bw=1;\ntext={text_value}\ncaption=x=y;z\nstar=yes\n");
    let s = parse(&file);
    let f = s.file("a.jpg").unwrap();
    let overlay = f.text.unwrap();
    assert_eq!(overlay.raw, t(text_value));
    assert_eq!(overlay.texts, [t("one; two=3\r\nfour")]);
    // The lines after the overlay are still read.
    assert_eq!(f.caption, Some(t("x=y;z")));
    assert!(f.star);
    assert_eq!(f.edits, ["bw=1"]);
}

#[test]
fn a_short_rectangle_is_padded_and_a_full_one_reads_whole() {
    let s = parse("[a.jpg]\ncrop=rect64(ffffffff)\nfaces=rect64(1),1;rect64(x),1;rect64(11112222333344445),2;rect64(ffff00000000ffff),3\n");
    let f = s.file("a.jpg").unwrap();
    assert_eq!(f.crop, Some([0.0, 0.0, 1.0, 1.0]));
    assert_eq!(f.faces.len(), 1);
    assert!(close(f.faces[0].rect, [0.0, 0.0, 0.0, 1.0 / 65535.0]));
    assert_eq!(f.faces[0].person, LegacyPerson::Contact("0000000000000001".into()));
    // Not hexadecimal, over 16 digits, corners out of order.
    assert_eq!(f.bad_faces, 3);
}

#[test]
fn duplicated_reversed_and_empty_sections_are_skipped_with_the_rest_kept() {
    let text = [
        "orphan=before any section",
        "[a.jpg]",
        "star=yes",
        "caption=first",
        "05292=hsahpukcab",
        "]gpj.b[",
        "[]",
        "caption=under an empty section",
        "[(null)]",
        format!("albums={ALBUM}").as_str(),
        "[b.jpg",
        "[a.jpg]",
        "caption=second",
        "[../escape.jpg]",
        "star=yes",
        "[b.jpg]",
        "star=yes",
    ]
    .join("\n");
    let s = parse(&text);
    assert_eq!(s.file_names(), ["a.jpg", "b.jpg"]);
    let a = s.file("a.jpg").unwrap();
    // The duplicate merges into the first, the later value winning; nothing else is lost.
    assert!(a.star);
    assert_eq!(a.caption, Some(t("second")));
    assert!(a.albums.is_empty());
    assert!(s.file("b.jpg").unwrap().star);
    assert!(s.file("(null)").is_none() && s.file("../escape.jpg").is_none());
}

#[test]
fn a_file_past_each_limit_is_refused_by_name() {
    let small = LegacyLimits { max_bytes: 64, max_line: 16, max_sections: 2, max_keys: 2 };
    let refuse = |text: &str, limits: &LegacyLimits| LegacySidecar::parse(text.as_bytes(), limits).unwrap_err();
    assert!(matches!(refuse(&"x".repeat(65), &small), LegacyError::FileTooLarge { size: 65, limit: 64 }));
    assert!(matches!(refuse("[a]\nk=0123456789abcdef\n", &small), LegacyError::LineTooLong { line: 2, limit: 16 }));
    assert!(matches!(refuse("[a]\n[b]\n[a]\n[c]\n", &small), LegacyError::TooManySections { limit: 2 }));
    assert!(matches!(refuse("[a]\nx=1\ny=2\nx=3\nz=4\n", &small), LegacyError::TooManyKeys { ref section, limit: 2 } if section == "a"));
    // At the limits themselves, each file reads.
    assert!(LegacySidecar::parse(b"[a]\nk=0123456789abcd\n[b]\nx=1\ny=2\n", &small).is_ok());

    // The default limits, by name.
    let d = LegacyLimits::default();
    assert_eq!((d.max_bytes, d.max_line, d.max_sections, d.max_keys), (64 << 20, 1 << 20, 1_000_000, 1000));
    assert!(matches!(LegacySidecar::parse(&vec![b'\n'; (64 << 20) + 1], &d), Err(LegacyError::FileTooLarge { .. })));
    assert!(matches!(refuse(&format!("[a]\nk={}\n", "v".repeat(1 << 20)), &d), LegacyError::LineTooLong { line: 2, .. }));
    let keys: String = (0..=1000).map(|i| format!("k{i}=v\n")).collect();
    assert!(matches!(refuse(&format!("[a]\n{keys}"), &d), LegacyError::TooManyKeys { .. }));
    if full() {
        let sections: String = (0..=1_000_000).map(|i| format!("[s{i}]\n")).collect();
        assert!(matches!(refuse(&sections, &d), LegacyError::TooManySections { .. }));
    }
}

/// Every typed read of a parsed file, as the scan will make them.
fn read_everything(s: &LegacySidecar) {
    let _ = s.folder();
    let _ = s.albums();
    let _ = s.contacts();
    for name in s.file_names() {
        assert!(s.file(name).is_some(), "{name:?}");
    }
}

/// Lines built from the format's own pieces, so the typed readers see near-valid values.
fn line_soup() -> impl Strategy<Value = Vec<u8>> {
    let piece = prop_oneof![
        Just(b"[a.jpg]".to_vec()),
        Just(format!("[{}]", folder_section()).into_bytes()),
        Just(b"[encoding]".to_vec()),
        Just(b"utf8=1".to_vec()),
        Just(b"[Contacts2]".to_vec()),
        Just(format!("[.album:{ALBUM}]").into_bytes()),
        Just(b"[".to_vec()),
        Just(b"[]".to_vec()),
        Just(b"text=1;5;".to_vec()),
        Just(b"text=2;3;1;a;".to_vec()),
        Just(b"faces=rect64(".to_vec()),
        Just(b"crop=rect64(ffffffffffffffffff)".to_vec()),
        Just(b"date=1e308".to_vec()),
        Just(b"date=-2999999.99".to_vec()),
        Just(b"date=2015-03-15T09:59:59.5+07:00".to_vec()),
        Just(b"rotate=rotate(99999)".to_vec()),
        Just(b"geotag=NaN,inf".to_vec()),
        Just(b"1a=n;e;g".to_vec()),
        Just(b"\r".to_vec()),
        Just(b"\n".to_vec()),
        Just(b"=".to_vec()),
        Just(b";".to_vec()),
        Just(b",".to_vec()),
        Just(vec![0xC3]),
        Just(vec![0x81]),
        "[a-z0-9=;,()]{0,12}".prop_map(String::into_bytes),
        prop::collection::vec(any::<u8>(), 0..8),
    ];
    prop::collection::vec(piece, 0..64).prop_map(|ps| ps.concat())
}

proptest! {
    #![proptest_config(cases(256, 32))]

    #[test]
    fn random_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        if let Ok(s) = LegacySidecar::parse(&bytes, &LegacyLimits::default()) {
            read_everything(&s);
        }
    }

    #[test]
    fn random_line_soups_never_panic(bytes in line_soup()) {
        let limits = LegacyLimits { max_bytes: 4096, max_line: 64, max_sections: 8, max_keys: 8 };
        for limits in [LegacyLimits::default(), limits] {
            if let Ok(s) = LegacySidecar::parse(&bytes, &limits) {
                read_everything(&s);
            }
        }
    }
}
