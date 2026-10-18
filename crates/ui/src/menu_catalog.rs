//! The menu catalog: the complete menu tree of `menus-and-shortcuts.md` §1 as data (labels, command
//! ids, default shortcuts, separators, submenus, check marks and radio groups), in the spec's order.
//! It changes nothing and knows nothing about the running app: [`crate::menus`] turns it into the
//! menu model (which items are live, enabled and checked, which shortcut is shown), and that model
//! feeds the in-window menus, `ui.menu.tree` / `ui.menu.invoke` and the native macOS menu bar.
//!
//! An item whose command id has a registered command is live; the others are drawn disabled.
//! Giving a new command the id written here lights up its menu item (`architecture.md` §3).
//!
//! Ids the spec leaves open are chosen here (and named in `menus-and-shortcuts.md`): rows of one
//! command that differ by a parameter (`view.caption` → Filename) carry a [`Item::part`], which
//! becomes the last part of the widget id (`menuitem:view.caption/filename`) and the value of the
//! parameter the row passes.

/// How an item shows its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A plain item.
    None,
    /// Checkable: a check mark while on.
    Check,
    /// One of a radio group (the neighbouring radio items up to a separator): a dot while chosen.
    Radio,
}

/// Where an item lives on macOS, whose application menu takes some items (`menus-and-shortcuts.md`
/// §1, conventions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mac {
    /// The same place on every platform.
    Here,
    /// Not in this menu on macOS: the application menu holds it ([`MAC_APP_MENU`]).
    AppMenu,
}

/// One menu item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    /// English label, `…` when it opens a dialog (also the i18n key).
    pub label: &'static str,
    /// The command it runs.
    pub id: &'static str,
    /// For rows of one command that differ by a parameter: the variant's name (the widget id's
    /// last part).
    pub part: Option<&'static str>,
    /// The fixed parameters the row passes, as a JSON object (`""` for none).
    pub params: &'static str,
    /// The default shortcut shown, written as the spec writes it (`Ctrl` = Cmd on macOS).
    pub shortcut: Option<&'static str>,
    /// A macOS shortcut that is not the default with Ctrl read as Cmd (Redo: Cmd+Shift+Z).
    pub mac_shortcut: Option<&'static str>,
    pub mark: Mark,
    pub mac: Mac,
    /// In the Album variant of the fourth menu: the command it runs there instead.
    pub album_id: Option<&'static str>,
    /// In the fourth menu: disabled while the section is an album.
    pub folder_only: bool,
}

/// One row of a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Item(Item),
    Separator,
    /// A submenu: its label and rows.
    Submenu(&'static str, &'static [Entry]),
}

/// One menu of the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Menu {
    /// The English title: the widget id `menu:<key>` and the title shown.
    pub key: &'static str,
    /// The title shown while the current section is an album or person (the fourth menu).
    pub album_title: Option<&'static str>,
    pub entries: &'static [Entry],
    /// The rows shown while the current section is an album (the fourth menu's Album variant,
    /// `organizing.md` §9.2); `None`: the same rows.
    pub album_entries: Option<&'static [Entry]>,
}

/// A plain item running `id`.
pub const fn item(label: &'static str, id: &'static str) -> Item {
    Item {
        label,
        id,
        part: None,
        params: "",
        shortcut: None,
        mac_shortcut: None,
        mark: Mark::None,
        mac: Mac::Here,
        album_id: None,
        folder_only: false,
    }
}

impl Item {
    /// As a row.
    pub const fn e(self) -> Entry {
        Entry::Item(self)
    }

    pub const fn key(self, shortcut: &'static str) -> Item {
        Item { shortcut: Some(shortcut), ..self }
    }

    pub const fn mac_key(self, shortcut: &'static str) -> Item {
        Item { mac_shortcut: Some(shortcut), ..self }
    }

    pub const fn check(self) -> Item {
        Item { mark: Mark::Check, ..self }
    }

    pub const fn radio(self) -> Item {
        Item { mark: Mark::Radio, ..self }
    }

    /// A variant of a shared command: its name and the parameters it passes.
    pub const fn part(self, part: &'static str, params: &'static str) -> Item {
        Item { part: Some(part), params, ..self }
    }

    pub const fn app_menu(self) -> Item {
        Item { mac: Mac::AppMenu, ..self }
    }

    pub const fn album(self, id: &'static str) -> Item {
        Item { album_id: Some(id), ..self }
    }

    pub const fn folder_only(self) -> Item {
        Item { folder_only: true, ..self }
    }
}

