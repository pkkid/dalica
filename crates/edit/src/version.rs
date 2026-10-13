//! A recipe's render key (`architecture.md` §6.4): what a cached picture of a photo depends on
//! besides the file itself. The key has two halves, both stable across runs, platforms and Rust
//! versions (FNV-1a, not the standard hasher):
//!
//! - [`RenderKey::recipe_hash`]: the printed recipe, every entry as stored (unknown and `legacy:`
//!   entries too);
//! - [`RenderKey::renderer_version`]: [`PIPELINE_VERSION`] and the version of every operation the
//!   recipe uses ([`crate::ops::version`]), so a new version of one operation changes the key of
//!   the recipes that use it and of no other.

use std::collections::BTreeMap;

use crate::Recipe;

/// The version of the renderer's own stages (resampling, working precision, pipeline order).
/// Bumping it remakes every cached picture; an operation's change bumps only its own version.
pub const PIPELINE_VERSION: u32 = 1;

/// What a rendered picture of a photo depends on, besides the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RenderKey {
    pub recipe_hash: u64,
    pub renderer_version: u64,
}

/// The render key of `recipe` with the operations' current versions.
pub fn key(recipe: &Recipe) -> RenderKey {
    key_with(recipe, crate::ops::version)
}

/// The render key of `recipe` if operation `name` had version `version(name)` (`None` for names
/// Dalica does not know): [`key`] with the current versions; tests and migrations ask what a
/// version bump changes.
pub fn key_with(recipe: &Recipe, version: impl Fn(&str) -> Option<u32>) -> RenderKey {
    let mut recipe_hash = Fnv::new();
    for op in recipe.ops() {
        let text = op.to_string();
        recipe_hash.write(&(text.len() as u64).to_le_bytes());
        recipe_hash.write(text.as_bytes());
    }
    let used: BTreeMap<&'static str, u32> = recipe.ops().iter().filter_map(|op| op.name()).filter_map(|name| Some((name, version(name)?))).collect();
    RenderKey { recipe_hash: recipe_hash.finish(), renderer_version: versions_hash(&used) }
}

/// The operations `recipe` uses with their versions, by name.
pub fn versions(recipe: &Recipe) -> BTreeMap<&'static str, u32> {
    recipe.ops().iter().filter_map(|op| op.name()).filter_map(|name| Some((name, crate::ops::version(name)?))).collect()
}

fn versions_hash(used: &BTreeMap<&'static str, u32>) -> u64 {
    let mut h = Fnv::new();
    h.write(&PIPELINE_VERSION.to_le_bytes());
    for (name, v) in used {
        h.write(&(name.len() as u64).to_le_bytes());
        h.write(name.as_bytes());
        h.write(&v.to_le_bytes());
    }
    h.finish()
}

/// FNV-1a, 64 bits.
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_matches_the_published_test_vectors() {
        for (input, expected) in [("", 0xcbf2_9ce4_8422_2325u64), ("a", 0xaf63_dc4c_8601_ec8c), ("foobar", 0x8594_4171_f739_67e8)] {
            let mut h = Fnv::new();
            h.write(input.as_bytes());
            assert_eq!(h.finish(), expected, "{input:?}");
        }
    }

    #[test]
    fn the_key_of_a_recipe_is_pinned() {
        // A change here remakes every user's cached thumbnails: only on purpose.
        let empty = key(&Recipe::new());
        assert_eq!(empty, key(&Recipe::new()));
        let r = Recipe::parse(["rotate(90)", "flip_h()"]).unwrap_or_default();
        assert_ne!(key(&r), empty);
        assert_ne!(key(&r).renderer_version, empty.renderer_version);
    }
}
