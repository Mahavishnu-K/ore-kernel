use crate::registry::AppManifest;
use regex::Regex;
use std::sync::OnceLock;
use thiserror::Error;

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

// DLP engine: Detects and redacts sensitive information from prompts before they reach the LLM.
#[derive(Debug)]
pub enum DlpCategory {
    Pii,
    Financial,
    Credential,
    Network,
}

struct DlpRule {
    pattern: OnceLock<Regex>,
    regex_str: &'static str,
    replacement: &'static str,
    category: DlpCategory,
}

impl DlpRule {
    const fn new(regex_str: &'static str, replacement: &'static str, category: DlpCategory) -> Self {
        Self {
            pattern: OnceLock::new(),
            regex_str,
            replacement,
            category,
        }
    }

    fn regex(&self) -> &Regex {
        self.pattern.get_or_init(|| Regex::new(self.regex_str).expect("Invalid DLP regex pattern"))
    }
}

// DLP Registry
// Rust's regex crate guarantees linear time matching (O(m*n)), meaning ORE is immune to ReDoS attacks.
static DLP_RULES: [DlpRule; 10] = [
    // --- PII (GDPR / HIPAA) ---
    DlpRule::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b", "[EMAIL REDACTED]", DlpCategory::Pii),
    DlpRule::new(r"\b\d{3}[-.]?\d{2}[-.]?\d{4}\b", "[SSN REDACTED]", DlpCategory::Pii),
    DlpRule::new(r"\b(?:\+?1[-.]?)?\(?\d{3}\)?[-.]?\d{3}[-.]?\d{4}\b", "[PHONE REDACTED]", DlpCategory::Pii),

    // --- FINANCIAL (PCI-DSS) ---
    DlpRule::new(r"\b(?:\d[ -]*?){13,16}\b", "[CREDIT CARD REDACTED]", DlpCategory::Financial),
    DlpRule::new(r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b", "[IBAN REDACTED]", DlpCategory::Financial), // European Bank Accounts

    // --- CREDENTIALS & SECRETS (Zero-Trust) ---
    DlpRule::new(r"(?i)(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36}", "[GITHUB TOKEN REDACTED]", DlpCategory::Credential),
    DlpRule::new(r"(?i)AKIA[0-9A-Z]{16}", "[AWS KEY REDACTED]", DlpCategory::Credential),
    DlpRule::new(r"(?i)sk-[a-zA-Z0-9]{32,}", "[API SECRET REDACTED]", DlpCategory::Credential),
    DlpRule::new(r"-----BEGIN [A-Z ]+ PRIVATE KEY-----[\s\S]*?-----END [A-Z ]+ PRIVATE KEY-----", "[RSA/PEM PRIVATE KEY REDACTED]", DlpCategory::Credential),

    // --- NETWORK TOPOLOGY ---
    DlpRule::new(r"\b(?:10\.|192\.168\.|172\.(?:1[6-9]|2[0-9]|3[0-1])\.)[0-9]{1,3}\.[0-9]{1,3}\b", "[INTERNAL IP REDACTED]", DlpCategory::Network),
];

pub struct PiiRedactor;

impl PiiRedactor {
    pub fn redact(mut text: String, manifest: &AppManifest) -> String {
        if !manifest.privacy.enforce_pii_redaction {
            return text; 
        }

        let mut redaction_count = 0;

        for rule in &DLP_RULES {
            let re = rule.regex();
            if re.is_match(&text) {
                redaction_count += 1;
                // SIEM-Friendly Telemetry: Datadog/Splunk can parse this immediately
                crate::kprintln!(
                    "-> [FIREWALL: DLP_TRIGGER] Category: {:?} | App: {} | Action: Redacted", 
                    rule.category, manifest.app_id
                );
                text = re.replace_all(&text, rule.replacement).to_string();
            }
        }

        if redaction_count > 0 {
            crate::kprintln!("-> [FIREWALL: DLP_SUMMARY] Total sensitive fields scrubbed: {}", redaction_count);
        }

        text
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ThreatSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, PartialEq)]
pub enum ThreatCategory {
    General,         // Jailbreaks, Context Escapes (Always enforced)
    CodeExecution,   // Python os.system, Bash, Eval (Bypassed if shell allowed)
    NetworkProbe,    // cURL, wget (Bypassed if network allowed)
}

struct ThreatRule {
    pattern: OnceLock<Regex>,
    regex_str: &'static str,
    threat_name: &'static str,
    severity: ThreatSeverity,
    category: ThreatCategory,
}

impl ThreatRule {
    const fn new(regex_str: &'static str, threat_name: &'static str, severity: ThreatSeverity, category: ThreatCategory) -> Self {
        Self {
            pattern: OnceLock::new(),
            regex_str,
            threat_name,
            severity,
            category,
        }
    }

    fn regex(&self) -> &Regex {
        self.pattern.get_or_init(|| Regex::new(self.regex_str).expect("Invalid threat regex pattern"))
    }
}

static THREAT_RULES: [ThreatRule; 7] = [
    // --- JAILBREAKS & HYPNOTISM ---
    ThreatRule::new(r"(?i)(ignore|disregard)\s+(all\s+)?(previous|prior)\s+(instructions|prompts|rules)", "Jailbreak (Ignore Previous)", ThreatSeverity::High, ThreatCategory::General),
    ThreatRule::new(r"(?i)you\s+are\s+now\s+(unbound|free|god|DAN|admin)", "Persona Hijack (Roleplay)", ThreatSeverity::Medium, ThreatCategory::General),

    // --- SYSTEM PROBES (RAG / Data Extraction) ---
    ThreatRule::new(r"(?i)(what|show|print|reveal|tell).{0,30}(system\s+prompt|root\s+password|hidden\s+instructions|core\s+directive)", "System Probe", ThreatSeverity::High, ThreatCategory::General),
    
    // --- SQL INJECTION (General - Always Blocked) ---
    ThreatRule::new(r"(?i)(\bUNION\b\s+\bSELECT\b|\bDROP\b\s+\bTABLE\b|\bOR\b\s+1=1)", "SQL Injection Attempt", ThreatSeverity::Critical, ThreatCategory::General),

    // --- CONTEXT ESCAPE (General - Always Blocked) ---
    ThreatRule::new(r"(?i)\]\]\]\s*\}\}\}\s*<\|im_end\|>", "Boundary Breakout Attempt", ThreatSeverity::Critical, ThreatCategory::General),

    // --- CODE EXECUTION (Tied to Manifest) ---
    // (If the agent has can_execute_shell = true, this is safely bypassed)
    ThreatRule::new(r"(?i)(os\.system|subprocess\.Popen|eval\(|exec\()", "Code Execution Payload", ThreatSeverity::Critical, ThreatCategory::CodeExecution),
    
    // --- NETWORK THREATS (Tied to Manifest) ---
    // (If the agent has network_enabled = true, this is safely bypassed)
    ThreatRule::new(r"(?i)(curl|wget|requests\.get)\s+http", "Network Probe", ThreatSeverity::High, ThreatCategory::NetworkProbe),
];

pub struct InjectionBlocker;

impl InjectionBlocker {
    pub fn check(prompt: &str, manifest: &AppManifest) -> Result<(), FirewallError> {
        for rule in &THREAT_RULES {
            if rule.regex().is_match(prompt) {

                let is_authorized = match rule.category {
                    // If rule is CodeExecution AND user granted shell, allow it!
                    ThreatCategory::CodeExecution => manifest.execution.can_execute_shell,
                    // If rule is NetworkProbe AND user granted network, allow it!
                    ThreatCategory::NetworkProbe => manifest.network.network_enabled,
                    // General jailbreaks are never authorized
                    ThreatCategory::General => false, 
                };

                if is_authorized {
                    crate::kprintln!(
                        "-> [FIREWALL: BYPASS] App '{}' is authorized for {:?}. Allowing payload.",
                        manifest.app_id, rule.category
                    );
                    continue;
                }
                
                // SIEM-Friendly Structured Logging
                crate::kprintln!(
                    "-> [FIREWALL: THREAT_DETECTED] App: {} | Threat: {} | Severity: {:?}", 
                    manifest.app_id, rule.threat_name, rule.severity
                );

                // For Critical threats OR if the agent has Shell Execution powers, we block instantly.
                if matches!(rule.severity, ThreatSeverity::Critical | ThreatSeverity::High) 
                   || manifest.execution.can_execute_shell 
                {
                    return Err(FirewallError::PromptInjection {
                        severity: rule.severity,
                        threat_name: rule.threat_name.to_string(),
                        app_id: manifest.app_id.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

// KERNEL ENTRY POINT
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