const SEP: Entry = Entry::Separator;

/// §1.1 File.
pub const FILE: &[Entry] = &[
    item("New Album…", "album.new").key("Ctrl+N").e(),
    SEP,
    // Add Folder… opens the Folder Manager, as Tools ▸ Folder Manager… does (`organizing.md` §2);
    // `folders.add {path}` stays the engine's command for agents and the CLI.
    item("Add Folder…", "folders.manager").e(),
    item("Open File…", "library.openFile").key("Ctrl+O").e(),
    item("Import From…", "import.open").key("Ctrl+M").e(),
    SEP,
    item("Open in External Editor", "photo.openExternal").key("Ctrl+Shift+O").e(),
    SEP,
    item("Move to New Folder…", "photo.moveToNewFolder").e(),
    item("Rename…", "photo.rename").key("F2").e(),
    SEP,
    item("Save", "photo.save").key("Ctrl+S").e(),
    item("Revert…", "photo.revert").e(),
    SEP,
    item("Save As…", "photo.saveAs").e(),
    item("Save a Copy", "photo.saveCopy").e(),
    item("Export Pictures to Folder…", "output.export").key("Ctrl+Shift+S").e(),
    SEP,
    item("Locate on Disk", "photo.locate").key("Ctrl+Enter").e(),
    item("Delete from Disk…", "photo.delete").key("Delete").e(),
    SEP,
    item("Exit", "app.quit").key("Ctrl+Q").app_menu().e(),
];

/// §1.2 Edit. Undo and Redo show the action they undo ("Undo Star") when there is one.
pub const EDIT: &[Entry] = &[
    item("Undo", "edit.undo").key("Ctrl+Z").e(),
    item("Redo", "edit.redo").key("Ctrl+Y").mac_key("Cmd+Shift+Z").e(),
    SEP,
    item("Cut", "photo.cut").key("Ctrl+X").e(),
    item("Copy", "photo.copy").key("Ctrl+C").e(),
    item("Paste", "photo.paste").key("Ctrl+V").e(),
    SEP,
    item("Copy All Edits", "edit.copyRecipe").e(),
    item("Paste All Edits", "edit.pasteRecipe").e(),
    SEP,
    item("Select All", "select.all").key("Ctrl+A").e(),
    item("Select Starred", "select.starred").e(),
    item("Invert Selection", "select.invert").key("Ctrl+I").e(),
    item("Select None", "select.none").key("Ctrl+D").e(),
];

