# dalica-mcp

**Layer L5.** The MCP server over the command registry (headless or connected to a running app).

- **May depend on:** `engine` and lower layers (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M10 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-mcp` (no window, GPU, network or personal photos needed).
