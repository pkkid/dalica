//! Cache upkeep (`architecture.md` §6.6): the pruning policy over the packed [`Cache`], and the
//! removal of entries nobody will ask for again.
//!
//! - **Pruning** ([`prune`]) runs on a pool worker, at start and whenever the cache has passed its
//!   [`Budget`] (2 GiB by default, the user's cache size setting): it takes the cache down to
//!   [`Budget::prune_to_percent`] (80 %) of the budget, the largest sizes first and, within a size,
//!   its oldest entries first, and stops there. Entries the caller asks to keep (the photos of
//!   offline folders, which could not be made again until their volume returns) are stepped over;
//!   when only kept entries are left the cache stays above the target, and [`Pruned::kept`] says
//!   how many were spared.
//! - **Forgetting** ([`forget`]) makes dead the entries of photos that left the library (or whose
//!   file changed); the next compaction gives their bytes back.
//!
//! Both only ever remove cache entries: the pack's own files, through [`Cache`] (which lists and
//! deletes only files of its own naming shape). Nothing here opens a photo or a sidecar. The cache
//! may be shared by several libraries (one at a time): entries this library does not know are
//! never forgotten, only pruned like any other entry, oldest first.

use crate::pack::{Cache, DEFAULT_BUDGET, Key, Result, Size};

/// Bytes an index record takes on disk (`pack/format.rs`): what each live entry costs besides its
/// own bytes, and what a compaction gives back per dead record.
const RECORD_BYTES: u64 = 40;
/// Bytes of an empty pack and its index (their two 16-byte headers).
const EMPTY_BYTES: u64 = 32;

/// The smallest and largest cache size the setting accepts.
pub const MIN_BUDGET: u64 = 64 << 20;
pub const MAX_BUDGET: u64 = 1 << 40;
/// What pruning shrinks the cache to, in percent of the budget.
pub const PRUNE_TO_PERCENT: u8 = 80;

/// How large the cache may grow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Disk bytes of every pack and index together before pruning starts.
    pub bytes: u64,
    /// What pruning shrinks the cache to, in percent of `bytes`.
    pub prune_to_percent: u8,
}

impl Default for Budget {
    fn default() -> Budget {
        Budget { bytes: DEFAULT_BUDGET, prune_to_percent: PRUNE_TO_PERCENT }
    }
}

impl Budget {
    /// A budget of `bytes`, clamped to [`MIN_BUDGET`]`..=`[`MAX_BUDGET`].
    pub fn of(bytes: u64) -> Budget {
        Budget { bytes: bytes.clamp(MIN_BUDGET, MAX_BUDGET), ..Budget::default() }
    }

    /// The disk bytes pruning stops at.
    pub fn target(&self) -> u64 {
        u64::try_from(u128::from(self.bytes) * u128::from(self.prune_to_percent.min(100)) / 100).unwrap_or(u64::MAX)
    }
}

/// What [`prune`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    /// Sizes it took entries from, in the order it took them.
    pub sizes: Vec<Size>,
    /// Entries it removed.
    pub removed: u64,
    /// Entries it stepped over because the caller keeps them.
    pub kept: u64,
    /// Disk bytes before and after.
    pub before: u64,
    pub after: u64,
}

/// Whether the cache has passed `budget` (from memory; no disk access).
pub fn over(cache: &Cache, budget: &Budget) -> bool {
    cache.disk_bytes() > budget.bytes
}

/// What `size` would take on disk once compacted: its two headers, its live entries and one
/// record each.
fn compacted(cache: &Cache, size: Size) -> u64 {
    let s = cache.stats(size);
    EMPTY_BYTES.saturating_add(s.live_bytes).saturating_add(s.entries.saturating_mul(RECORD_BYTES))
}

/// Shrinks the cache to [`Budget::target`] once it is over `budget`: the largest sizes first,
/// within a size the oldest entries first, stepping over the entries `keep` names; then compacts
/// the sizes it took from (and any other with dead bytes while the cache is still over the target),
/// so the bytes really go. Does nothing while the cache is within its budget. Run it on a worker.
pub fn prune(cache: &Cache, budget: &Budget, keep: &dyn Fn(Size, Key) -> bool) -> Result<Pruned> {
    let before = cache.disk_bytes();
    let mut out = Pruned { before, after: before, ..Pruned::default() };
    if before <= budget.bytes {
        return Ok(out);
    }
    let target = budget.target();
    let mut total: u64 = Size::ALL.into_iter().map(|s| compacted(cache, s)).fold(0, u64::saturating_add);
    for size in Size::LARGEST_FIRST {
        if total <= target {
            break;
        }
        let mut took = false;
        for (key, len) in cache.entries(size) {
            if total <= target {
                break;
            }
            if keep(size, key) {
                out.kept = out.kept.saturating_add(1);
                continue;
            }
            if cache.remove(size, key)? {
                total = total.saturating_sub(u64::from(len).saturating_add(RECORD_BYTES));
                out.removed = out.removed.saturating_add(1);
                took = true;
            }
        }
        if took {
            out.sizes.push(size);
        }
    }
    for size in Size::LARGEST_FIRST {
        let stats = cache.stats(size);
        let dead = stats.dead_bytes > 0 || stats.dead_records > 0;
        if dead && (out.sizes.contains(&size) || cache.disk_bytes() > target) {
            // `Ok(false)`: another compaction of this size is running and drops the same entries.
            cache.compact(size)?;
        }
    }
    out.after = cache.disk_bytes();
    Ok(out)
}

/// Makes dead every entry in `entries` (each a size and the key it is stored under there).
/// Returns how many there were.
pub fn forget(cache: &Cache, entries: &[(Size, Key)]) -> Result<usize> {
    let mut n = 0;
    for &(size, key) in entries {
        if cache.remove(size, key)? {
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_are_clamped_and_their_target_is_their_share() {
        assert_eq!(Budget::of(0).bytes, MIN_BUDGET);
        assert_eq!(Budget::of(u64::MAX).bytes, MAX_BUDGET);
        assert_eq!(Budget::default().target(), DEFAULT_BUDGET * 4 / 5);
        let b = Budget { bytes: 1000, prune_to_percent: 250 };
        assert_eq!(b.target(), 1000, "a share over 100 % is 100 %");
    }
}
