//! The commands, one module per area (`architecture.md` §3). New areas add a module and one line
//! in [`specs`].

pub mod albums;
pub mod app;
pub mod batch;
pub mod caption;
pub mod compare;
pub mod crop;
pub mod dates;
pub mod delete;
pub mod duplicates;
pub mod edit;
pub mod effects;
pub mod export;
pub mod files;
pub mod folder_data;
pub mod folder_info;
pub mod folder_ops;
pub mod folders;
pub mod hide;
pub mod history;
pub mod import;
pub mod import_run;
pub mod info;
pub mod inspect;
pub mod keymap;
pub mod library;
pub mod options;
pub mod photo;
pub mod recipe;
pub mod rename;
pub mod rotate;
pub mod save;
pub mod save_copy;
pub mod search;
pub mod section;
pub mod slideshow;
pub mod tags;
pub mod tuning;
pub mod view;
pub mod viewer;

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
        // M10: empty until M10.5 (options), M10.6 (keymap), M10.7 (inspect) and M10.9 (recipe).
        options::specs(),
        keymap::specs(),
        inspect::specs(),
        recipe::specs(),
        folder_data::specs(),
        // M11: empty until M11.7 (import), M11.8 (import_run), M11.9 (batch), M11.10 (compare),
        // M11.11 (slideshow) and M11.12 (viewer).
        import::specs(),
        import_run::specs(),
        batch::specs(),
        compare::specs(),
        slideshow::specs(),
        viewer::specs(),
        // M12: the effect list and the colour pick (M12.5).
        effects::specs(),
    ]
    .concat();
    #[cfg(test)]
    let all = [all, testing::specs()].concat();
    all
}
