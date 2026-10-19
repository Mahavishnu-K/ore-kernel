# Context Firewall

> Every inference prompt passes through this 2-stage pipeline before reaching the model.

**Source:** [`ore-core/src/firewall.rs`](../../ore-core/src/firewall.rs)

---

## Overview

The `ContextFirewall` is the security entry point for inference requests. It takes a raw user prompt and the caller's `AppManifest`, then runs two sequential stages:

```
Raw Prompt → InjectionBlocker → PiiRedactor → Secured Prompt
```

If the injection check fails, the request is rejected before it reaches the GPU.

> [!IMPORTANT]
> Both stages are **regex heuristics**. They catch common, literal patterns. They don't understand paraphrases, and some rules can match legitimate text. Treat the firewall as a filter, not a security boundary: the real boundaries are the manifest permissions and the WASM sandbox. See the [Security Model](../security-model.md).

---

## Entry Point

```rust
pub struct ContextFirewall;

impl ContextFirewall {
    pub fn secure_request(
        manifest: &AppManifest,
        raw_prompt: &str,
    ) -> Result<(String, String), FirewallError> {
        InjectionBlocker::check(raw_prompt, manifest)?;

        let safe_text = PiiRedactor::redact(raw_prompt.to_string(), manifest);

        Ok((safe_text.clone(), safe_text))
    }
}
```

---

## Stage 1: Injection Blocker

The Injection Blocker matches the prompt against `THREAT_RULES`. Each rule has a severity and a category.

```rust
pub enum ThreatSeverity { Low, Medium, High, Critical }

pub enum ThreatCategory {
    General,        // Jailbreaks, context escapes (always enforced)
    CodeExecution,  // os.system, subprocess, eval/exec
    Sql,            // UNION SELECT, DROP TABLE, OR 1=1
    NetworkProbe,   // curl/wget/requests.get against a URL
}
```

### The Rules

| Threat | Pattern (case-insensitive) | Severity | Category |
|---|---|---|---|
| Jailbreak (Ignore Previous) | `ignore`/`disregard` + optional `all` + `previous`/`prior` + `instructions`/`prompts`/`rules` | High | General |
| Persona Hijack (Roleplay) | `you are now` + `unbound`/`free`/`god`/`DAN`/`admin` | Medium | General |
| System Probe | `what`/`show`/`print`/`reveal`/`tell` within 30 chars of `system prompt`/`root password`/`hidden instructions`/`core directive` | High | General |
| SQL Injection Attempt | `UNION SELECT`, `DROP TABLE`, `OR 1=1` | Critical | Sql |
| Boundary Breakout Attempt | `]]] }}} <\|im_end\|>` | Critical | General |
| Code Execution Payload | `os.system`, `subprocess.Popen`, `eval(`, `exec(` | Critical | CodeExecution |
| Network Probe | `curl`/`wget`/`requests.get` followed by `http` | High | NetworkProbe |

### Decision Logic

```rust
let is_authorized = match rule.category {
    ThreatCategory::CodeExecution => {
        manifest.execution.can_execute_shell || manifest.execution.is_coding_agent
    }
    ThreatCategory::Sql => manifest.execution.is_coding_agent,
    ThreatCategory::NetworkProbe => manifest.network.network_enabled,
    ThreatCategory::General => false,
};

if is_authorized { continue; } // logged as [FIREWALL: BYPASS]

// logged as [FIREWALL: THREAT_DETECTED]
if matches!(rule.severity, ThreatSeverity::Critical | ThreatSeverity::High)
    || manifest.execution.can_execute_shell
{
    return Err(FirewallError::PromptInjection { severity, threat_name, app_id });
}
```

| Category | Bypassed when |
|---|---|
| `General` | Never |
| `CodeExecution` | `execution.can_execute_shell = true` or `execution.is_coding_agent = true` |
| `Sql` | `execution.is_coding_agent = true` |
| `NetworkProbe` | `network.network_enabled = true` |

### Design Decisions

