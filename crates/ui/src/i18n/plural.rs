//! Plural rules (`localization.md` → Languages and plural rules): which of a language's forms a
//! count takes. A catalog's `@plural` entry lists the forms in the order of [`Plural::categories`].
//!
//! Adapted from PhotoCraft `crates/ui-egui/src/i18n/mod.rs` (revision d95fdf31; MIT OR
//! Apache-2.0): the rules as one enum rather than a function per language, so `cargo xtask i18n`
//! can read a language's form count from the registry.

/// A language's plural rule, for whole counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Plural {
    /// One form for every count: Chinese, Japanese, Korean, Indonesian, Thai, Vietnamese.
    Single,
    /// `one` for 1, `other` for the rest: English, German, Spanish, Italian, Dutch, Greek.
    OneOther,
    /// `one` for 0 and 1, `other` from 2: French, Brazilian Portuguese.
    ZeroOneOther,
    /// `one` (…1 but not …11), `few` (…2–…4 but not …12–…14), `many` (the rest): Russian,
    /// Ukrainian.
    EastSlavic,
    /// `one` for 1 only, `few` (…2–…4 but not …12–…14), `many` (the rest): Polish.
    Polish,
    /// `one` for 1, `few` for 2–4, `other` for the rest: Czech, Slovak.
    Czech,
}

impl Plural {
    /// Every rule, for tests and `cargo xtask i18n`.
    pub const ALL: [Plural; 6] = [Plural::Single, Plural::OneOther, Plural::ZeroOneOther, Plural::EastSlavic, Plural::Polish, Plural::Czech];

    /// The forms' categories, in the order a catalog lists them.
    pub fn categories(self) -> &'static [&'static str] {
        match self {
            Plural::Single => &["other"],
            Plural::OneOther | Plural::ZeroOneOther => &["one", "other"],
            Plural::EastSlavic | Plural::Polish => &["one", "few", "many"],
            Plural::Czech => &["one", "few", "other"],
        }
    }

    /// How many forms a catalog's `@plural` entry lists.
    pub fn forms(self) -> usize {
        self.categories().len()
    }

    /// The name `languages.rs` writes it under (`Plural::OneOther` → `"OneOther"`).
    pub fn name(self) -> &'static str {
        match self {
            Plural::Single => "Single",
            Plural::OneOther => "OneOther",
            Plural::ZeroOneOther => "ZeroOneOther",
            Plural::EastSlavic => "EastSlavic",
            Plural::Polish => "Polish",
            Plural::Czech => "Czech",
        }
    }

    /// The form count `n` takes: an index into [`Plural::categories`].
    pub fn index(self, n: u64) -> usize {
        let (last, last_two) = (n % 10, n % 100);
        let teen = (12..=14).contains(&last_two);
        match self {
            Plural::Single => 0,
            Plural::OneOther => usize::from(n != 1),
            Plural::ZeroOneOther => usize::from(n > 1),
            Plural::EastSlavic => match last {
                1 if last_two != 11 => 0,
                2..=4 if !teen => 1,
                _ => 2,
            },
            Plural::Polish => match last {
                _ if n == 1 => 0,
                2..=4 if !teen => 1,
                _ => 2,
            },
            Plural::Czech => match n {
                1 => 0,
                2..=4 => 1,
                _ => 2,
            },
        }
    }

    /// The category `n` takes (`"one"`, `"few"`, …).
    pub fn category(self, n: u64) -> &'static str {
        self.categories().get(self.index(n)).copied().unwrap_or("other")
    }
}
