# dalica-scan

**Layer L3.** Watched-folder rules, walks, change detection, `notify` events, offline volumes and card/folder import; turns what it finds into `Op`s.

- **May depend on:** L0–L2 (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API (M1.11):**
  - `scan(roots, cancel)` → `Scan { walked, folders, warnings }`: bounded walks through `dalica-fs`, `skipped()` leaves out dot files and folders (so `.dalica.json`, `.dalica-originals`) and OS system folders; JPEG and PNG recognised by content; size as displayed and capture date from the headers; stars from each folder's sidecar. Never fails as a whole: unreadable parts are warnings.
  - `diff(&mut Library, &Scan)` → the ops (batches of at most `BATCH` = 500 photos) that make the model match the disk: new folders, removed / renamed / changed / added photos, folders gone. A delete plus an add with the same size and mtime in one folder is a rename (id and star kept). Folders under roots that could not be walked are never removed.
- **Test it alone:** `cargo test -p dalica-scan` (no window, GPU, network or personal photos needed): temp trees of `scenes` photos; the verification pass emits exactly the ops for an add, a delete, a rename and a rewrite; 1000 JPEGs are scanned and timed.
