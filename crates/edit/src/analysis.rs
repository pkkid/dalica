//! Picture analysis shared by the operations that look at the picture before changing it (Auto
//! Fix, Auto Contrast, Auto Color and the Tuning wands, `editing.md` §6.3 and §7).
//!
//! They analyse a **fixed-size proxy**: the picture at the operation's place in the pipeline (after
//! the crop, the straighten and the operations applied before it, before the recipe's rotations
//! and flips), rendered so that its longer side is [`PROXY_EDGE`] pixels (smaller pictures as they
//! are). The renderer makes it ([`crate::ops::Prepare::proxy`]) the same way for a 128-pixel
//! thumbnail, the edit view and a full-size Save, so all three reach the same result.
//!
//! The analyses themselves arrive with M7.13.

/// The longer side of the analysis proxy, in pixels.
pub const PROXY_EDGE: u32 = 256;
