//! The IPTC of a rendered copy (`data-model.md` §6): kept as the source holds it, in the form the
//! target takes.
//!
//! IPTC says nothing about pixels, so its datasets are carried byte for byte. A source's Photoshop
//! image resources are carried with them, resource by resource, except those that describe the
//! source's own pixels or repeat a block the copy carries (or replaces) on its own: the thumbnails
//! (0x0409, 0x040C), the ICC profile (0x040F), the JPEG quality (0x0406), EXIF (0x0422, 0x0423) and
//! XMP (0x0424). A bare IIM stream (a TIFF's tag 33723, a PNG raw profile) is wrapped as resource
//! 0x0404 with its digest 0x0425 where the target needs resources (a JPEG's APP13); when a source
//! has both, the bare stream is the one read, so it replaces the IPTC resource.
//!
//! Provenance: written for Dalica from the Photoshop file format description of image resource
//! blocks and the IPTC-NAA Information Interchange Model 4.2. No other writer's code was consulted.

use crate::container::Scan;
use crate::iptc::read::resources;
use crate::iptc::write::digest;
use crate::iptc::{RESOURCE_IPTC, RESOURCE_IPTC_DIGEST};
use crate::{Limits, Result};

/// Image resources a copy leaves out: they describe the source's pixels or repeat another block.
const LEFT_OUT: [u16; 7] = [0x0406, 0x0409, 0x040c, 0x040f, 0x0422, 0x0423, 0x0424];

/// The IPTC a copy carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Iptc {
    /// Photoshop image resources (`8BIM` blocks), IPTC as resource 0x0404.
    Resources(Vec<u8>),
    /// A bare IIM stream.
    Stream(Vec<u8>),
}

impl Iptc {
    /// The block as Photoshop image resources (a bare stream wrapped with its digest).
    pub fn into_resources(self) -> Vec<u8> {
        match self {
            Iptc::Resources(r) => r,
            Iptc::Stream(s) => wrap(&s),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Iptc::Resources(b) | Iptc::Stream(b) => b,
        }
    }
}

/// The IPTC of `scan`'s file to carry; `None` when it holds none (or only resources a copy leaves
/// out). `Err` when its resources cannot be read.
pub fn carry(scan: &Scan<'_>, limits: &Limits) -> Result<Option<Iptc>> {
    let bare = scan.iptc.as_deref();
    let Some(ps) = scan.photoshop.as_deref() else {
        return Ok(bare.map(|s| Iptc::Stream(s.to_vec())));
    };
    let mut out = Vec::with_capacity(ps.len());
    for r in resources(ps, limits)? {
        let replaced = bare.is_some() && (r.id == RESOURCE_IPTC || r.id == RESOURCE_IPTC_DIGEST);
        if LEFT_OUT.contains(&r.id) || replaced {
            continue;
        }
        out.extend_from_slice(ps.get(r.range.clone()).unwrap_or_default());
        if out.len() % 2 == 1 {
            out.push(0);
        }
    }
    if let Some(stream) = bare {
        out.extend(wrap(stream));
    }
    Ok((!out.is_empty()).then_some(Iptc::Resources(out)))
}

/// A bare IIM stream as resources 0x0404 and its digest 0x0425.
fn wrap(stream: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(stream.len() + 48);
    resource(&mut out, RESOURCE_IPTC, stream);
    resource(&mut out, RESOURCE_IPTC_DIGEST, &digest(stream));
    out
}

/// One `8BIM` resource with an empty name, its data padded to an even size.
fn resource(out: &mut Vec<u8>, id: u16, data: &[u8]) {
    out.extend_from_slice(b"8BIM");
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_stream_is_wrapped_with_its_digest() {
        let stream = b"\x1c\x02\x78\x00\x03abc";
        let r = Iptc::Stream(stream.to_vec()).into_resources();
        let res = resources(&r, &Limits::default()).unwrap();
        assert_eq!(res.len(), 2);
        assert_eq!((res[0].id, res[0].data), (RESOURCE_IPTC, &stream[..]));
        assert_eq!((res[1].id, res[1].data), (RESOURCE_IPTC_DIGEST, &digest(stream)[..]));
    }
}
