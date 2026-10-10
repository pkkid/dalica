# dalica-meta

**Layer L1.** EXIF / IPTC / XMP reading; write-back of owned fields by segment surgery (merge, never replace); MWG face regions; `.xmp` sidecars for RAW.

- **May depend on:** L0 and `image` (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M6 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-meta` (no window, GPU, network or personal photos needed).
