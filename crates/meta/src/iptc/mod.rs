//! IPTC-IIM: the datasets Dalica reads (record 1 and 2 of IPTC-IIM 4.2) and the Photoshop image
//! resources that carry them in JPEG APP13. Reading is in [`read`]; writing arrives with M6.10.
//!
//! Provenance: dataset numbers and the 1:90 character-set escape from the public IPTC-IIM 4.2
//! specification; resource ids from the public description of Photoshop image resource blocks. No
//! other reader's code was consulted.

pub mod read;
pub mod write;

/// The Photoshop image resource holding IPTC-IIM.
pub const RESOURCE_IPTC: u16 = 0x0404;
/// The Photoshop image resource holding the MD5 digest of the IPTC resource.
pub const RESOURCE_IPTC_DIGEST: u16 = 0x0425;

/// 1:90 CodedCharacterSet.
pub const CODED_CHARACTER_SET: (u8, u8) = (1, 90);
/// The 1:90 value that declares UTF-8 (ISO 2022 escape `ESC % G`).
pub const UTF8_ESCAPE: &[u8] = b"\x1b%G";
/// 2:25 Keywords (repeatable).
pub const KEYWORDS: (u8, u8) = (2, 25);
/// 2:55 DateCreated, `CCYYMMDD`.
pub const DATE_CREATED: (u8, u8) = (2, 55);
/// 2:60 TimeCreated, `HHMMSS±HHMM`.
pub const TIME_CREATED: (u8, u8) = (2, 60);
/// 2:120 Caption-Abstract.
pub const CAPTION: (u8, u8) = (2, 120);

/// The name of a dataset, for the listing.
pub fn dataset_name(record: u8, number: u8) -> Option<&'static str> {
    Some(match (record, number) {
        (1, 0) => "Envelope record version",
        (1, 90) => "Coded character set",
        (2, 0) => "Record version",
        (2, 3) => "Object type",
        (2, 4) => "Object attribute",
        (2, 5) => "Object name",
        (2, 7) => "Edit status",
        (2, 10) => "Urgency",
        (2, 12) => "Subject reference",
        (2, 15) => "Category",
        (2, 20) => "Supplemental category",
        (2, 22) => "Fixture identifier",
        (2, 25) => "Keywords",
        (2, 26) => "Content location code",
        (2, 27) => "Content location name",
        (2, 30) => "Release date",
        (2, 35) => "Release time",
        (2, 37) => "Expiration date",
        (2, 38) => "Expiration time",
        (2, 40) => "Special instructions",
        (2, 55) => "Date created",
        (2, 60) => "Time created",
        (2, 62) => "Digital creation date",
        (2, 63) => "Digital creation time",
        (2, 65) => "Originating program",
        (2, 70) => "Program version",
        (2, 80) => "By-line",
        (2, 85) => "By-line title",
        (2, 90) => "City",
        (2, 92) => "Sublocation",
        (2, 95) => "Province/State",
        (2, 100) => "Country code",
        (2, 101) => "Country",
        (2, 103) => "Original transmission reference",
        (2, 105) => "Headline",
        (2, 110) => "Credit",
        (2, 115) => "Source",
        (2, 116) => "Copyright notice",
        (2, 118) => "Contact",
        (2, 120) => "Caption",
        (2, 122) => "Caption writer",
        _ => return None,
    })
}
