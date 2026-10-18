//! Suggestions (`faces.md` §5; the state is `crate::suggestions`): `faces.suggest {ids? |
//! person?}`, `faces.confirm {faces}`, `faces.confirmAll {person}`, `faces.reject {faces}`,
//! `faces.findMore {person}` and `faces.evaluate`. Empty until M13.26.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
