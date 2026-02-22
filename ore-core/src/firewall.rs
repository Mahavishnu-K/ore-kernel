use regex::Regex;
use serde::Deserialize;
use std::sync::OnceLock;
use thiserror::Error;

// =====================================================================
// 1. ERROR HANDLING (Production Grade)
// We don't use string errors. We use strict Enums so the Server knows
// exactly WHY a request was blocked and can return the right HTTP code.
// =====================================================================
#[derive(Error, Debug)]
pub enum FirewallError {
    #[error("Manifest Error: App '{0}' is not registered or missing manifest.")]
    UnregisteredApp(String),
    
    #[error("Permission Denied: App lacks '{0}' permission.")]
    UnauthorizedAction(String),
    
    #[error("SECURITY BREACH: Prompt injection detected. Rule triggered: {0}")]
    PromptInjection(String),
}

// =====================================================================
// 2. APP MANIFESTS (.toml)
// ORE reads this to know what an app is allowed to do.
// =====================================================================
#[derive(Deserialize, Debug, Clone)]
pub struct AppManifest {
    pub app_id: String,
    pub permissions: Permissions,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Permissions {
    pub can_read_files: bool,
    pub can_access_internet: bool,
}

impl AppManifest {
    /// In production, this reads from `/etc/ore/apps/app_id.toml`
    /// For now, we simulate a registered app.
    pub fn load(app_id: &str) -> Result<Self, FirewallError> {
        // Mocking a file read. Imagine this came from "openclaw.toml"
        if app_id == "openclaw" {
            Ok(AppManifest {
                app_id: "openclaw".to_string(),
                permissions: Permissions {
                    can_read_files: false, // OpenClaw is NOT allowed to read files!
                    can_access_internet: true,
                },
            })
        } else {
            Err(FirewallError::UnregisteredApp(app_id.to_string()))
        }
    }
}

// =====================================================================
// 3. PII REDACTION (High-Performance)
// We use `OnceLock` so the Regex engine only compiles these complex 
// patterns ONCE when the Kernel boots, not on every single request.
// =====================================================================
static EMAIL_REGEX: OnceLock<Regex> = OnceLock::new();
static CREDIT_CARD_REGEX: OnceLock<Regex> = OnceLock::new();

pub struct PiiRedactor;

impl PiiRedactor {
    pub fn redact(mut text: String) -> String {
        // Initialize Regexes safely
        let email_re = EMAIL_REGEX.get_or_init(|| {
            Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b").unwrap()
        });
        
        let cc_re = CREDIT_CARD_REGEX.get_or_init(|| {
            Regex::new(r"\b(?:\d[ -]*?){13,16}\b").unwrap()
        });

        // Apply Redactions
        text = email_re.replace_all(&text, "[EMAIL REDACTED]").to_string();
        text = cc_re.replace_all(&text, "[CREDIT CARD REDACTED]").to_string();
        
        text
    }
}

// =====================================================================
// 4. PROMPT INJECTION BLOCKING (Heuristics)
// =====================================================================
pub struct InjectionBlocker;

impl InjectionBlocker {
    const BLACKLIST: &'static [&'static str] = &[
        "ignore previous instructions",
        "forget everything",
        "system prompt",
        "you are now",
        "bypass security",
        "print your instructions"
    ];

    pub fn check(prompt: &str) -> Result<(), FirewallError> {
        let lower_prompt = prompt.to_lowercase();
        
        for &phrase in Self::BLACKLIST {
            if lower_prompt.contains(phrase) {
                return Err(FirewallError::PromptInjection(phrase.to_string()));
            }
        }
        Ok(())
    }
}

// =====================================================================
// 5. THE MASTER FIREWALL ENTRY POINT
// This is the function `ore-server` will call.
// =====================================================================
pub struct ContextFirewall;

impl ContextFirewall {
    /// Passes the prompt through all security layers.
    /// Returns the cleaned prompt, or a FirewallError if blocked.
    pub fn secure_request(app_id: &str, raw_prompt: &str) -> Result<String, FirewallError> {
        // 1. Check Identity & Manifest
        let _manifest = AppManifest::load(app_id)?;

        // 2. Check for Malicious Injections (Jailbreaks)
        InjectionBlocker::check(raw_prompt)?;

        // 3. Scrub Private Data (PII)
        let safe_prompt = PiiRedactor::redact(raw_prompt.to_string());

        // Return the scrubbed, safe prompt to the Kernel
        Ok(safe_prompt)
    }
}