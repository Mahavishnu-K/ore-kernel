# Extending Ore

> How to add new inference backends, model architectures, and firewall rules.

Ore is designed with extension points at every layer. This guide walks through the four most common contribution paths.

---

## 1. Adding a New Inference Driver

**Difficulty:** Medium · **Files:** `ore-core/src/inference/external/` + `ore-server/src/main.rs`

The Hardware Abstraction Layer (HAL) is built around the `InferenceDriver` trait. To add a new backend (vLLM, LM Studio, llamafile, etc.), implement this trait.

### The Trait

Source: [`ore-core/src/driver.rs`](../ore-core/src/driver.rs)

```rust
#[async_trait]
pub trait InferenceDriver: Send + Sync {
    fn engine_name(&self) -> &'static str;

    fn device_name(&self) -> String {               // optional, has a default
        "Default Device".to_string()
    }

    async fn is_online(&self) -> bool;

    async fn get_running_models(&self) -> Result<Vec<VramProcess>, DriverError>;

    async fn unload_model(&self, model: &str) -> Result<(), DriverError>;

    async fn preload_model(&self, model: &str) -> Result<(), DriverError>;

    async fn pull_model(&self, model_name: &str) -> Result<(), DriverError>;

    async fn list_local_models(&self) -> Result<Vec<LocalModel>, DriverError>;

    async fn generate_text(
        &self,
        model: &str,
        app_id: &str,
        stateful_paging: bool,
        prompt: &str,
        history: Option<Vec<ContextMessage>>,
        tx: UnboundedSender<String>,
        current_fingerprint: &str,
    ) -> Result<(), DriverError>;

    async fn generate_embeddings(
        &self,
        model: &str,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, DriverError>;

    async fn flush_idle_memory(&self, idle_timeout_mins: u64) -> Result<(), DriverError>;

    async fn invalidate_agent_cache(&self, app_id: &str) -> Result<(), DriverError>;

    async fn count_tokens(&self, _model: &str, _text: &str) -> Option<u32> {  // optional
        None
    }
}
```

See [Hardware Abstraction Layer](./kernel-internals/hardware-abstraction-layer.md) for what each method is for.

### Step-by-Step

1. **Create the file:**
   ```
   ore-core/src/inference/external/your_driver.rs
   ```

2. **Implement the trait:**

   ```rust
   use crate::driver::{DriverError, InferenceDriver, LocalModel, VramProcess};
   use crate::memory::ContextMessage;
   use async_trait::async_trait;
   use tokio::sync::mpsc::UnboundedSender;

   pub struct YourDriver {
       api_url: String,
   }

   impl YourDriver {
       pub fn new(api_url: &str) -> Self {
           Self { api_url: api_url.to_string() }
       }
   }

   #[async_trait]
   impl InferenceDriver for YourDriver {
       fn engine_name(&self) -> &'static str {
           "Your Engine"
       }

       async fn is_online(&self) -> bool {
           // Health check your backend
           todo!()
       }

       // ... implement the other 10 required methods.
       // Override `device_name` / `count_tokens` if your backend can answer them.
   }
   ```

3. **Register the module** in `ore-core/src/inference/external/mod.rs`:
   ```rust
   pub mod your_driver;
   ```

4. **Wire it into the boot sequence** in `ore-server/src/main.rs`. Today the kernel picks between two engines with an `if`; turn it into a `match`:
   ```rust
   let driver: Arc<dyn InferenceDriver> = match config.system.engine.as_str() {
       "native" => Arc::new(NativeDriver::new()),
       "ollama" => Arc::new(OllamaDriver::default()),
       "your_engine" => Arc::new(YourDriver::new("http://...")),
       other => panic!("Unknown engine: {}", other),
   };
   ```

5. **Select it in `ore.toml`:**
   ```toml
   [system]
   engine = "your_engine"
   ```

### Key Contracts

- `generate_text` must stream tokens through the `tx: UnboundedSender<String>` channel.
- If your backend can't persist KV caches, ignore `stateful_paging` and `current_fingerprint`; the kernel still keeps JSON chat history.
- `generate_embeddings` must return one `Vec<f32>` per input string.
- Use `DriverError` for all error reporting (not panics).
- The trait requires `Send + Sync`: the driver is shared across async tasks via `Arc`.

---

## 2. Adding a New Model Architecture (Native Engine)

**Difficulty:** Hard · **Files:** `ore-core/src/inference/native/models/` + `ore-core/src/inference/native/engine.rs`

The Native Candle engine routes each GGUF file to an architecture-specific loader. It currently ships with **Llama**, **Qwen2**, **Qwen3**, and **Qwen3-MoE** (which also loads `qwen2moe` files).

### Step-by-Step

