//! Game hub plus a localhost-only web server (static page + WebSocket).
//! See `docs/protocol.md` for the wire format.

mod hub;
mod web;

pub use hub::{HubError, HubHandle, Score, StateView, spawn_hub};
pub use web::{ClientMsg, ServerMsg, serve};
