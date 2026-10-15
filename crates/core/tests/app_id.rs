//! The application id's syntax rules (`roadmap.md` M6.1): [`APP_ID`] is a valid D-Bus well-known
//! name, macOS bundle id and reverse-DNS (Flatpak) id; each check refuses what its system would,
//! and none panics on any input.

use dalica_core::Error;
use dalica_core::app_id::{APP_ID, MAX_DBUS_NAME, check, check_bundle_id, check_dbus_name, check_reverse_dns};
use proptest::prelude::*;

#[test]
fn the_app_id_passes_every_check() {
    assert_eq!(check_dbus_name(APP_ID), Ok(()));
    assert_eq!(check_bundle_id(APP_ID), Ok(()));
    assert_eq!(check_reverse_dns(APP_ID), Ok(()));
    assert_eq!(check(APP_ID), Ok(()));
}

/// Two or more non-empty elements, none starting with a digit, only ASCII letters, digits, `_` and
/// `-`, at most 255 bytes.
#[test]
fn dbus_names_follow_the_bus_rules() {
    for ok in ["a.b", "org.dalica.Dalica", "A_b.c-d.E9", "_x._y", "org.d9.D9"] {
        assert_eq!(check_dbus_name(ok), Ok(()), "{ok}");
    }
    let bad_elements = ["", "Dalica", ".org.dalica", "org.dalica.", "org..dalica", "."];
    let digit_first = ["org.9dalica.Dalica", "1org.dalica", "org.dalica.2"];
    let bad_chars = ["org.dalica.Da lica", "org.dalica.Dalicá", "org.dalica.Da+lica", ":1.42", "org/dalica.Dalica", "org.dalica.Dalica\0"];
    for bad in bad_elements.into_iter().chain(digit_first).chain(bad_chars) {
        assert!(check_dbus_name(bad).is_err(), "{bad:?} accepted");
    }
    let at_limit = format!("a.{}", "b".repeat(MAX_DBUS_NAME - 2));
    assert_eq!(at_limit.len(), MAX_DBUS_NAME);
    assert_eq!(check_dbus_name(&at_limit), Ok(()));
    let over = format!("{at_limit}b");
    assert!(check_dbus_name(&over).is_err());
}

#[test]
fn bundle_ids_take_only_ascii_letters_digits_dots_and_hyphens() {
    for ok in ["org.dalica.Dalica", "com.example.my-app", "Dalica"] {
        assert_eq!(check_bundle_id(ok), Ok(()), "{ok}");
    }
    // An underscore is a valid D-Bus character but not a bundle id one.
    for bad in ["", "org.dalica.Da_lica", "org.dalica.Da lica", "org.dalica.Dalicá"] {
        assert!(check_bundle_id(bad).is_err(), "{bad:?} accepted");
    }
}

#[test]
fn reverse_dns_ids_have_a_lowercase_domain_and_an_app_name() {
    for ok in ["org.dalica.Dalica", "io.github.pkkid.Dalica", "org.dalica.dalica-viewer"] {
        assert_eq!(check_reverse_dns(ok), Ok(()), "{ok}");
    }
    for bad in [
        "",
        "Dalica",
        // Two elements: D-Bus takes them, Flatpak does not.
        "dalica.Dalica",
        "org.Dalica.Dalica",
        "org.da-lica.Dalica",
        "org..Dalica",
        "org.dalica.",
    ] {
        assert!(check_reverse_dns(bad).is_err(), "{bad:?} accepted");
    }
    let long_label = format!("org.{}.Dalica", "a".repeat(64));
    assert!(check_reverse_dns(&long_label).is_err());
}

#[test]
fn a_refusal_names_the_rule_and_the_value() {
    let Err(Error::InvalidId { kind, value }) = check("org.9dalica.Dalica") else { panic!("expected a refusal") };
    assert!(kind.contains("digit"), "{kind}");
    assert_eq!(value, "org.9dalica.Dalica");
    // Hostile input is cut to a readable length in the message.
    let Err(Error::InvalidId { value, .. }) = check(&"x".repeat(10_000)) else { panic!("expected a refusal") };
    assert!(value.len() < 100);
}

proptest! {
    #![proptest_config(crate::common::cases(2048, 32))]

    #[test]
    fn no_check_panics_and_every_pass_is_a_valid_id(id in prop_oneof!["\\PC{0,40}", "[a-zA-Z0-9_.-]{0,40}", "[a-z]{1,8}(\\.[a-zA-Z0-9_-]{0,8}){0,4}"]) {
        let _ = check_dbus_name(&id);
        let _ = check_bundle_id(&id);
        let _ = check_reverse_dns(&id);
        if check(&id).is_ok() {
            prop_assert!(id.len() <= MAX_DBUS_NAME);
            prop_assert!(id.split('.').count() >= 3);
            prop_assert!(id.split('.').all(|e| e.chars().next().is_some_and(|c| !c.is_ascii_digit())));
            prop_assert!(id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'));
        }
    }
}