- **Manifest-aware bypass.** Code, SQL, and network keywords are normal content for some agents. A coding agent discussing `DROP TABLE` or `eval(` sets `is_coding_agent = true`; an agent with network access may legitimately mention `curl https://…`.
- **Severity decides blocking.** `High` and `Critical` matches block. `Medium` and `Low` matches are logged but allowed.
- **Shell agents get zero tolerance.** If `can_execute_shell = true`, any unauthorized match blocks, whatever its severity, because a successful injection could reach the host shell.
- **Structured logs.** Every match logs `[FIREWALL: THREAT_DETECTED] App: … | Threat: … | Severity: …` (or `[FIREWALL: BYPASS]`), ready for SIEM ingestion.

---

## Stage 2: PII Redactor (DLP)

```rust
pub enum DlpCategory { Pii, Financial, Credential, Network }

impl PiiRedactor {
    pub fn redact(mut text: String, manifest: &AppManifest) -> String {
        if !manifest.privacy.enforce_pii_redaction {
            return text;
        }
        for rule in &DLP_RULES {
            let re = rule.regex();
            if re.is_match(&text) {
                // logs [FIREWALL: DLP_TRIGGER] Category: … | App: … | Action: Redacted
                text = re.replace_all(&text, rule.replacement).to_string();
            }
        }
        // logs [FIREWALL: DLP_SUMMARY] when anything was scrubbed
        text
    }
}
```

### The Rules (applied in this order)

| Replacement | Matches | Category |
|---|---|---|
| `[EMAIL REDACTED]` | Email addresses | Pii |
| `[SSN REDACTED]` | `ddd-dd-dddd`, also with `.` or no separators | Pii |
| `[PHONE REDACTED]` | US-style numbers, optional `+1` and parentheses | Pii |
| `[CREDIT CARD REDACTED]` | Visa, Mastercard, Amex, Discover number formats | Financial |
| `[IBAN REDACTED]` | Two letters, two digits, then 11–30 alphanumerics | Financial |
| `[GITHUB TOKEN REDACTED]` | `ghp_` / `gho_` / `ghu_` / `ghs_` / `ghr_` tokens | Credential |
| `[AWS KEY REDACTED]` | `AKIA` access key IDs | Credential |
| `[API SECRET REDACTED]` | `sk-` keys (32+ chars) | Credential |
| `[RSA/PEM PRIVATE KEY REDACTED]` | `-----BEGIN … PRIVATE KEY-----` blocks | Credential |
| `[INTERNAL IP REDACTED]` | `10.x.x.x`, `192.168.x.x`, `172.16–31.x.x` | Network |

### Design Decisions

- **Manifest opt-in.** Redaction only runs if `privacy.enforce_pii_redaction = true`. Some internal agents need to process sensitive logs.
- **`OnceLock` caching.** Each pattern is compiled once, on first use, and shared across threads.
- **No ReDoS.** Rust's `regex` crate guarantees linear-time matching.
- **Known false positives.** The SSN rule matches any 9-digit number (including bare `ddddddddd`), and the phone rule matches many 10-digit numbers. Order IDs and timestamps can be redacted.

---

## Error Types

```rust
#[derive(Error, Debug)]
pub enum FirewallError {
    #[error("Manifest Error: App '{0}' is not registered. Could not find manifest file.")]
    UnregisteredApp(String),

    #[error("Manifest Error: Failed to parse manifest TOML. {0}")]
    CorruptManifest(String),

    #[error("Permission Denied: App lacks '{0}' permission.")]
    UnauthorizedAction(String),

    #[error("SECURITY BREACH [Severity: {severity:?}]: {threat_name}. Target Agent: {app_id}")]
    PromptInjection {
        severity: ThreatSeverity,
        threat_name: String,
        app_id: String,
    },
}
```

Only `PromptInjection` is raised by the firewall pipeline. The other variants are defined for manifest-level enforcement.

---

## Extending

To add a rule, see [Extending Ore → Adding a DLP Rule](../extending-ore.md#3-adding-a-dlp-redaction-rule) and [Adding a Prompt-Injection Rule](../extending-ore.md#4-adding-a-prompt-injection-rule).

---

**← Back to:** [Kernel Internals Index](./README.md)
