//! Connect mode (`mcp.md` §1): `dalica-cli mcp --connect [HOST:PORT]` drives a running app through
//! its loopback control channel (`control-protocol.md`). Not built until M10.17, which ports
//! LightCraft's `Remote` here; until then the CLI refuses `--connect` with this message.

/// Why `--connect` is refused today.
pub const CONNECT_NOT_BUILT: &str =
    "connect mode (--connect) is not built yet: it arrives with M10.17. Run `dalica-cli mcp --demo` or `--library DIR` for a headless session";
