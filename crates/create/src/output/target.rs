//! Which format a new file takes (`formats.md` §1, `data-model.md` §6, `creating.md` §2).

use dalica_image::Format;

/// The formats Dalica writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutFormat {
    Jpeg,
    Png,
}

impl OutFormat {
    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            OutFormat::Jpeg => "jpg",
            OutFormat::Png => "png",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            OutFormat::Jpeg => "JPEG",
            OutFormat::Png => "PNG",
        }
    }
}

/// What the new file is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Purpose {
    /// Save: the edited pixels replace the photo under its own name.
    InPlace,
    /// Save of a format Dalica cannot write in place: a JPEG beside the source.
    Beside,
    /// Save a Copy: `<stem>-001.jpg` beside the photo.
    Copy,
    /// Export: JPEG, or PNG for a source with transparency.
    Export,
}

/// The format a file of `kind` becomes for `purpose`; `alpha` says whether the source has
/// transparency (an alpha sample below fully opaque). `None` when the photo cannot be written in
/// place (every format but JPEG, and PNG without transparency): its Save goes beside it.
pub fn target(kind: Format, alpha: bool, purpose: Purpose) -> Option<OutFormat> {
    match purpose {
        Purpose::InPlace => match kind {
            Format::Jpeg => Some(OutFormat::Jpeg),
            Format::Png if !alpha => Some(OutFormat::Png),
            _ => None,
        },
        Purpose::Beside | Purpose::Copy => Some(OutFormat::Jpeg),
        Purpose::Export if alpha => Some(OutFormat::Png),
        Purpose::Export => Some(OutFormat::Jpeg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_image::RawKind;

    #[test]
    fn in_place_only_for_jpeg_and_opaque_png() {
        assert_eq!(target(Format::Jpeg, false, Purpose::InPlace), Some(OutFormat::Jpeg));
        assert_eq!(target(Format::Png, false, Purpose::InPlace), Some(OutFormat::Png));
        assert_eq!(target(Format::Png, true, Purpose::InPlace), None);
        for kind in [Format::Tiff, Format::WebP, Format::Gif, Format::Bmp, Format::Raw(RawKind::Cr3)] {
            assert_eq!(target(kind, false, Purpose::InPlace), None, "{kind:?}");
            assert_eq!(target(kind, false, Purpose::Beside), Some(OutFormat::Jpeg), "{kind:?}");
        }
    }

    #[test]
    fn copies_are_jpeg_and_exports_keep_transparency() {
        assert_eq!(target(Format::Png, true, Purpose::Copy), Some(OutFormat::Jpeg));
        assert_eq!(target(Format::Png, true, Purpose::Export), Some(OutFormat::Png));
        assert_eq!(target(Format::WebP, true, Purpose::Export), Some(OutFormat::Png));
        assert_eq!(target(Format::Png, false, Purpose::Export), Some(OutFormat::Jpeg));
        assert_eq!(OutFormat::Png.extension(), "png");
    }
}
