# Context Firewall

> Every prompt passes through this 3-stage pipeline before reaching the model.

**Source:** [`ore-core/src/firewall.rs`](../../ore-core/src/firewall.rs)

---

## Overview

The `ContextFirewall` is the security entry point for all inference requests. It takes a raw user prompt and an `AppManifest`, then runs three sequential transformations:

```
Raw Prompt → InjectionBlocker → PiiRedactor → Secured Prompt
```

If any check fails, the request is rejected before it reaches the GPU.

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

The Injection Blocker uses a severity-based and category-based rule engine.

```rust
#[derive(Debug, Clone, Copy)]
pub enum ThreatSeverity { Low, Medium, High, Critical }

#[derive(Debug, PartialEq)]
pub enum ThreatCategory {
    General,         // Jailbreaks, Context Escapes (Always enforced)
    CodeExecution,   // Python os.system, Bash, Eval (Bypassed if shell allowed)
    NetworkProbe,    // cURL, wget (Bypassed if network allowed)
}

pub struct InjectionBlocker;

impl InjectionBlocker {
    pub fn check(prompt: &str, manifest: &AppManifest) -> Result<(), FirewallError> {
        for rule in &THREAT_RULES {
            if rule.regex().is_match(prompt) {
                let is_authorized = match rule.category {
                    ThreatCategory::CodeExecution => manifest.execution.can_execute_shell,
                    ThreatCategory::NetworkProbe => manifest.network.network_enabled,
                    ThreatCategory::General => false, 
                };

                if is_authorized { continue; } // Authorized bypass
                
                // For Critical/High threats OR if the agent has Shell Execution powers, we block instantly.
                if matches!(rule.severity, ThreatSeverity::Critical | ThreatSeverity::High) 
                   || manifest.execution.can_execute_shell 
                {
                    return Err(FirewallError::PromptInjection { /* ... */ });
                }
            }
        }
        Ok(())
    }
}
```

### Design Decisions

- **Dynamic Authorization Bypass** - Certain threats like code execution or network probing might be legitimate behaviors if the agent's manifest explicitly grants them. If `can_execute_shell` or `network_enabled` are true, these specific threat categories are safely bypassed.
- **Strict Shell Restrictions** - If an agent *does* have shell execution powers (`can_execute_shell`), the firewall instantly upgrades all rule severities to block. An agent with shell access cannot be allowed to bypass any general threat.

---

## Stage 2: PII Redactor

```rust
#[derive(Debug)]
pub enum DlpCategory { Credential, Network, General }

pub struct PiiRedactor;

impl PiiRedactor {
    pub fn redact(mut text: String, manifest: &AppManifest) -> String {
        if !manifest.privacy.enforce_pii_redaction {
            return text; 
        }

        for rule in &DLP_RULES {
            let re = rule.regex();
            if re.is_match(&text) {
                // SIEM-Friendly Telemetry Logging
                text = re.replace_all(&text, rule.replacement).to_string();
            }
        }
        text
    }
}
```

### Design Decisions

- **Manifest Opt-In/Opt-Out** - Redaction only runs if `manifest.privacy.enforce_pii_redaction` is true. Some internal agents may need to process sensitive logs safely.
- **`OnceLock` caching** - Regex compilation is expensive. `OnceLock` compiles each pattern exactly once across all threads.
- **DLP Categories** - Replaces sensitive data like AWS Keys, RSA Private Keys, and Internal IPs with explicit tags (e.g., `[AWS KEY REDACTED]`).

---



---

## Error Types

```rust
#[derive(Error, Debug)]
pub enum FirewallError {
    #[error("Manifest Error: App '{0}' is not registered.")]
    UnregisteredApp(String),

    #[error("Manifest Error: Failed to parse manifest TOML. {0}")]
    CorruptManifest(String),

    #[error("Permission Denied: App lacks '{0}' permission.")]
    UnauthorizedAction(String),

    #[error("SECURITY BREACH: Prompt injection detected. Threat: {threat_name} | App: {app_id}")]
    PromptInjection {
        severity: ThreatSeverity,
        threat_name: String,
        app_id: String,
    },
}
```

Only `PromptInjection` is currently raised by the firewall pipeline. The other variants are defined for future manifest-level enforcement (e.g., rejecting requests from apps lacking specific permissions).

---

**← Back to:** [Kernel Internals Index](./README.md)
