//! The search facts a scan reads with a photo's metadata (`organizing.md` §8.1, §8.2): its camera
//! make and model, whether it is a video and whether it holds a GPS location
//! ([`dalica_library::Facts`]).
//!
//! - **Camera:** the EXIF `Make` and `Model` of IFD0 (the file's own block: a JPEG's APP1, a PNG's
//!   `eXIf`, a TIFF's or a TIFF-structured RAW's IFD0), as `dalica-meta` reads them for the
//!   Properties pane; for a RAW container `dalica-meta` does not read (CR3, RAF, CRW, MRW …), the
//!   header's EXIF ([`of_header`]). Each is kept as read (no case change, the make never taken out
//!   of the model) once made safe: control characters dropped, line breaks and tabs turned into
//!   spaces, trimmed, cut at [`MAX_CAMERA`] bytes on a character boundary ([`camera`]).
//! - **Video:** the file's type ([`FileType::Video`]). Videos are listed from M15.11
//!   (`formats.md` §1, [`crate::video`]), their container's facts beside these.
//! - **Geotagged:** the file holds a usable GPS location: the EXIF GPS IFD's latitude and
//!   longitude, else XMP `exif:GPSLatitude` and `exif:GPSLongitude` (a RAW's `.xmp` sidecar
//!   included), the place `dalica-meta` resolves (`xmp-interop.md` §2). A GPS block without a
//!   valid latitude and longitude is no location.
//!
//! Everything comes from what the scan's metadata pass ([`crate::meta`]) already read, bounded by
//! its limits; a block that cannot be read gives no facts, never an error.

use dalica_image::{Format, Header};
use dalica_library::{Facts, MAX_CAMERA};
use dalica_meta::Metadata;

use crate::skip::FileType;

/// The facts of a photo of `format` whose metadata reads as `metadata`.
pub fn of(format: Format, metadata: &Metadata) -> Facts {
    Facts {
        make: camera(metadata.summary.make.as_deref()),
        model: camera(metadata.summary.model.as_deref()),
        video: is_video(format),
        geotagged: metadata.owned.place.is_some(),
    }
}

/// The facts a header's EXIF gives (camera make and model only): for the RAW containers whose
/// metadata `dalica-meta` does not read.
pub fn of_header(header: &Header) -> Facts {
    let exif = header.exif.as_ref();
    Facts {
        make: camera(exif.and_then(|e| e.make.as_deref())),
        model: camera(exif.and_then(|e| e.model.as_deref())),
        video: is_video(header.format),
        geotagged: false,
    }
}

/// Whether a file of `format` is a video (its type).
pub fn is_video(format: Format) -> bool {
    FileType::of(format) == FileType::Video
}

/// A camera make or model as the facts keep it: control characters dropped, line breaks and tabs
/// as spaces, trimmed, at most [`MAX_CAMERA`] bytes (cut on a character boundary); empty for none.
pub fn camera(text: Option<&str>) -> String {
    text.and_then(|t| dalica_meta::fields::clean_tag(t, MAX_CAMERA)).unwrap_or_default()
}

/// `facts` with the camera of `header` (facts from the file's header) when it has none of its own.
pub(crate) fn with_camera_of(mut facts: Facts, header: &Facts) -> Facts {
    if facts.make.is_empty() && facts.model.is_empty() {
        facts.make.clone_from(&header.make);
        facts.model.clone_from(&header.model);
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_camera_is_kept_as_read_once_safe() {
        assert_eq!(camera(None), "");
        assert_eq!(camera(Some("  NIKON CORPORATION ")), "NIKON CORPORATION");
        assert_eq!(camera(Some("Canon EOS R5")), "Canon EOS R5", "case kept, the make not taken out");
        assert_eq!(camera(Some("a\u{7}b\nc\td")), "ab c d");
        assert_eq!(camera(Some(" \u{0} \u{1b} ")), "");
        let long = "é".repeat(MAX_CAMERA);
        let cut = camera(Some(&long));
        assert!(cut.len() <= MAX_CAMERA && cut.len() >= MAX_CAMERA - 1 && cut.chars().all(|c| c == 'é'));
    }

    #[test]
    fn a_header_camera_fills_only_a_missing_one() {
        let header = Facts { make: "Canon".into(), model: "EOS R5".into(), ..Facts::default() };
        let read = Facts { geotagged: true, ..Facts::default() };
        assert_eq!(with_camera_of(read, &header), Facts { make: "Canon".into(), model: "EOS R5".into(), video: false, geotagged: true });
        let own = Facts { model: "X100V".into(), ..Facts::default() };
        assert_eq!(with_camera_of(own.clone(), &header), own);
    }

    #[test]
    fn every_video_kind_is_a_video_and_no_image_format_is() {
        for f in [Format::Jpeg, Format::Png, Format::Tiff, Format::WebP, Format::Heif, Format::Raw(dalica_image::RawKind::Cr3)] {
            assert!(!is_video(f));
        }
        for kind in dalica_image::sniff::VideoKind::ALL {
            assert!(is_video(Format::Video(kind)), "{kind:?}, not RAW");
        }
    }
}
