//! demo core: everything the demo server, the TUI and the WASM web client
//! share (docs/demo/protocol.md). Pure: no I/O, no clock, no randomness
//! (callers pass "today" and "now" in), so it compiles for wasm32 and every
//! test is deterministic.
//!
//! - [`keys`]: the 26 control keys, defaults, validation and normalisation
//! - [`dataset`]: people, programs ▸ projects ▸ phases, tags, choice lists, tabs, peer colours
//! - [`date`]: calendar dates and natural-date parsing
//! - [`fuzzy`]: subsequence matching with highlight positions
//! - [`protocol`]: wire messages and transport limits
//! - [`values`]: the client-side value store

pub mod dataset;
pub mod date;
pub mod fuzzy;
pub mod keys;
pub mod protocol;
pub mod values;

pub use date::Date;
pub use keys::{KEYS, SetError, safe_multiline, safe_text};
pub use protocol::{ClientMsg, Entry, ServerMsg};
pub use values::{Event, Values};

/// Version reported in `/api/health` and `welcome` (the demo crates' own version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
