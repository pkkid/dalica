//! dalica-mcp: the MCP server over the command registry (`docs/mcp.md`).
//!
//! [Model Context Protocol](https://modelcontextprotocol.io) over stdio: newline-delimited
//! JSON-RPC 2.0, hand-written (no async runtime, no new dependency). Layer L5; see
//! `docs/architecture.md` §2. Ported from LightCraft `crates/mcp/src/lib.rs`, `server.rs`,
//! `tools.rs`, `backend.rs`, `headless.rs` and `progress.rs` @ b3d392b4, MIT OR Apache-2.0
//! (`NOTICE`); each module names what it took.
//!
//! | Module | What |
//! |---|---|
//! | [`server`] | the MCP methods: `initialize`, `ping`, `tools/*`, `resources/*`; a panic in a tool becomes `internal` |
//! | `jsonrpc` | JSON-RPC 2.0 messages and the stdio transport: a reader thread (1 MiB lines), a writer thread |
//! | `progress` | `notifications/progress` (≤ 10 per second plus the final one), cancellation and `ping` while a tool runs |
//! | [`backend`] | the [`Backend`] trait: where tool calls end up (control-protocol methods) |
//! | [`headless`] | the in-process backend: an engine `Session` with no window (`--demo`, `--library`, folders, empty) |
//! | [`tray`] | headless MCP's own selection and Photo Tray (`ui-layout.md` §5.4) |
//! | [`roots`] | the read and write roots of automation paths (`mcp.md` §5) |
//! | `resources` | `resources/list` and `resources/read` (empty until M10.15) |
//! | [`connect`] | connect mode: the running app's control channel ([`Client`], the [`Connect`] backend, `--connect` [`connect::Options`]) |
//! | `tools` | the tool groups: `core` (the shared tools of `mcp.md` §4.1) and the later groups, empty until their tasks |
//!
//! Entry points: [`Server::serve`] (the stdio loop) and [`Server::handle_line`] (one message).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod backend;
pub mod connect;
pub mod headless;
mod jsonrpc;
mod progress;
mod resources;
pub mod roots;
pub mod server;
mod tools;
pub mod tray;

pub use backend::{Backend, NoProgress, Progress, Rendered, ToolError};
pub use connect::{CONNECT_NOT_BUILT, Client, Connect};
pub use headless::{Headless, OpenFailure, Source};
pub use jsonrpc::{MAX_LINE, MAX_RESULT};
pub use server::{PROTOCOL_VERSION, SUPPORTED_VERSIONS, Server};
pub use tools::{MAX_BATCH_STEPS, MAX_PNG_BYTES, MAX_PREVIEW_SIDE, MIN_PREVIEW_SIDE, ToolResult};

/// The desktop app's default control-channel address (`dalica --control 7990`; `mcp.md` §1).
pub const DEFAULT_ADDR: &str = "127.0.0.1:7990";
