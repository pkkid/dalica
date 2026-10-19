//! The Frame Mosaic layout (`creating.md` §3). A stand-in until M14.9: the simple grid of
//! [`super::simple_grid`].

use super::{Ctx, Slot};

/// One slot per picture of `ctx`, in its order.
pub(super) fn layout(ctx: &Ctx) -> Vec<Slot> {
    super::simple_grid(ctx)
}