/// View ▸ Thumbnail Caption.
pub const CAPTION: &[Entry] = &[
    item("None", "view.caption").part("none", r#"{"caption":"none"}"#).radio().e(),
    item("Filename", "view.caption").part("filename", r#"{"caption":"filename"}"#).radio().e(),
    item("Caption", "view.caption").part("caption", r#"{"caption":"caption"}"#).radio().e(),
    item("Tags", "view.caption").part("tags", r#"{"caption":"tags"}"#).radio().e(),
    item("Resolution", "view.caption").part("resolution", r#"{"caption":"resolution"}"#).radio().e(),
    item("Date", "view.caption").part("date", r#"{"caption":"date"}"#).radio().e(),
];

/// View ▸ Folder View.
pub const FOLDER_VIEW: &[Entry] = &[
    item("Flat Folder View", "view.folderView").part("flat", r#"{"mode":"flat"}"#).radio().e(),
    item("Tree View", "view.folderView").part("tree", r#"{"mode":"tree"}"#).radio().e(),
    SEP,
    item("Simplified Tree View", "view.simplifiedTree").check().e(),
];

/// §1.3 View.
pub const VIEW: &[Entry] = &[
    item("Library View", "view.library").key("Ctrl+1").e(),
    SEP,
    item("Small Thumbnails", "view.thumbsSmall").key("Ctrl+1").radio().e(),
    item("Normal Thumbnails", "view.thumbsNormal").key("Ctrl+2").radio().e(),
    item("Edit View", "view.edit").key("Ctrl+3").e(),
    SEP,
    item("Properties", "pane.properties").key("Alt+Enter").check().e(),
    item("Tags", "pane.tags").key("Ctrl+T").check().e(),
    item("People", "pane.people").check().e(),
    item("Places", "pane.places").check().e(),
    SEP,
    item("Show Edit Controls", "edit.panelToggle").key("Ctrl+9").check().e(),
    SEP,
    item("Slideshow", "view.slideshow").key("Ctrl+4").e(),
    SEP,
    item("Small Pictures", "view.smallPictures").check().e(),
    item("Hidden Pictures", "view.hiddenPictures").check().e(),
    SEP,
    item("Use Color Management", "view.colorManagement").check().e(),
    SEP,
    Entry::Submenu("Thumbnail Caption", CAPTION),
    Entry::Submenu("Folder View", FOLDER_VIEW),
    SEP,
    item("Full Screen", "window.fullscreen").key("F11").e(),
];

/// Folder / Album ▸ Sort By.
pub const SORT: &[Entry] = &[
    item("Date", "section.sort").part("date", r#"{"by":"date"}"#).radio().e(),
    item("Name", "section.sort").part("name", r#"{"by":"name"}"#).radio().e(),
    item("Size", "section.sort").part("size", r#"{"by":"size"}"#).radio().e(),
    item("Reverse Order", "section.sort").part("reverse", r#"{"reverse":true}"#).check().e(),
];

/// §1.4 Folder / Album: also the section's context menu.
pub const SECTION: &[Entry] = &[
    item("Edit Description…", "section.properties").e(),
    item("View Slideshow", "section.slideshow").key("Ctrl+4").e(),
    SEP,
    item("Refresh Thumbnails", "section.refreshThumbs").e(),
    Entry::Submenu("Sort By", SORT),
    SEP,
    item("Hide", "folder.hide").e(),
    item("Unhide", "folder.unhide").e(),
    SEP,
    item("Locate on Disk", "folder.locate").key("Ctrl+Enter").folder_only().e(),
    item("Remove from Dalica…", "folder.remove").folder_only().e(),
    SEP,
    item("Move…", "folder.move").folder_only().e(),
    item("Delete…", "folder.delete").album("album.delete").e(),
];

/// Album ▸ Sort Album By (`organizing.md` §4.3): rewrites the album's order.
pub const ALBUM_SORT: &[Entry] = SORT;

/// The fourth menu's Album variant, also an album section's context menu (`organizing.md` §9.2):
/// Album Properties…, Delete Album…, the selection rows, Refresh Thumbnails and Sort Album By.
pub const ALBUM_SECTION: &[Entry] = &[
    item("Album Properties…", "section.properties").e(),
    item("Delete Album…", "album.delete").e(),
    SEP,
    item("Select All", "select.all").key("Ctrl+A").e(),
    item("Select None", "select.none").key("Ctrl+D").e(),
    item("Invert Selection", "select.invert").key("Ctrl+I").e(),
    SEP,
    item("Refresh Thumbnails", "section.refreshThumbs").e(),
    Entry::Submenu("Sort Album By", ALBUM_SORT),
];

/// The photo menu's Add to Album ▸: the albums by name (M9.18 adds them), then New Album….
pub const ADD_TO_ALBUM: &[Entry] = &[item("New Album…", "album.new").e()];

/// The photo menu's Open With ▸: the system's handlers (OS integration, M15).
pub const OPEN_WITH: &[Entry] = &[item("Choose Application…", "photo.openWith").e()];

/// The photo context menu of a thumbnail in a folder (`organizing.md` §9.1), in order.
pub const PHOTO: &[Entry] = &[
    item("View and Edit", "view.edit").key("Enter").e(),
    Entry::Submenu("Add to Album", ADD_TO_ALBUM),
    SEP,
    item("Rotate Clockwise", "photo.rotate.cw").key("Ctrl+R").e(),
    item("Rotate Counterclockwise", "photo.rotate.ccw").key("Ctrl+Shift+R").e(),
    SEP,
    item("Undo All Edits…", "edit.undoAll").e(),
    SEP,
    item("Hide", "photo.hide").e(),
    SEP,
    item("Move to New Folder…", "photo.moveToNewFolder").e(),
    item("Split Folder Here…", "photo.splitFolder").e(),
    SEP,
    item("Open in External Editor", "photo.openExternal").key("Ctrl+Shift+O").e(),
    Entry::Submenu("Open With", OPEN_WITH),
    SEP,
    item("Save", "photo.save").key("Ctrl+S").e(),
    item("Revert…", "photo.revert").e(),
    SEP,
    item("Locate on Disk", "photo.locate").key("Ctrl+Enter").e(),
    item("Delete from Disk…", "photo.delete").key("Delete").e(),
    item("Copy Path", "photo.copyPath").e(),
    SEP,
    item("Find Faces Again", "faces.reset").e(),
    SEP,
    item("Properties", "pane.properties").key("Alt+Enter").mac_key("Cmd+I").e(),
];

/// The photo context menu of a thumbnail in an album: Delete from Disk becomes Remove from Album,
/// and Show in Folder and Set as Album Cover join (`organizing.md` §4.3, §9.1).
pub const PHOTO_IN_ALBUM: &[Entry] = &[
    item("View and Edit", "view.edit").key("Enter").e(),
    Entry::Submenu("Add to Album", ADD_TO_ALBUM),
    SEP,
    item("Rotate Clockwise", "photo.rotate.cw").key("Ctrl+R").e(),
    item("Rotate Counterclockwise", "photo.rotate.ccw").key("Ctrl+Shift+R").e(),
    SEP,
    item("Undo All Edits…", "edit.undoAll").e(),
    SEP,
    item("Hide", "photo.hide").e(),
    SEP,
    item("Move to New Folder…", "photo.moveToNewFolder").e(),
    item("Split Folder Here…", "photo.splitFolder").e(),
    SEP,
    item("Open in External Editor", "photo.openExternal").key("Ctrl+Shift+O").e(),
    Entry::Submenu("Open With", OPEN_WITH),
    SEP,
    item("Save", "photo.save").key("Ctrl+S").e(),
    item("Revert…", "photo.revert").e(),
    SEP,
    item("Show in Folder", "photo.showInFolder").e(),
    item("Set as Album Cover", "album.setCover").e(),
    item("Locate on Disk", "photo.locate").key("Ctrl+Enter").e(),
    item("Remove from Album", "album.remove").key("Delete").e(),
    item("Copy Path", "photo.copyPath").e(),
    SEP,
    item("Find Faces Again", "faces.reset").e(),
    SEP,
    item("Properties", "pane.properties").key("Alt+Enter").mac_key("Cmd+I").e(),
];

/// The face menu's Move to Person ▸: one row per person by name, `faces.moveToPerson {person}`
/// (`menuitem:faces.moveToPerson/<PersonId>`), added when the menu opens (M13.21).
pub const MOVE_TO_PERSON: &[Entry] = &[];

/// The face context menu (`menus-and-shortcuts.md` §4: a face in the Unnamed grid, a person
/// section's Zoom to Faces cells, the People pane), in order. Its commands act on the faces
/// selected (M13).
pub const FACE: &[Entry] = &[
    item("Move to New Person…", "faces.moveToNewPerson").e(),
    Entry::Submenu("Move to Person", MOVE_TO_PERSON),
    item("Remove from Person", "faces.removeFromPerson").e(),
    item("Set as Person Thumbnail", "people.thumbnail").e(),
    item("Ignore Face", "faces.ignoreSelected").e(),
];

/// The person menu's Merge Into ▸: one row per other person by name, `people.merge {person, into}`
/// (`menuitem:people.merge/<PersonId>`), added when the menu opens (M13.25).
pub const MERGE_INTO: &[Entry] = &[];

/// The person context menu (`organizing.md` §6.3: a person's tree row and section header), in
/// order (M13).
pub const PERSON: &[Entry] =
    &[item("Rename…", "people.rename").e(), Entry::Submenu("Merge Into", MERGE_INTO), item("Delete Person", "people.delete").e()];

/// Submenus whose rows are all added when their menu opens (the people), empty in the catalog.
pub const DYNAMIC: &[&[Entry]] = &[MOVE_TO_PERSON, MERGE_INTO];

/// Commands whose only rows are those of a [`DYNAMIC`] submenu.
pub const DYNAMIC_COMMANDS: &[&str] = &["faces.moveToPerson", "people.merge"];

/// Picture ▸ Batch Edit (`editing.md` §9): applies to the selection or the tray.
pub const BATCH_EDIT: &[Entry] = &[
    item("Rename…", "photo.rename").part("batch", "").key("F2").e(),
    SEP,
    item("Rotate Clockwise", "photo.rotate.cw").part("batch", "").e(),
    item("Rotate Counterclockwise", "photo.rotate.ccw").part("batch", "").e(),
    SEP,
    item("Auto Contrast", "edit.apply").part("autocontrast", r#"{"op":"autocontrast"}"#).e(),
    item("Auto Color", "edit.apply").part("autocolor", r#"{"op":"autocolor"}"#).e(),
    item("Auto Fix", "edit.apply").part("autofix", r#"{"op":"autofix"}"#).e(),
    SEP,
    item("Sepia", "edit.apply").part("sepia", r#"{"op":"sepia"}"#).e(),
    item("Sharpen", "edit.apply").part("sharpen", r#"{"op":"sharpen"}"#).e(),
    item("Warmify", "edit.apply").part("warmify", r#"{"op":"warmify"}"#).e(),
    item("Film Grain", "edit.apply").part("grain", r#"{"op":"grain"}"#).e(),
    item("Black and White", "edit.apply").part("bw", r#"{"op":"bw"}"#).e(),
];

/// §1.5 Picture.
pub const PICTURE: &[Entry] = &[
    item("View and Edit", "view.edit").key("Ctrl+3").e(),
    Entry::Submenu("Batch Edit", BATCH_EDIT),
    SEP,
    item("Undo All Edits…", "edit.undoAll").e(),
    SEP,
    item("Hide", "photo.hide").e(),
    item("Unhide", "photo.unhide").e(),
    SEP,
    item("Find Faces Again", "faces.reset").e(),
    SEP,
    item("Properties", "pane.properties").key("Alt+Enter").mac_key("Cmd+I").e(),
    SEP,
    item("Star / Unstar", "photo.star.toggle").key("Ctrl+8").e(),
    item("Edit Caption", "photo.caption.edit").e(),
    item("Rotate Clockwise", "photo.rotate.cw").key("Ctrl+R").e(),
    item("Rotate Counterclockwise", "photo.rotate.ccw").key("Ctrl+Shift+R").e(),
    item("Flip Horizontal", "edit.flipH").key("Ctrl+Shift+H").e(),
    item("Flip Vertical", "edit.flipV").key("Ctrl+Shift+V").e(),
];

/// §1.6 Create.
pub const CREATE: &[Entry] = &[item("Picture Collage…", "create.collage").e()];

/// Tools ▸ Geotag.
pub const GEOTAG: &[Entry] =
    &[item("Clear Location…", "geo.clear").e(), item("Copy Location", "geo.copy").e(), item("Paste Location", "geo.paste").e()];

/// Tools ▸ Experimental ▸ Search for.
pub const SEARCH_COLOR: &[Entry] = &[
    item("Red", "search.color").part("red", r#"{"color":"red"}"#).e(),
    item("Orange", "search.color").part("orange", r#"{"color":"orange"}"#).e(),
    item("Yellow", "search.color").part("yellow", r#"{"color":"yellow"}"#).e(),
    item("Green", "search.color").part("green", r#"{"color":"green"}"#).e(),
    item("Blue", "search.color").part("blue", r#"{"color":"blue"}"#).e(),
    item("Purple", "search.color").part("purple", r#"{"color":"purple"}"#).e(),
    item("Black and White", "search.color").part("bw", r#"{"color":"bw"}"#).e(),
];

/// Tools ▸ Experimental.
pub const EXPERIMENTAL: &[Entry] = &[
    item("Show Duplicate Files", "search.duplicates").key("Ctrl+F6").e(),
    Entry::Submenu("Search for", SEARCH_COLOR),
    item("Save Search Results…", "search.saveAsAlbum").e(),
    item("Show Tag as Album…", "search.tagAsAlbum").e(),
    item("Write People Names to Photos…", "faces.writeXmp").e(),
];

/// §1.7 Tools.
pub const TOOLS: &[Entry] = &[
    item("Folder Manager…", "folders.manager").e(),
    item("People Manager…", "people.manager").e(),
    SEP,
    item("Configure Photo Viewer…", "viewer.settings").e(),
    SEP,
    item("Adjust Date and Time…", "photo.adjustDate").e(),
    SEP,
    Entry::Submenu("Geotag", GEOTAG),
    Entry::Submenu("Experimental", EXPERIMENTAL),
    SEP,
    item("Options…", "app.preferences").key("Ctrl+,").app_menu().e(),
];

/// §1.8 Help.
pub const HELP: &[Entry] = &[
    item("Help Contents", "help.contents").key("F1").e(),
    item("Keyboard Shortcuts…", "app.shortcuts").key("Ctrl+/").e(),
    SEP,
    item("Release Notes", "help.releaseNotes").e(),
    item("Report a Problem…", "help.reportProblem").e(),
    item("Show Log Folder", "help.logFolder").e(),
    item("Rebuild Library…", "library.rebuild").e(),
    SEP,
    item("About Dalica", "app.about").app_menu().e(),
];

/// The menu bar, left to right.
pub const MENU_BAR: &[Menu] = &[
    Menu { key: "File", album_title: None, entries: FILE, album_entries: None },
    Menu { key: "Edit", album_title: None, entries: EDIT, album_entries: None },
    Menu { key: "View", album_title: None, entries: VIEW, album_entries: None },
    Menu { key: "Folder", album_title: Some("Album"), entries: SECTION, album_entries: Some(ALBUM_SECTION) },
    Menu { key: "Picture", album_title: None, entries: PICTURE, album_entries: None },
    Menu { key: "Create", album_title: None, entries: CREATE, album_entries: None },
    Menu { key: "Tools", album_title: None, entries: TOOLS, album_entries: None },
    Menu { key: "Help", album_title: None, entries: HELP, album_entries: None },
];

/// The macOS application menu, first on the bar there: About, Preferences…, Hide, Quit.
pub const MAC_APP_MENU: Menu = Menu {
    key: "Dalica",
    album_title: None,
    album_entries: None,
    entries: &[
        item("About Dalica", "app.about").e(),
        SEP,
        item("Preferences…", "app.preferences").key("Ctrl+,").e(),
        SEP,
        item("Hide Dalica", "app.hide").e(),
        item("Quit Dalica", "app.quit").key("Ctrl+Q").e(),
    ],
};

/// Every item of `entries`, submenus included, depth first in menu order.
pub fn items(entries: &'static [Entry]) -> Vec<&'static Item> {
    let mut out = Vec::new();
    let mut stack: Vec<std::slice::Iter<'static, Entry>> = vec![entries.iter()];
    while let Some(top) = stack.last_mut() {
        match top.next() {
            None => {
                stack.pop();
            }
            Some(Entry::Item(i)) => out.push(i),
            Some(Entry::Separator) => {}
            Some(Entry::Submenu(_, children)) => stack.push(children.iter()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fixed_parameter_list_is_a_json_object() {
        for menu in MENU_BAR.iter().chain([&MAC_APP_MENU]) {
            for i in items(menu.entries) {
                if !i.params.is_empty() {
                    let v: serde_json::Value = serde_json::from_str(i.params).unwrap();
                    assert!(v.is_object(), "{}: {}", i.label, i.params);
                }
                assert!(i.part.is_some() || i.params.is_empty(), "{}: parameters belong to a variant", i.label);
            }
        }
    }

    #[test]
    fn menus_never_start_or_end_with_a_separator_or_repeat_one() {
        fn check(name: &str, entries: &'static [Entry]) {
            // The people's submenus are filled when they open.
            if DYNAMIC.iter().any(|d| std::ptr::eq(*d, entries)) {
                assert!(entries.is_empty(), "{name}");
                return;
            }
            assert!(!matches!(entries.first(), Some(Entry::Separator) | None), "{name}");
            assert!(!matches!(entries.last(), Some(Entry::Separator)), "{name}");
            for w in entries.windows(2) {
                assert!(!(w[0] == Entry::Separator && w[1] == Entry::Separator), "{name}");
            }
            for e in entries {
                if let Entry::Submenu(label, children) = e {
                    check(label, children);
                }
            }
        }
        for menu in MENU_BAR.iter().chain([&MAC_APP_MENU]) {
            check(menu.key, menu.entries);
        }
        check("Album", ALBUM_SECTION);
        check("Photo", PHOTO);
        check("Photo in an album", PHOTO_IN_ALBUM);
        check("Face", FACE);
        check("Person", PERSON);
    }

    #[test]
    fn the_face_and_person_menus_list_the_spec_rows_in_order() {
        let labels = |e| items(e).iter().map(|i| i.label).collect::<Vec<_>>();
        assert_eq!(labels(FACE), ["Move to New Person…", "Remove from Person", "Set as Person Thumbnail", "Ignore Face"]);
        assert_eq!(labels(PERSON), ["Rename…", "Delete Person"]);
        assert!(matches!(FACE[1], Entry::Submenu("Move to Person", _)) && matches!(PERSON[1], Entry::Submenu("Merge Into", _)));
    }

    #[test]
    fn items_walks_submenus_in_order() {
        let labels: Vec<&str> = items(TOOLS).iter().map(|i| i.label).collect();
        assert_eq!(labels[..4], ["Folder Manager…", "People Manager…", "Configure Photo Viewer…", "Adjust Date and Time…"]);
        let search = labels.iter().position(|l| *l == "Show Duplicate Files").unwrap();
        assert_eq!(labels[search + 1], "Red", "Experimental ▸ Search for ▸ follows Show Duplicate Files");
        assert_eq!(labels.last(), Some(&"Options…"));
    }
}
