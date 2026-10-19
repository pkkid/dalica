//! Collages (`creating.md` §3, M14): the document ([`doc`]), its page ([`page`]), the seeded
//! generator its layouts draw from ([`seed`]), the layouts ([`layout`]), the `.dalica-collage`
//! recipe ([`recipe`]) and the render ([`render`]). Pure: no I/O, no clock, no egui; the engine
//! owns the open collage, and the canvas, the render, the room's thumbnail and the MCP reply all
//! read the same [`layout::layout`].

pub mod doc;
pub mod layout;
pub mod page;
pub mod recipe;
pub mod render;
pub mod seed;

pub use doc::{Background, Border, Clip, Collage, DocError, MAX_CLIPS, Move, Settings, Style};
pub use layout::{Placement, layout};
pub use page::{Format, Orientation, Page, PhotoSizes, formats};
pub use recipe::{Recipe, RecipeError};
pub use seed::Rng;
