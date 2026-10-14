//! The commands, one module per area (`architecture.md` §3). New areas add a module and one line
//! in [`specs`].

pub mod albums;
pub mod app;
pub mod caption;
pub mod crop;
pub mod dates;
pub mod delete;
pub mod duplicates;
pub mod edit;
pub mod export;
pub mod files;
pub mod folder_info;
pub mod folder_ops;
pub mod folders;
pub mod hide;
pub mod history;
pub mod info;
pub mod library;
pub mod photo;
pub mod rename;
pub mod rotate;
pub mod save;
pub mod save_copy;
pub mod search;
pub mod section;
pub mod tags;
pub mod tuning;
pub mod view;

#[cfg(test)]
pub(crate) mod testing;

use crate::registry::CommandSpec;

/// Every registered command.
pub fn specs() -> Vec<CommandSpec> {
    let all = [
        photo::specs(),
        library::specs(),
        edit::specs(),
        folders::specs(),
        app::specs(),
        view::specs(),
        section::specs(),
        // M6: empty until M6.14 (rotate, hide, caption, folder_info), M6.15 (tags) and M6.16
        // (dates, info) fill them.
        rotate::specs(),
        hide::specs(),
        caption::specs(),
        folder_info::specs(),
        tags::specs(),
        dates::specs(),
        info::specs(),
        // M7: the photo's own history (M7.3); empty until M7.17 (crop) and M7.18 (tuning).
        history::specs(),
        crop::specs(),
        tuning::specs(),
        // M8: empty until M8.15 (save), M8.17 (save_copy) and M8.16 (export).
        save::specs(),
        save_copy::specs(),
        export::specs(),
        // M9: empty until M9.10 (albums), M9.11 (search), M9.13 (files), M9.14 (rename), M9.15
        // (delete, folder_ops) and M9.16 (duplicates).
        albums::specs(),
        search::specs(),
        files::specs(),
        rename::specs(),
        delete::specs(),
        folder_ops::specs(),
        duplicates::specs(),
    ]
    .concat();
    #[cfg(test)]
    let all = [all, testing::specs()].concat();
    all
}
