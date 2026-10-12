//! The application id, in one place (`roadmap.md` → *Reserved decisions* item 10).
//!
//! [`APP_ID`] is the reverse-DNS name the desktop knows Dalica by: the window's Wayland app id and
//! X11 class, and (from M6 on) the packages' ids, the Linux desktop file's name and the macOS
//! bundle id. It is the reverse of `dalica.org` plus the app's name, three elements so Flatpak
//! and Flathub accept it.
//!
//! It does not name the settings or data folders: those stay as `data-model.md` §4 says (moving
//! them would need a migration).
//!
//! The checks below say why a candidate id would be refused by one of the systems that use it;
//! the tests hold [`APP_ID`] to all of them.

use crate::error::{Error, excerpt};

/// Dalica's application id (maintainer's decision, 2026-10-11).
pub const APP_ID: &str = "org.dalica.Dalica";

/// The longest D-Bus name, in bytes (the D-Bus specification, "Bus names").
pub const MAX_DBUS_NAME: usize = 255;

/// The longest DNS label, in bytes (RFC 1035 §2.3.4).
pub const MAX_DNS_LABEL: usize = 63;

fn refuse(kind: &'static str, id: &str) -> Error {
    Error::InvalidId { kind, value: excerpt(id) }
}

/// `id` as a D-Bus well-known bus name: at most 255 bytes; two or more elements separated by `.`;
/// every element non-empty, of `[A-Za-z0-9_-]` only, and not starting with a digit.
pub fn check_dbus_name(id: &str) -> Result<(), Error> {
    if id.is_empty() || id.len() > MAX_DBUS_NAME {
        return Err(refuse("D-Bus name (empty or longer than 255 bytes)", id));
    }
    let mut elements = 0usize;
    for element in id.split('.') {
        elements = elements.saturating_add(1);
        let Some(first) = element.bytes().next() else {
            return Err(refuse("D-Bus name (an empty element)", id));
        };
        if first.is_ascii_digit() {
            return Err(refuse("D-Bus name (an element starts with a digit)", id));
        }
        if !element.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            return Err(refuse("D-Bus name (a character outside A-Z a-z 0-9 _ -)", id));
        }
    }
    if elements < 2 {
        return Err(refuse("D-Bus name (fewer than two elements)", id));
    }
    Ok(())
}

/// `id` as a macOS bundle identifier (`CFBundleIdentifier`): non-empty, of `[A-Za-z0-9.-]` only.
pub fn check_bundle_id(id: &str) -> Result<(), Error> {
    if id.is_empty() {
        return Err(refuse("macOS bundle id (empty)", id));
    }
    if !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') {
        return Err(refuse("macOS bundle id (a character outside A-Z a-z 0-9 . -)", id));
    }
    Ok(())
}

/// `id` as a reverse-DNS application id the way Flatpak takes it: three or more elements; every
/// element but the last is a lowercase DNS label (`[a-z0-9]`, at most 63 bytes; Flatpak allows `-`
/// only in the last element); the last is the app's name.
pub fn check_reverse_dns(id: &str) -> Result<(), Error> {
    let Some((domain, name)) = id.rsplit_once('.') else {
        return Err(refuse("reverse-DNS id (fewer than three elements)", id));
    };
    if !domain.contains('.') {
        return Err(refuse("reverse-DNS id (fewer than three elements)", id));
    }
    for label in domain.split('.') {
        if label.is_empty() || label.len() > MAX_DNS_LABEL {
            return Err(refuse("reverse-DNS id (a domain label empty or longer than 63 bytes)", id));
        }
        if !label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) {
            return Err(refuse("reverse-DNS id (a domain label outside a-z 0-9)", id));
        }
    }
    if name.is_empty() {
        return Err(refuse("reverse-DNS id (no app name)", id));
    }
    Ok(())
}

/// Every check above: `id` is a valid D-Bus name, macOS bundle id and reverse-DNS id.
pub fn check(id: &str) -> Result<(), Error> {
    check_dbus_name(id)?;
    check_bundle_id(id)?;
    check_reverse_dns(id)
}

#[cfg(test)]
mod tests {
    use super::{APP_ID, check};

    #[test]
    fn the_app_id_passes_every_check() {
        assert_eq!(check(APP_ID), Ok(()));
    }
}
