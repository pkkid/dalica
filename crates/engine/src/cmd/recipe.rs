//! `edit.recipe.set {ids, recipe, label?, dropUnknown?}` (`mcp.md` §4.2, `data-model.md` §3.1):
//! replaces photos' recipes, each as one step of its own history, refusing to drop an operation it
//! cannot read unless `dropUnknown`. Empty until M10.9.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
