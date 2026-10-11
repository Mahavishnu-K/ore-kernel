//! Global constants shared across ORE kernel, server, and CLI tools.

// File & Directory Names
pub const TOKEN_FILE_NAME: &str = "ore-kernel.token";
pub const CONFIG_FILE_NAME: &str = "ore.toml";
pub const MODELS_DIR_NAME: &str = "models";
pub const MANIFESTS_DIR_NAME: &str = "manifests";
pub const SANDBOX_DIR_NAME: &str = "sandbox";
pub const MEMORY_DIR_NAME: &str = "memory";
pub const TOOLS_DIR_NAME: &str = "tools";
pub const RUNTIMES_DIR_NAME: &str = "runtimes";
pub const PLUGINS_DIR_NAME: &str = "plugins";
pub const CACHE_DIR_NAME: &str = "cache";
pub const TMP_DIR_NAME: &str = "tmp";

// Standard Model Artifact Filenames
pub const MODEL_GGUF_FILE_NAME: &str = "model.gguf";
pub const MODEL_SAFETENSORS_FILE_NAME: &str = "model.safetensors";
pub const TOKENIZER_JSON_FILE_NAME: &str = "tokenizer.json";
pub const CONFIG_JSON_FILE_NAME: &str = "config.json";

// Server & Network Defaults
pub const DEFAULT_SERVER_HOST: &str = "127.0.0.1";
pub const DEFAULT_SERVER_PORT: u16 = 6767;
pub const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:6767";

pub const DEFAULT_OLLAMA_HOST: &str = "127.0.0.1";
pub const DEFAULT_OLLAMA_PORT: u16 = 11434;
pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";

// Execution, Resource & Safety Limits
pub const DEFAULT_FUEL_LIMIT: u64 = 5_000_000_000;
pub const DEFAULT_EMBEDDER: &str = "all-minilm";
pub const DEFAULT_MAX_TOKENS_PER_MINUTE: u32 = 10_000;
pub const DEFAULT_MAX_JSON_TOKENS: u32 = 8192;
pub const DEFAULT_MAX_KV_CACHE_MB: u32 = 1024;
