# dalica-thumbs

**Layer L2.** The prioritised, de-duplicating job pool, the in-memory LRU and the on-disk thumbnail cache.

- **May depend on:** `image`, `edit`, `fs`, `core` (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API (M1.12):**
  - `Pool<K, T>`: `new(workers)` (`default_workers()` = cores − 1, clamped 2..=8), `submit(key, Priority, work)` → `Queued` / `Merged` (priority raised) / `Running`, `cancel(key)`, `queued()`, `try_recv()` / `recv_timeout()` for `(key, result)`. Highest priority first, first come first served within one; one job per key; a cancelled job is never delivered; dropping the pool cancels running jobs and joins the workers. `Priority` constants follow `architecture.md` §5.
  - `Lru<K, V>`: byte budget never exceeded, least recently used evicted first, an entry larger than the budget refused.
  - `thumbnail(path, long_side, &Limits)` → upright `Rgba8` (the thumbnail job).
- **Test it alone:** `cargo test -p dalica-thumbs` (no window, GPU, network or personal photos needed): fake jobs held behind gates show strict priority order, one run per duplicate key, cancellation and a 10 000-job stress run with nothing lost; the LRU against a model.