1. **Create the model file:**
   ```
   ore-core/src/inference/native/models/your_arch.rs
   ```

   Follow the shape of [`llama.rs`](../ore-core/src/inference/native/models/llama.rs): a `ModelWeights` struct built from GGUF tensors, plus:
   - `forward(&mut self, x: &Tensor, index_pos: usize) -> Result<Tensor>`
   - KV-cache helpers used by paging: `clear_kv_cache`, `truncate_kv_cache`, `get_kv_cache_len`, `get_kv_cache`, `set_kv_cache`
   - a loader with the shared signature:
     ```rust
     pub fn load<R: Read + Seek>(
         model_name: &str,
         model_content: gguf_file::Content,
         reader: &mut R,
         device: &Device,
         tokenizer: &Tokenizer,
     ) -> anyhow::Result<(OreEngine, ModelConfig)>
     ```

   Shared building blocks live in `models/transformers/` (quantized layers, fused MoE) and `models/nn/kv_cache.rs`.

2. **Register the module** in `ore-core/src/inference/native/models/mod.rs`.

3. **Add a variant to `OreEngine`** in `ore-core/src/inference/native/engine.rs`, and add an arm for it to every `match self` in `impl OreEngine` (`forward`, `num_layers`, and the KV-cache methods):
   ```rust
   pub enum OreEngine {
       Qwen2(Qwen2Model),
       Llama(LlamaModel),
       Qwen3(Qwen3Model),
       Qwen3Moe(Qwen3MoeModel),
       YourArch(YourArchModel),   // new
   }
   ```

4. **Route the architecture** in the loader `match` in `engine.rs` (`OreEngine::load`):
   ```rust
   "your_arch" => models::your_arch::load(model_name, model_content, &mut cursor, device, &tokenizer)?,
   ```

### Architecture Detection

GGUF files contain a `general.architecture` metadata field (e.g. `"llama"`, `"qwen2"`, `"qwen3"`, `"qwen3moe"`). `OreEngine::load` reads it and dispatches to the matching loader; anything else fails with `Architecture not supported natively`. Your match string must be exactly what the GGUF converter writes into that field.

If the model has no local `tokenizer.json`, the tokenizer is extracted from the GGUF metadata. Check [`gguf_tokenizer.rs`](../ore-core/src/inference/native/gguf_tokenizer.rs) handles your architecture's pre-tokenizer.

---

## 3. Adding a DLP (Redaction) Rule

**Difficulty:** Easy · **File:** `ore-core/src/firewall.rs`

Redaction rules live in the `DLP_RULES` table. Each rule is a regex, a replacement string, and a category (`Pii`, `Financial`, `Credential`, `Network`). Regexes are compiled lazily once (`OnceLock`) and reused.

### Step-by-Step

1. **Add an entry** to `DLP_RULES` and **bump the array length** (`[DlpRule; 10]` → `[DlpRule; 11]`):
   ```rust
   DlpRule::new(
       r"\bxoxb-[0-9A-Za-z-]{20,}\b",
       "[SLACK TOKEN REDACTED]",
       DlpCategory::Credential,
   ),
   ```

2. That's it. `PiiRedactor::redact` applies every rule in order whenever the agent's manifest sets `privacy.enforce_pii_redaction = true`, and logs a `[FIREWALL: DLP_TRIGGER]` line per matching category.

### Guidelines

- Rules run in table order, so put more specific patterns before broader ones that might overlap.
- Prefer anchored, specific patterns: broad digit patterns cause false positives on IDs and timestamps.
- Rust's `regex` crate matches in linear time, so patterns can't cause ReDoS, but keep them simple to stay fast.

---

## 4. Adding a Prompt-Injection Rule

**Difficulty:** Easy · **File:** `ore-core/src/firewall.rs`

Injection heuristics live in the `THREAT_RULES` table. Each rule has a regex, a name, a severity, and a category that decides whether the agent's manifest can authorize it.

### Step-by-Step

Add an entry and **bump the array length** (`[ThreatRule; 7]` → `[ThreatRule; 8]`):

```rust
ThreatRule::new(
    r"(?i)(print|dump|reveal).{0,20}(api\s+key|access\s+token)",
    "Credential Exfiltration",
    ThreatSeverity::High,
    ThreatCategory::General,
),
```

### How `InjectionBlocker::check` treats a match

| Category | Bypassed when |
|---|---|
| `General` | Never |
| `CodeExecution` | `execution.can_execute_shell` or `execution.is_coding_agent` |
| `Sql` | `execution.is_coding_agent` |
| `NetworkProbe` | `network.network_enabled` |

If a match isn't bypassed, `High` and `Critical` rules block the request. `Medium` and `Low` rules only log, unless the agent has `can_execute_shell = true`, in which case any match blocks.

### Guidelines

- Pick the category carefully: `General` can't be overridden by any manifest.
- Test against legitimate prompts to avoid over-blocking (for example, an SQL tutor shouldn't trip the SQL rule unless it really is user input).
- These are heuristics, not a security boundary. See the [Security Model](./security-model.md).
- Open an Issue first if your rule is complex. We'll design it together.

---

## Contribution Workflow

1. **Fork** the repository
2. **Create a branch:** `git checkout -b feat/your-feature`
3. **Run checks** (the same ones CI runs):
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace -- -D warnings
   cargo test --workspace
   ```
4. **Commit:** `git commit -m 'feat: describe your change'`
5. **Push:** `git push origin feat/your-feature`
6. **Open a PR**. We review actively.

Read [CONTRIBUTING.md](../CONTRIBUTING.md) for the full code of conduct.

---

**← Back to:** [Documentation Index](./README.md)
