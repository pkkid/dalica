//! The palette of Studio (Dark) and Studio (Light), to be ported value for value from PhotoCraft
//! `crates/ui-egui/src/theme.rs` at revision 3a896a5e by M16.6. Until then it answers `None`,
//! and the theme wears Dark's values.

use super::super::{Theme, ThemeKind};

/// The palette and overrides of `kind`, or `None` while it is not ported.
pub(super) fn theme(kind: ThemeKind) -> Option<Theme> {
    let _ = kind;
    None
}
