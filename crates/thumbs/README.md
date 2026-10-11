# dalica-thumbs

**Layer L2.** The prioritised, de-duplicating job pool, the in-memory LRU and the on-disk thumbnail cache.

- **May depend on:** `image`, `edit`, `fs`, `core` (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API (M1.12):**
  - `Pool<K, T>`: `new(workers)` (`default_workers()` = cores − 1, clamped 2..=8), `submit(key, Priority, work)` → `Queued` / `Merged` (priority raised) / `Running`, `cancel(key)`, `queued()`, `try_recv()` / `recv_timeout()` for `(key, result)`. Highest priority first, first come first served within one; one job per key; a cancelled job is never delivered; dropping the pool cancels running jobs and joins the workers. `Priority` constants follow `architecture.md` §5.
  - `Lru<K, V>`: byte budget never exceeded, least recently used evicted first, an entry larger than the budget refused.
  - `thumbnail(path, long_side, &Limits)` → upright `Rgba8` (the thumbnail job).
- **Public API (M4.3), `pack`:** the packed disk cache of `architecture.md` §6.6.
  - `Cache::open(dir, Options)` takes `<dir>/lock` (a second process gets `Fs(Locked)`) and loads one pack + index per `Size` (`S128`, `S256`, `S512`, `S1024`, `Screen`); `get` / `contains` / `put` / `remove` / `remove_all(key)` / `sync`; `stats(size)`, `disk_bytes()`; `compaction_due`, `compact(size)`, `prune()` and `maintain()` (the background job: compact what passed 25 % dead, then prune past the 2 GiB budget down to 80 %, largest sizes first, oldest entries first). `Sync`: one lock per size, held for one lookup and one read or append.
  - `Key::new(path, file_size, mtime_ns, recipe_hash, renderer_version)`: the content-addressed key (§6.4), a stable 128-bit FNV-1a. Entries are opaque bytes; the ladder stores JPEG at `QUALITY` (90).
  - Files: `<size>-<generation as 8 hex>.pack` (16-byte header, entry bytes) and `.idx` (16-byte header, 40-byte records each with its own CRC-32; a torn tail is cut, a damaged record costs only its entry). An entry's bytes carry a CRC-32 checked on the first read in a process (a mismatch is a miss). A compaction writes generation n + 1 beside n through `fs` temporary files (pack first, then index; a generation counts only once its index exists) while gets and puts go on. Only names of this shape (`is_own_name`) are ever listed or deleted.
  - Bench: `cargo bench -p dalica-thumbs --bench pack [-- --count N --bytes N --json]` reads 100 k entries packed and as loose files.
- **Test it alone:** `cargo test -p dalica-thumbs` (no window, GPU, network or personal photos needed): fake jobs held behind gates show strict priority order, one run per duplicate key, cancellation and a 10 000-job stress run with nothing lost; the LRU against a model; the pack cache in temp folders (round trips at every size, torn and damaged files, compaction alongside puts and gets, pruning next to foreign files, an `fs` fault at every pack write, hostile bytes).
