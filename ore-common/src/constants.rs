//! Global constants shared across ORE kernel, server, and CLI tools.

pub const TOKEN_FILE_NAME: &str = "ore-kernel.token";
pub const CONFIG_FILE_NAME: &str = "ore.toml";
pub const MODELS_DIR_NAME: &str = "models";
pub const MANIFESTS_DIR_NAME: &str = "manifests";
pub const SANDBOX_DIR_NAME: &str = "sandbox";

pub const DEFAULT_SERVER_HOST: &str = "127.0.0.1";
pub const DEFAULT_SERVER_PORT: u16 = 8080;
pub const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:8080";
