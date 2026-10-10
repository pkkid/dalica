# dalica-ui

**Layer L5.** The app: serialisable `UiState`, panels, menus and the menu catalog, shortcut dispatch, `UI_COMMANDS`, the headless driver and the control-method handlers.

- **May depend on:** `engine`, `ui-kit` and lower layers (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** `App` (`run`: UI-only `UI_COMMANDS` here, everything else forwarded to the
  engine's `Session::execute`; `frame` paints one frame), `UiState` (serialisable view state),
  `headless::Driver` (`step`, `step_until`, `settle`, `click(id)`, `key("Ctrl+8")`, `run`, `state`,
  `widgets`, `screenshot` on a virtual 60 Hz clock).
- **Test it alone:** `cargo test -p dalica-ui` (no window, GPU, network or personal photos needed).
  From the command line: `dalica-cli snapshot --library DIR -o out.png --size 1400x900`.
