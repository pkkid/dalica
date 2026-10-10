# dalica-image

**Layer L1.** Sniffing, scaled decoding, encoding, orientation, ICC → sRGB, decode limits, embedded thumbnails and RAW previews (a bounded TIFF/IFD walker of our own).

- **May depend on:** L0 (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M1.5, M1.6 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-image` (no window, GPU, network or personal photos needed).
