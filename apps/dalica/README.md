# dalica

Dalica desktop app: the eframe window, the control-server transport, the native menu and the rfd picker (`docs/architecture.md` §2). Apps may depend on any crate.

- **Status:** an empty window (M0.8) with its log file (M1.20: `src/logging.rs`, `docs/development.md` §10); the library window comes in M1.18 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica` (the logging tests run the test binary and the `dalica` binary as child processes with their settings and temp folders in a temporary directory).
