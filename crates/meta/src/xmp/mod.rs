//! XMP: reading the owned properties of a packet and the merge writer. A stub until M6.9, which
//! fills it with our own bounded XML tokenizer (`roadmap.md` M6 decisions; `xmp-interop.md`
//! §1–§3): until then a packet reads as holding none of Dalica's fields and lists no rows, and the
//! containers still find, reassemble and bound the packets ([`crate::container`]).

use crate::fields::XmpOwned;
use crate::listing::Row;
use crate::{Limits, Result};

/// The owned properties of a standard packet and, for JPEG, its reassembled extended part.
pub fn read(packet: &[u8], extended: Option<&[u8]>, limits: &Limits) -> Result<XmpOwned> {
    let _ = (packet, extended, limits);
    Ok(XmpOwned::default())
}

/// Every property of a packet as listing rows (group "XMP").
pub fn rows(packet: &[u8], extended: Option<&[u8]>, limits: &Limits) -> Result<Vec<Row>> {
    let _ = (packet, extended, limits);
    Ok(Vec::new())
}
