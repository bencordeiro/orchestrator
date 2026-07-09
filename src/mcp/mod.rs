//! MCP streamable-HTTP server exposing `delegate` and `list_slots`.

pub mod server;

pub use server::{build_router, load_bearer_token, McpState, DEFAULT_LISTEN};
