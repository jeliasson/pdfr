//! Agent control: AI agents (Claude Code, Claude Desktop, Codex, …) drive a
//! pdfr window the way they drive a browser.
//!
//! ```text
//! agent ──stdio (MCP)──> `pdfr mcp` ──unix socket──> a pdfr window
//! ```
//!
//! - [`mcp`]: `pdfr mcp`, a Model Context Protocol server on stdin/stdout.
//!   It never opens a window of its own; it finds or starts pdfr windows.
//! - [`bridge`]: where the sockets live, and the client side of the line
//!   protocol between `pdfr mcp` and a window.
//! - [`server`]: the window's side: a listener thread that hands each
//!   request to the UI thread.
//! - [`shot`]: screenshot scaling and encoding.
//!
//! What a request does to the window is in `app/agent.rs`.

pub mod bridge;
pub mod mcp;
pub mod server;
pub mod shot;

use std::sync::atomic::{AtomicUsize, Ordering};

/// Agents connected to this window right now.
static CLIENTS: AtomicUsize = AtomicUsize::new(0);

/// Whether an agent is connected to this window.
pub fn active() -> bool {
    CLIENTS.load(Ordering::SeqCst) > 0
}

#[cfg(unix)]
pub(crate) fn client_joined() {
    CLIENTS.fetch_add(1, Ordering::SeqCst);
}

#[cfg(unix)]
pub(crate) fn client_left() {
    CLIENTS.fetch_sub(1, Ordering::SeqCst);
}
