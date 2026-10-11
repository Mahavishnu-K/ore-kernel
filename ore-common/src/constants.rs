//! Global constants shared across ORE kernel, server, and CLI tools.

// ─── File & Directory Names ──────────────────────────────────────────────────
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

// ─── Standard Model Artifact Filenames ───────────────────────────────────────
pub const MODEL_GGUF_FILE_NAME: &str = "model.gguf";
pub const MODEL_SAFETENSORS_FILE_NAME: &str = "model.safetensors";
pub const TOKENIZER_JSON_FILE_NAME: &str = "tokenizer.json";
pub const CONFIG_JSON_FILE_NAME: &str = "config.json";

// ─── Server & Network Defaults ───────────────────────────────────────────────
pub const DEFAULT_SERVER_HOST: &str = "127.0.0.1";
pub const DEFAULT_SERVER_PORT: u16 = 6767;
pub const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:6767";

pub const DEFAULT_OLLAMA_HOST: &str = "127.0.0.1";
pub const DEFAULT_OLLAMA_PORT: u16 = 11434;
pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";

// ─── Execution, Resource & Safety Limits ─────────────────────────────────────
/// Default WASM execution CPU fuel instruction limit (5 Billion ~= 2-3s of compute).
pub const DEFAULT_FUEL_LIMIT: u64 = 5_000_000_000;

/// Default embedding model identifier.
pub const DEFAULT_EMBEDDER: &str = "all-minilm";

/// Default idle timeout for WASM instances in minutes.
pub const DEFAULT_WASM_IDLE_TIMEOUT_MINS: u64 = 5;

/// Default token bucket rate limit per minute for agents.
pub const DEFAULT_MAX_TOKENS_PER_MINUTE: u32 = 10_000;

/// Default conversational JSON token limit.
pub const DEFAULT_MAX_JSON_TOKENS: u32 = 8192;

/// Default physical KV-cache size limit in megabytes.
pub const DEFAULT_MAX_KV_CACHE_MB: u32 = 1024;
