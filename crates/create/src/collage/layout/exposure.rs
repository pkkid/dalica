//! The Multiple Exposure layout (`creating.md` §3): every picture covers the whole page (cropped
//! to the page's shape, centred), unturned, in slot order. The render blends them with equal
//! weights; the style has no background, so nothing of the page shows. The seed changes nothing.

use super::{Ctx, Slot, fill};

/// One slot per picture of `ctx`, in its order.
pub(super) fn layout(ctx: &Ctx) -> Vec<Slot> {
    ctx.aspects.iter().map(|&aspect| Slot { centre: [0.5, 0.5], size: [1.0, 1.0], angle: 0.0, shown: fill(aspect, ctx.page_ratio) }).collect()
}
