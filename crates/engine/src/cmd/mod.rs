//! The commands, one module per area (`architecture.md` §3). New areas add a module and one line
//! in [`specs`].

pub mod app;
pub mod edit;
pub mod folders;
pub mod library;
pub mod photo;
pub mod section;
pub mod view;

#[cfg(test)]
pub(crate) mod testing;

use crate::registry::CommandSpec;

/// Every registered command.
pub fn specs() -> Vec<CommandSpec> {
    let all = [photo::specs(), library::specs(), edit::specs(), folders::specs(), app::specs(), view::specs(), section::specs()].concat();
    #[cfg(test)]
    let all = [all, testing::specs()].concat();
    all
}
