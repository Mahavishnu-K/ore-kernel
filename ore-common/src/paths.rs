//! Canonical path resolvers for ORE base directory, configuration, and data folders.

use std::path::PathBuf;
use crate::constants::{
    CONFIG_FILE_NAME, MANIFESTS_DIR_NAME, MODELS_DIR_NAME, SANDBOX_DIR_NAME, TOKEN_FILE_NAME,
};

/// Resolves the canonical ORE directory across Windows, Linux, and macOS.
///
/// Priority:
/// 1. `ORE_DIR` environment variable (if set)
/// 2. Current working directory if `ore.toml` exists
/// 3. Parent directory if `ore.toml` exists
/// 4. User profile home `.ore` directory (`~/.ore` or `%USERPROFILE%\.ore`)
pub fn get_ore_dir() -> PathBuf {
    if let Ok(custom_dir) = std::env::var("ORE_DIR") {
        return PathBuf::from(custom_dir);
    }

    let cur_path = PathBuf::from(".");
    if cur_path.join(CONFIG_FILE_NAME).exists() {
        return cur_path;
    }

    let local_dev_path = PathBuf::from("..");
    if local_dev_path.join(CONFIG_FILE_NAME).exists() {
        return local_dev_path;
    }

    let home = std::env::var("USERPROFILE") // Windows
        .or_else(|_| std::env::var("HOME")) // Linux / macOS
        .expect("FATAL: Could not determine user home directory.");

    let ore_path = PathBuf::from(home).join(".ore");

    if !ore_path.exists() {
        std::fs::create_dir_all(&ore_path).expect("FATAL: Failed to create ~/.ore directory.");
    }

    ore_path
}

/// Resolves the path to the kernel authentication token file.
pub fn get_token_path() -> PathBuf {
    get_ore_dir().join(TOKEN_FILE_NAME)
}

/// Resolves the path to the local model weights directory.
pub fn get_models_dir() -> PathBuf {
    let dir = get_ore_dir().join(MODELS_DIR_NAME);
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir
}

/// Resolves the path to the agent manifests directory.
pub fn get_manifests_dir() -> PathBuf {
    let dir = get_ore_dir().join(MANIFESTS_DIR_NAME);
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir
}

/// Resolves the path to the WASM isolated sandbox directory.
pub fn get_sandbox_dir() -> PathBuf {
    let dir = get_ore_dir().join(SANDBOX_DIR_NAME);
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir
}
