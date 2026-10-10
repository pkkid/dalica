# dalica-ui

**Layer L5.** The app: serialisable `UiState`, panels, menus and the menu catalog, shortcut dispatch, `UI_COMMANDS`, the headless driver and the control-method handlers.

- **May depend on:** `engine`, `ui-kit` and lower layers (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M1.17, M1.18 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-ui` (no window, GPU, network or personal photos needed).
