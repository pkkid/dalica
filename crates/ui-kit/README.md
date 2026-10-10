# dalica-ui-kit

**Layer L5.** The generic look: `Tokens`, widgets, icons drawn in code, the widget registry and the CPU rasterizer for screenshots. Knows nothing about photos or the engine.

- **May depend on:** `core` (+ egui; tests also use `scenes` for fingerprints) (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** `Tokens` (+ `Tokens::apply`, `theme::contrast`), `fonts` (Inter, installed in
  code), `icons::paint` + `Icon`, `widgets::{text_button, icon_button, star_toggle, badge_row,
  star_badge}`, `registry::{register, widgets, find}` + `WidgetInfo`, `Rasterizer` and
  `snapshot`/`raw_input`/`paint` for headless frames.
- **Test it alone:** `cargo test -p dalica-ui-kit` (no window, GPU, network or personal photos needed).
  The widget gallery test writes its render to `target/test-output/ui-kit/gallery.png`;
  `DALICA_BLESS=1` stores a new fingerprint in `tests/fingerprints/` (look at the PNG first).
