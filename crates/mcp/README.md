# dalica-mcp

**Layer L5.** The MCP server over the command registry (headless or connected to a running app): newline-delimited JSON-RPC 2.0 over stdio, hand-written, no async runtime (`docs/mcp.md`). Ported from LightCraft's `crates/mcp` @ b3d392b4 (`NOTICE`).

- **May depend on:** `engine` and lower layers (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** `Server` (`serve` on any reader and writer, `handle_line` for one message, `with_compact`); the `Backend` trait (`call` a control-protocol method with a `Progress`, `render`, `has_ui`, `describe`, `tick`) and `ToolError` (the control protocol's `code`); `Headless` (`open(Source::Demo | Library(dir) | Empty, folders)`), its `Tray` and `Roots`.
- **Modules:** `server` (the MCP methods, the panic guard, the 8 MiB result cap), `jsonrpc` (messages and the stdio transport, 1 MiB lines), `progress` (progress notifications, cancellation, `ping` during a job), `backend`, `headless`, `tray`, `roots`, `tools/` (one module per group: `core` is built; `inspect`, `organize`, `edit`, `output`, `commands`, `ui` are empty until their M10 tasks), `resources` and `connect` (empty until M10.15 and M10.17).
- **Test it alone:** `cargo test -p dalica-mcp` (no window, GPU, network or personal photos needed): `tests/server.rs` drives the server through the real framing on in-process pipes (`tests/common/`); `dalica-cli`'s `tests/mcp_cli.rs` runs `dalica-cli mcp --demo` on real stdio.
