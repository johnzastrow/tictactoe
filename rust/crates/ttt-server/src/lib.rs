//! Game hub plus a localhost-only web server (static page + WebSocket).
//! See `docs/protocol.md` for the wire format. The web server is behind the
//! default `web` feature; without it only the hub is built.

mod hub;
#[cfg(feature = "web")]
mod web;

pub use hub::{HubError, HubHandle, Score, StateView, spawn_hub};
#[cfg(feature = "web")]
pub use web::{ClientMsg, ServerMsg, serve};
