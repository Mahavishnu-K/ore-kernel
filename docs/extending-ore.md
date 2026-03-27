# Extending ORE

> How to add new inference backends, model architectures, and firewall rules.

ORE is designed with extension points at every layer. This guide walks through the three most common contribution paths.

---

## 1. Adding a New Inference Driver

**Difficulty:** Medium · **Files:** `ore-core/src/external/` + `ore-core/src/driver.rs`

The Hardware Abstraction Layer (HAL) is built around the `InferenceDriver` trait. To add a new backend (vLLM, LM Studio, llamafile, etc.), implement this trait.

### The Trait

Source: [`ore-core/src/driver.rs`](../ore-core/src/driver.rs)

```rust
#[async_trait]
pub trait InferenceDriver: Send + Sync {
    fn engine_name(&self) -> &'static str;

    async fn is_online(&self) -> bool;

    async fn get_running_models(&self) -> Result<Vec<VramProcess>, DriverError>;

    async fn unload_model(&self, model: &str) -> Result<(), DriverError>;

    async fn preload_model(&self, model: &str) -> Result<(), DriverError>;

    async fn pull_model(&self, model_name: &str) -> Result<(), DriverError>;

    async fn list_local_models(&self) -> Result<Vec<LocalModel>, DriverError>;

    async fn generate_text(
        &self,
        model: &str,
        prompt: &str,
        history: Option<Vec<ContextMessage>>,
        tx: UnboundedSender<String>,
    ) -> Result<(), DriverError>;

    async fn generate_embeddings(
        &self,
        model: &str,
        inputs: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, DriverError>;
}
```

### Step-by-Step

1. **Create the file:**
   ```
   ore-core/src/external/your_driver.rs
   ```

2. **Implement the trait:**

   ```rust
   use crate::driver::{DriverError, InferenceDriver, LocalModel, VramProcess};
   use crate::swap::ContextMessage;
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

       // ... implement all 9 methods
   }
   ```

3. **Register the module** in `ore-core/src/external/mod.rs`:
   ```rust
   pub mod your_driver;
   ```

4. **Wire it into the boot sequence** in `ore-server/src/main.rs`:
   ```rust
   let driver: Arc<dyn InferenceDriver> = match config.system.engine.as_str() {
       "native" => Arc::new(NativeDriver::new()),
       "ollama" => Arc::new(OllamaDriver::new("http://127.0.0.1:11434")),
       "your_engine" => Arc::new(YourDriver::new("http://...")),
       _ => panic!("Unknown engine"),
   };
   ```

5. **Update `ore.toml`** to accept the new engine name:
   ```toml
   [system]
   engine = "your_engine"
   ```

### Key Contracts

- `generate_text` must stream tokens through the `tx: UnboundedSender<String>` channel
- `generate_embeddings` must return one `Vec<f32>` per input string
- Use `DriverError` for all error reporting (not panics)
- The trait requires `Send + Sync` — the driver is shared across async tasks via `Arc`

---

## 2. Adding a New Model Architecture (Native Engine)

**Difficulty:** Hard · **Files:** `ore-core/src/native/models/` + `ore-core/src/native/engine.rs`

The Native Candle engine supports architecture-specific model loaders. Currently ships with Llama and Qwen2.

### Step-by-Step

1. **Create the model loader:**
   ```
   ore-core/src/native/models/your_arch.rs
   ```

   Your loader must implement the loading and forward-pass logic using `candle-core` and `candle-transformers`. Study the existing `llama.rs` and `qwen.rs` files for the pattern.

2. **Add to the `OreEngine` enum** in `ore-core/src/native/engine.rs`:
   ```rust
   pub enum OreEngine {
       Llama(/* ... */),
       Qwen(/* ... */),
       YourArch(/* ... */),   // Add your variant
   }
   ```

3. **Update architecture detection** in `ore-core/src/native/mod.rs`:

   The `NativeDriver` reads the GGUF file's `general.architecture` metadata field to determine which loader to use. Add your architecture to the match statement.

4. **Register the module** in `ore-core/src/native/models/mod.rs`.

### Architecture Detection

GGUF files contain an `general.architecture` metadata field (e.g., `"llama"`, `"qwen2"`). The `NativeDriver` reads this field and routes to the appropriate model loader. Your architecture string must match what HuggingFace encodes in the GGUF metadata.

---

## 3. Adding New PII Patterns

**Difficulty:** Easy · **File:** `ore-core/src/firewall.rs`

The PII redactor uses `OnceLock`-cached compiled regex patterns. Adding a new pattern is straightforward.

### Step-by-Step

1. **Add a static regex** at file scope:
   ```rust
   static PHONE_REGEX: OnceLock<Regex> = OnceLock::new();
   ```

2. **Initialize and apply** in `PiiRedactor::redact()`:
   ```rust
   let phone_re = PHONE_REGEX.get_or_init(|| {
       Regex::new(r"\b\d{3}[-.]?\d{3}[-.]?\d{4}\b").unwrap()
   });

   text = phone_re.replace_all(&text, "[PHONE REDACTED]").to_string();
   ```

### Guidelines

- Use `OnceLock` — the regex is compiled once and reused forever
- Place the `static` declaration next to the existing `EMAIL_REGEX` and `CREDIT_CARD_REGEX`
- Test edge cases: international formats, separators, partial matches

---

## 4. Adding Injection Detection Rules

**Difficulty:** Easy · **File:** `ore-core/src/firewall.rs`

The `InjectionBlocker` uses heuristic pattern matching on lowercased prompts. Add new rules to `InjectionBlocker::check()`:

```rust
pub fn check(prompt: &str) -> Result<(), FirewallError> {
    let lower = prompt.to_lowercase();

    // Existing rules
    let is_jailbreak = lower.contains("ignore") && lower.contains("previous");
    let is_system_probe = lower.contains("system prompt") || lower.contains("root password");
    let is_override = lower.contains("bypass") || lower.contains("forget everything");

    // Your new rule
    let is_data_theft = lower.contains("print") && lower.contains("api key");

    if is_jailbreak || is_system_probe || is_override || is_data_theft {
        return Err(FirewallError::PromptInjection(
            "Heuristic rule triggered".to_string(),
        ));
    }

    Ok(())
}
```

### Guidelines

- Combine multiple `contains()` checks to reduce false positives (e.g., `"ignore" + "previous"` instead of just `"ignore"`)
- Test against legitimate user prompts to avoid over-blocking
- Open an Issue first if your rule is complex — we'll design it together

---

## Contribution Workflow

1. **Fork** the repository
2. **Create a branch:** `git checkout -b feat/your-feature`
3. **Run checks:**
   ```bash
   cargo fmt        # Format
   cargo clippy     # Lint
   cargo test       # Test
   ```
4. **Commit:** `git commit -m 'feat: describe your change'`
5. **Push:** `git push origin feat/your-feature`
6. **Open a PR** — we review actively

Read [CONTRIBUTING.md](../CONTRIBUTING.md) for the full code of conduct.

---

**← Back to:** [Documentation Index](./README.md)
