//! The one name sanitizer (`security.md` §5.4): every name Dalica makes up for a new file or
//! folder goes through `names::sanitize` first.

use dalica_core::Date;
use dalica_core::names::{self, FALLBACK, FORBIDDEN, MAX_NAME_BYTES, sanitize};
use proptest::prelude::*;

const FB: &str = "Export 2026-06-04";

#[test]
fn ordinary_names_pass_unchanged() {
    for name in ["Lisbon Weekend", "IMG_0042", "Lisboa à noite", "東京 2026", "beach.v2", "a-b_c (1)", "🌊"] {
        assert_eq!(sanitize(name, FB), name);
    }
}

#[test]
fn reserved_device_names_are_replaced() {
    assert_eq!(sanitize("CON", FB), "_CON");
    assert_eq!(sanitize("con", FB), "_con");
    assert_eq!(sanitize("nul.txt", FB), "_nul.txt");
    assert_eq!(sanitize("Aux .jpg", FB), "_Aux .jpg");
    assert_eq!(sanitize("COM1", FB), "_COM1");
    assert_eq!(sanitize("lpt9.tar.gz", FB), "_lpt9.tar.gz");
    assert_eq!(sanitize("CONOUT$", FB), "_CONOUT$");
    // Only the whole stem counts.
    assert_eq!(sanitize("CONCERT", FB), "CONCERT");
    assert_eq!(sanitize("COM10", FB), "COM10");
    assert!(names::is_reserved("prn.anything") && !names::is_reserved("_prn"));
}

#[test]
fn paths_never_survive() {
    assert_eq!(sanitize("../x", FB), "x");
    assert_eq!(sanitize("..\\..\\Windows\\win.ini", FB), "Windowswin.ini");
    assert_eq!(sanitize("/etc/passwd", FB), "etcpasswd");
    assert_eq!(sanitize("C:photo", FB), "Cphoto");
    assert_eq!(sanitize("name:stream", FB), "namestream");
    assert_eq!(sanitize("a*b?c\"d<e>f|g", FB), "abcdefg");
    assert_eq!(sanitize("..", FB), FB);
    assert_eq!(sanitize(".", FB), FB);
}

#[test]
fn hidden_option_like_and_trailing_parts_are_stripped() {
    assert_eq!(sanitize(".dalica.json", FB), "dalica.json");
    assert_eq!(sanitize(".hidden", FB), "hidden");
    assert_eq!(sanitize("-rf", FB), "rf");
    assert_eq!(sanitize("--. -name", FB), "name");
    assert_eq!(sanitize("  Lisbon  ", FB), "Lisbon");
    assert_eq!(sanitize("notes...", FB), "notes");
    assert_eq!(sanitize("trip. . .", FB), "trip");
    assert_eq!(sanitize("a-b-", FB), "a-b-", "a trailing dash is fine");
}

#[test]
fn control_and_bidirectional_characters_are_removed() {
    assert_eq!(sanitize("photo\u{202E}gpj.exe", FB), "photogpj.exe");
    assert_eq!(sanitize("\u{2067}שלום\u{2069} trip", FB), "שלום trip");
    assert_eq!(sanitize("a\u{200E}b\u{200F}c\u{061C}d", FB), "abcd");
    assert_eq!(sanitize("line\nbreak\ttab\u{0}nul\u{7f}del\u{85}", FB), "linebreaktabnuldel");
    assert!(names::is_bidi_control('\u{202A}') && !names::is_bidi_control('a'));
}

#[test]
fn long_names_are_cut_on_a_character_boundary() {
    let name = "é".repeat(150); // 300 bytes
    let s = sanitize(&name, FB);
    assert_eq!(s.len(), 254, "127 two-byte characters fit in 255 bytes");
    assert_eq!(s, "é".repeat(127));
    let ascii = "a".repeat(300);
    assert_eq!(sanitize(&ascii, FB).len(), MAX_NAME_BYTES);
    // A cut that ends on a dot or space strips it.
    let dotted = format!("{}. tail", "b".repeat(254));
    assert_eq!(sanitize(&dotted, FB), "b".repeat(254));
    // A reserved name long enough to overflow once prefixed still fits.
    let long_reserved = format!("CON.{}", "x".repeat(251));
    let s = sanitize(&long_reserved, FB);
    assert!(s.starts_with("_CON.") && s.len() == MAX_NAME_BYTES, "{s}");
}

#[test]
fn an_empty_name_takes_the_fallback() {
    assert_eq!(sanitize("", FB), FB);
    assert_eq!(sanitize(" . - ", FB), FB);
    assert_eq!(sanitize("///", "../Album"), "Album", "the fallback is sanitized too");
    assert_eq!(sanitize("", ""), FALLBACK);
    assert_eq!(sanitize("...", "\u{202E}"), FALLBACK);
    assert_eq!(sanitize("", "CON"), "_CON");
}

#[test]
fn dated_names_the_fallback() {
    let day = Date::new(2026, 6, 4).unwrap();
    assert_eq!(names::dated("Export", day), "Export 2026-06-04");
    assert_eq!(sanitize("", &names::dated("Export", day)), "Export 2026-06-04");
}

fn hostile() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        "[./\\\\:*?\"<>| -]{0,12}",
        "\\PC{0,40}",
        ".{0,40}",
        "(CON|con|nul|COM1|LPT9|aux)(\\.[a-z]{0,4})?[ .]{0,3}",
        "[\u{202A}-\u{202E}\u{2066}-\u{2069}\u{200E}a./]{0,20}",
        "[é東🌊a. .-]{100,300}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn sanitized_names_are_never_empty_never_paths_and_stable(name in hostile(), fallback in hostile()) {
        let s = sanitize(&name, &fallback);
        prop_assert!(!s.is_empty());
        prop_assert!(s.len() <= MAX_NAME_BYTES, "{} bytes", s.len());
        prop_assert!(!s.contains(FORBIDDEN), "{s:?}");
        prop_assert!(!s.chars().any(|c| c.is_control() || names::is_bidi_control(c)), "{s:?}");
        prop_assert!(!s.starts_with(['.', '-']) && !s.starts_with(char::is_whitespace), "{s:?}");
        prop_assert!(!s.ends_with('.') && !s.ends_with(char::is_whitespace), "{s:?}");
        prop_assert!(s != "." && s != "..");
        prop_assert!(!names::is_reserved(&s), "{s:?}");
        prop_assert_eq!(sanitize(&s, "unused"), s.clone(), "a second pass changes nothing");
    }
}
