//! pt core: everything the server, the TUI and the WASM web client share.
//! Pure: no I/O, no clock, no randomness (callers pass "today" in), so it
//! compiles for wasm32 and every test is deterministic.

pub mod date;
pub mod fields;
pub mod fuzzy;
pub mod grid;
pub mod model;
pub mod protocol;
pub mod query;
pub mod replica;

pub use date::Date;
pub use model::*;
pub use protocol::{ClientMsg, Op, ServerMsg};
pub use replica::Replica;

/// Version reported in `/api/health` and `welcome`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
