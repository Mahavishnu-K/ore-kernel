//! ORE Common (`ore-common`)
//!
//! Shared foundation library for the ORE AI Kernel ecosystem.
//! Provides unified protocol DTOs, domain models, manifest schemas, configuration schemas,
//! canonical path resolvers, global constants, and terminal formatting utilities shared
//! across `ore-core`, `ore-server`, and `ore-cli`.

pub mod config;
pub mod constants;
pub mod manifest;
pub mod paths;
pub mod protocol;
pub mod text;
pub mod types;

// Re-export common symbols at the crate root for ergonomics
pub use config::*;
pub use constants::*;
pub use manifest::*;
pub use paths::*;
pub use protocol::*;
pub use text::*;
pub use types::*;
