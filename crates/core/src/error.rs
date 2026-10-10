//! The crate's one error type. Every fallible function in `dalica-core` returns it.

/// Why a value was refused. Messages name the offending value, cut to a readable length.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// An album or person id that does not have its documented shape (`ids` module).
    #[error("not a valid {kind}: {value:?}")]
    InvalidId { kind: &'static str, value: String },
    /// A normalised rectangle outside 0..1, inverted, empty or not finite.
    #[error("not a valid normalised rectangle: {0}")]
    InvalidRect(String),
    /// A pixel rectangle or image size that cannot be normalised.
    #[error("not a valid pixel rectangle: {0}")]
    InvalidPixels(String),
    /// A position key with characters or structure outside its alphabet (`poskey` module).
    #[error("not a valid position key: {0:?}")]
    InvalidKey(String),
    /// `between(a, b)` called with `a >= b`.
    #[error("position keys out of order: {a:?} is not before {b:?}")]
    KeysNotOrdered { a: String, b: String },
    /// A position key that would exceed [`crate::poskey::MAX_KEY_LEN`].
    #[error("position key longer than {0} bytes")]
    KeyTooLong(usize),
    /// Every numbered candidate for a name is taken (`names` module).
    #[error("no free name for {0:?}")]
    NamesExhausted(String),
    /// A date or time that is not one of the accepted forms, or not a real calendar date.
    #[error("not a valid date: {0:?}")]
    InvalidDate(String),
}

/// `value` cut to at most 64 bytes on a character boundary, for error messages about hostile input.
pub(crate) fn excerpt(value: &str) -> String {
    const MAX: usize = 64;
    if value.len() <= MAX {
        return value.to_string();
    }
    let mut end = MAX;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", value.get(..end).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::excerpt;

    #[test]
    fn excerpt_cuts_on_a_char_boundary() {
        assert_eq!(excerpt("short"), "short");
        let long = "é".repeat(40); // 80 bytes
        let cut = excerpt(&long);
        assert!(cut.ends_with('…'));
        assert_eq!(cut.trim_end_matches('…').len(), 64);
    }
}
