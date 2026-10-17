//! dalica-create: Export (resize, quality, watermark, metadata copy), collage layout and render, slideshow sequencing.
//!
//! Layer L3; see `docs/architecture.md` §2. [`output`] is the one output pipeline Save, Save a
//! Copy, Save As and Export share (M8.10); [`watermark`] is the export watermark it calls;
//! [`save`] is Save in place, Undo Save and Revert (M8.12); [`slideshow`] is the slideshow's
//! sequencing and transition geometry (M11.4).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod export;
pub mod output;
pub mod save;
pub mod slideshow;
pub mod watermark;
