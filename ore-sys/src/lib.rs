// ore-rs/src/lib.rs

// If they are compiling a plugin, we MUST strip the standard library!
#![cfg_attr(feature = "plugin", no_std)]

#[cfg(feature = "host")]
pub mod host;

#[cfg(feature = "plugin")]
pub mod plugin;
