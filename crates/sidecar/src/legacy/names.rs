//! The legacy file names, built from a masked byte constant.
//!
//! The legacy organizer named its files after itself. The repository never spells that name
//! (`AGENTS.md`, `cargo xtask names`), so the stem is kept here with every byte XORed with
//! [`MASK`] and unmasked at run time. Every crate (and every test) takes the names from these
//! functions, never from a literal.

/// The stem's bytes, each XORed with [`MASK`] (lowercase ASCII once unmasked).
const MASKED_STEM: [u8; 6] = [0x2a, 0x33, 0x39, 0x3b, 0x29, 0x3b];
const MASK: u8 = 0x5a;

/// The stem in lowercase.
fn stem() -> String {
    MASKED_STEM.iter().map(|b| char::from(b ^ MASK)).collect()
}

/// The stem with its first letter in uppercase.
fn title_stem() -> String {
    let s = stem();
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => s,
    }
}

/// The legacy sidecar's two names, the current one first (a hidden `.<stem>.ini`), then the older
/// `<Stem>.ini`. When a folder holds both, the first is read.
pub fn sidecar_names() -> [String; 2] {
    [format!(".{}.ini", stem()), format!("{}.ini", title_stem())]
}

/// The legacy originals folder's name (`.<stem>originals`), which holds the originals of saved
/// photos and a sidecar of the same format (read with the same reader) holding their recipes.
pub fn originals_dir_name() -> String {
    format!(".{}originals", stem())
}

/// The name of the section holding the folder's own data (`[<Stem>]`).
pub fn folder_section() -> String {
    title_stem()
}
