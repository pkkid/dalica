# dalica-scenes

**Layer L1.** Procedural test and demo photos (gradients, horizons, soft blobs, text-free patterns) at any size, depth and orientation, with a minimal EXIF block. No media files.

- **May depend on:** `core` (`docs/architecture.md` §2; `cargo xtask layers` enforces it). External: `image` (its JPEG encoder), `png`, `thiserror`.
- **Public API:**
  - `Scene { seed, width, height, kind }` → `render()` → `Rgb8` (upright). `Kind`: `Gradient`, `Horizon`, `Blobs`. A light square near the top-left marks "up and left". Only `+ − × ÷ √` and a SplitMix64 generator, so pixels and file bytes are identical on every platform and build profile (`tests/scenes.rs` pins golden hashes).
  - `jpeg(scene, quality, Option<&Exif>)`, `png(scene)`. `Exif { orientation, date_time_original, make, model, thumbnail, byte_order }`: the pixels are stored so that the orientation displays them upright; IFD0, the EXIF IFD and an optional IFD1 thumbnail, in `II` or `MM` order.
  - `orient::store` / `orient::display`, `exif::tiff`, `Rgb8::downscale`.
  - `demo_library(folders, photos, seed)` → `Vec<DemoFile>` (relative path, bytes, orientation, date, size). This crate never writes to disk (layer rule 4); callers write the files (through `dalica-fs` in the product).
- **Test it alone:** `cargo test -p dalica-scenes` (no window, GPU, network or personal photos needed). The `image` crate also serves as the independent decoder in tests. Dev builds compile this crate, `image` and `png` at `opt-level = 3` (root `Cargo.toml`), so a 200-photo demo library takes under a second.
