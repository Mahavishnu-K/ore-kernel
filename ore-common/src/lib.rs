//! # ORE Common (`ore-common`)
//!
//! Shared foundation library for the ORE AI Kernel ecosystem.
//! Provides unified protocol DTOs, canonical path resolvers, global constants,
//! and terminal formatting utilities shared across `ore-core`, `ore-server`, and `ore-cli`.

pub mod constants;
pub mod paths;
pub mod protocol;
pub mod text;

// Re-export common symbols at the crate root for ergonomics
pub use constants::*;
pub use paths::*;
pub use protocol::*;
pub use text::*;
