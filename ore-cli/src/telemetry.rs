use colored::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HostTelemetry {
    pub os_name: String,
    pub os_version: String,
    pub host_name: String,
    pub cpu_count: usize,
    pub cpu_brand: String,
    pub cpu_usage_pct: f32,
    pub total_memory_mb: u64,
    pub used_memory_mb: u64,
    pub free_memory_mb: u64,
    pub total_swap_mb: u64,
    pub used_swap_mb: u64,
    pub uptime_seconds: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KernelHealthResponse {
    pub status: String,
    pub version: String,
    pub engine: String,
    pub device: String,
    pub ore_dir: String,
    pub loaded_models: usize,
    pub registered_agents: usize,
    pub uptime_seconds: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModelProcessInfo {
    pub model_name: String,
    pub engine: String,
    pub device: String,
    pub status: String,
    pub active_requests: usize,
    pub host_ram_mb: u64,
    pub gpu_vram_mb: u64,
    pub last_used_secs: u64,
    pub current_app_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PsResponse {
    pub models: Vec<ModelProcessInfo>,
    pub total_ram_mb: u64,
    pub total_vram_mb: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TopTelemetryResponse {
    pub host: HostTelemetry,
    pub accelerator_device: String,
    pub vram_total_mb: u64,
    pub vram_used_mb: u64,
    pub vram_free_mb: u64,
    pub vram_reserved_mb: u64,
    pub vram_safety_margin_mb: u64,
    pub engine_name: String,
    pub scheduler_summary: String,
    pub active_models_count: usize,
    pub registered_agents_count: usize,
    pub context_firewall: String,
    pub semantic_bus_cache_count: usize,
    pub wasm_sandbox: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalModelInfo {
    pub name: String,
    pub format: String,
    pub size_bytes: u64,
    pub modified_at: String,
    pub is_loaded: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LsResponse {
    pub models: Vec<LocalModelInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentTelemetryInfo {
    pub app_id: String,
    pub version: String,
    pub allowed_models: Vec<String>,
    pub priority: String,
    pub network_enabled: bool,
    pub filesystem_permissions: String,
    pub execution_engine: String,
    pub pii_enforcement: bool,
    pub status: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AgentsResponse {
    pub agents: Vec<AgentTelemetryInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManifestTelemetryInfo {
    pub file_name: String,
    pub app_id: String,
    pub network: String,
    pub file_io: String,
    pub execution: String,
    pub pii_scrubbing: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManifestsResponse {
    pub manifests: Vec<ManifestTelemetryInfo>,
}

/// Computes visible character count in the terminal by ignoring ANSI escape codes.
pub fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_ansi = false;
    for c in s.chars() {
        if c == '\x1B' {
            in_ansi = true;
        } else if in_ansi {
            if c.is_ascii_alphabetic() {
                in_ansi = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

/// Left-pads a potentially ANSI-colored string to an exact visible terminal width.
pub fn pad_cell(s: &str, width: usize) -> String {
    let vlen = visible_len(s);
    if vlen >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - vlen))
    }
}

/// Right-pads a potentially ANSI-colored string to an exact visible terminal width.
#[allow(dead_code)]
pub fn pad_cell_right(s: &str, width: usize) -> String {
    let vlen = visible_len(s);
    if vlen >= width {
        s.to_string()
    } else {
        format!("{}{}", " ".repeat(width - vlen), s)
    }
}

/// Prints a top box border with an optional title: ╭─ TITLE ────────────╮
pub fn print_box_top(title: &str, inner_width: usize) {
    let total_width = inner_width + 6;
    let border = if title.is_empty() {
        format!("╭{}╮", "─".repeat(total_width.saturating_sub(2)))
    } else {
        let title_prefix = format!("╭─ {} ", title);
        let title_vlen = visible_len(&title_prefix);
        let dashes = total_width.saturating_sub(title_vlen + 1);
        format!("{}{}╮", title_prefix, "─".repeat(dashes))
    };
    println!("{}", border.bright_black());
}

/// Prints a bottom box border: ╰────────────────────────────╯
pub fn print_box_bottom(inner_width: usize) {
    let total_dashes = inner_width + 4;
    let border = format!("╰{}╯", "─".repeat(total_dashes));
    println!("{}", border.bright_black());
}

/// Prints an internal divider line: │  ────────────────────────  │
pub fn print_box_divider(inner_width: usize) {
    println!(
        "{}  {}  {}",
        "│".bright_black(),
        "─".repeat(inner_width).bright_black(),
        "│".bright_black()
    );
}

/// Prints a blank row inside the box
pub fn print_box_blank(inner_width: usize) {
    println!(
        "{}  {}  {}",
        "│".bright_black(),
        " ".repeat(inner_width),
        "│".bright_black()
    );
}

/// Truncates a string to at most `max_width` visible characters while preserving ANSI escape sequences.
pub fn truncate_visible(s: &str, max_width: usize) -> String {
    let mut res = String::new();
    let mut vlen = 0;
    let mut in_ansi = false;
    let mut ansi_buf = String::new();
    let target = max_width.saturating_sub(3);
    let mut truncated = false;

    for c in s.chars() {
        if c == '\x1B' {
            in_ansi = true;
            ansi_buf.push(c);
        } else if in_ansi {
            ansi_buf.push(c);
            if c.is_ascii_alphabetic() {
                in_ansi = false;
                res.push_str(&ansi_buf);
                ansi_buf.clear();
            }
        } else {
            if vlen < target {
                res.push(c);
                vlen += 1;
            } else {
                truncated = true;
                break;
            }
        }
    }

    if truncated {
        res.push_str("\x1B[0m...");
    }
    res
}

/// Prints a content row inside the box, guaranteed to align with the right border
pub fn print_box_row(content: &str, inner_width: usize) {
    let vlen = visible_len(content);
    let (rendered_content, padding) = if vlen > inner_width {
        (truncate_visible(content, inner_width), String::new())
    } else {
        (content.to_string(), " ".repeat(inner_width - vlen))
    };
    println!(
        "{}  {}{}{}",
        "│".bright_black(),
        rendered_content,
        padding,
        format!("  {}", "│".bright_black())
    );
}

fn progress_bar(pct: f64, width: usize) -> String {
    let p = pct.clamp(0.0, 100.0);
    let filled = ((p / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width.saturating_sub(filled);

    let color_block = if p > 85.0 {
        "█".repeat(filled).red()
    } else if p > 65.0 {
        "█".repeat(filled).yellow()
    } else {
        "█".repeat(filled).cyan()
    };

    format!("[{}{}]", color_block, "░".repeat(empty).bright_black())
}

pub fn render_status(h: &KernelHealthResponse) {
    let uptime_h = h.uptime_seconds / 3600;
    let uptime_m = (h.uptime_seconds % 3600) / 60;
    let uptime_s = h.uptime_seconds % 60;

    println!();
    print_box_top("ORE KERNEL SYSTEM STATUS", 76);
    print_box_blank(76);

    let row_state = format!(
        "{} :: {} {}",
        pad_cell(&"Kernel State".bold().to_string(), 16),
        format!("[{}]", h.status).green().bold(),
        format!("(v{})", h.version).bright_black()
    );
    print_box_row(&row_state, 76);

    let row_engine = format!(
        "{} :: {}",
        pad_cell(&"Inference HAL".bold().to_string(), 16),
        h.engine.cyan().bold()
    );
    print_box_row(&row_engine, 76);

    let row_device = format!(
        "{} :: {}",
        pad_cell(&"Compute Device".bold().to_string(), 16),
        h.device.yellow().bold()
    );
    print_box_row(&row_device, 76);

    let row_dir = format!(
        "{} :: {}",
        pad_cell(&"Home Directory".bold().to_string(), 16),
        h.ore_dir.white()
    );
    print_box_row(&row_dir, 76);

    let row_mem = format!(
        "{} :: {} {}",
        pad_cell(&"Active Memory".bold().to_string(), 16),
        h.loaded_models.to_string().cyan().bold(),
        "model(s) loaded in VRAM/RAM".bright_black()
    );
    print_box_row(&row_mem, 76);

    let row_apps = format!(
        "{} :: {} {}",
        pad_cell(&"Registered Apps".bold().to_string(), 16),
        h.registered_agents.to_string().cyan().bold(),
        "agents authorized".bright_black()
    );
    print_box_row(&row_apps, 76);

    let row_uptime = format!(
        "{} :: {}h {}m {}s",
        pad_cell(&"System Uptime".bold().to_string(), 16),
        uptime_h,
        uptime_m,
        uptime_s
    );
    print_box_row(&row_uptime, 76);

    let row_sec = format!(
        "{} :: {}",
        pad_cell(&"Security Ring".bold().to_string(), 16),
        "[ACTIVE] Token Verified & AES-GCM Enforcing".green()
    );
    print_box_row(&row_sec, 76);

    print_box_blank(76);
    print_box_bottom(76);
    println!();
}

pub fn render_top(top: &TopTelemetryResponse) {
    let h = &top.host;

    let ram_pct = if h.total_memory_mb > 0 {
        (h.used_memory_mb as f64 / h.total_memory_mb as f64) * 100.0
    } else {
        0.0
    };

    let swap_pct = if h.total_swap_mb > 0 {
        (h.used_swap_mb as f64 / h.total_swap_mb as f64) * 100.0
    } else {
        0.0
    };

    let vram_pct = if top.vram_total_mb > 0 {
        (top.vram_used_mb as f64 / top.vram_total_mb as f64) * 100.0
    } else {
        0.0
    };

    println!();
    print_box_top("HOST HARDWARE & OPERATING SYSTEM", 76);

    let os_row = format!(
        "{} : {}",
        pad_cell(&"OS Platform".bold().to_string(), 15),
        format!("{} {} ({})", h.os_name, h.os_version, h.host_name).white()
    );
    print_box_row(&os_row, 76);

    let proc_row = format!(
        "{} : {} ({} Cores)",
        pad_cell(&"Processor".bold().to_string(), 15),
        h.cpu_brand.cyan(),
        h.cpu_count
    );
    print_box_row(&proc_row, 76);

    let cpu_row = format!(
        "{} : {} {:>5.1}%",
        pad_cell(&"CPU Load".bold().to_string(), 15),
        progress_bar(h.cpu_usage_pct as f64, 18),
        h.cpu_usage_pct
    );
    print_box_row(&cpu_row, 76);

    let ram_used_disp = if h.used_memory_mb >= 1024 {
        format!("{:.1} GB", h.used_memory_mb as f64 / 1024.0)
    } else {
        format!("{} MB", h.used_memory_mb)
    };
    let ram_tot_disp = if h.total_memory_mb >= 1024 {
        format!("{:.1} GB", h.total_memory_mb as f64 / 1024.0)
    } else {
        format!("{} MB", h.total_memory_mb)
    };

    let ram_row = format!(
        "{} : {} {:>5.1}%  ({} / {})",
        pad_cell(&"Host Memory".bold().to_string(), 15),
        progress_bar(ram_pct, 18),
        ram_pct,
        ram_used_disp,
        ram_tot_disp
    );
    print_box_row(&ram_row, 76);

    let swap_used_disp = if h.used_swap_mb >= 1024 {
        format!("{:.1} GB", h.used_swap_mb as f64 / 1024.0)
    } else {
        format!("{} MB", h.used_swap_mb)
    };
    let swap_tot_disp = if h.total_swap_mb >= 1024 {
        format!("{:.1} GB", h.total_swap_mb as f64 / 1024.0)
    } else {
        format!("{} MB", h.total_swap_mb)
    };

    let swap_row = format!(
        "{} : {} {:>5.1}%  ({} / {})",
        pad_cell(&"Swap Memory".bold().to_string(), 15),
        progress_bar(swap_pct, 18),
        swap_pct,
        swap_used_disp,
        swap_tot_disp
    );
    print_box_row(&swap_row, 76);

    let up_row = format!(
        "{} : {}h {}m {}s",
        pad_cell(&"Host Uptime".bold().to_string(), 15),
        h.uptime_seconds / 3600,
        (h.uptime_seconds % 3600) / 60,
        h.uptime_seconds % 60
    );
    print_box_row(&up_row, 76);

    print_box_bottom(76);

    print_box_top("ACCELERATOR & VRAM ACCOUNTING", 76);

    let dev_row = format!(
        "{} : {}",
        pad_cell(&"Compute Device".bold().to_string(), 15),
        top.accelerator_device.yellow().bold()
    );
    print_box_row(&dev_row, 76);

    if top.vram_total_mb > 0 {
        let vram_used_disp = if top.vram_used_mb >= 1024 {
            format!("{:.1} GB", top.vram_used_mb as f64 / 1024.0)
        } else {
            format!("{} MB", top.vram_used_mb)
        };
        let vram_tot_disp = if top.vram_total_mb >= 1024 {
            format!("{:.1} GB", top.vram_total_mb as f64 / 1024.0)
        } else {
            format!("{} MB", top.vram_total_mb)
        };

        let vram_row = format!(
            "{} : {} {:>5.1}%  ({} / {})",
            pad_cell(&"VRAM Load".bold().to_string(), 15),
            progress_bar(vram_pct, 18),
            vram_pct,
            vram_used_disp,
            vram_tot_disp
        );
        print_box_row(&vram_row, 76);
    } else {
        let vram_row = format!(
            "{} : {}",
            pad_cell(&"VRAM Status".bold().to_string(), 15),
            "Unified Host Memory (CPU/Metal Fallback Active)".white()
        );
        print_box_row(&vram_row, 76);
    }

    let res_row = format!(
        "{} : Reserved: {} MB  |  Safety Buffer: {} MB",
        pad_cell(&"Reservations".bold().to_string(), 15),
        top.vram_reserved_mb.to_string().cyan(),
        top.vram_safety_margin_mb.to_string().yellow()
    );
    print_box_row(&res_row, 76);

    print_box_bottom(76);

    print_box_top("ORE KERNEL SUBSYSTEMS", 76);

    let subs = [
        ("Inference HAL", "[ACTIVE]".green().bold(), top.engine_name.as_str()),
        ("GPU Scheduler", "[RUNNING]".green().bold(), top.scheduler_summary.as_str()),
        ("Context Firewall", "[ENFORCING]".green().bold(), top.context_firewall.as_str()),
        ("WASM Sandbox", "[ISOLATED]".green().bold(), top.wasm_sandbox.as_str()),
    ];
    for (label, badge, val) in subs {
        let row = format!(
            "{} : {}  {}",
            pad_cell(&label.bold().to_string(), 17),
            badge,
            val.white()
        );
        print_box_row(&row, 76);
    }

    let app_row = format!(
        "{} : {}  {} Registered Agents",
        pad_cell(&"App Registry".bold().to_string(), 17),
        "[LOADED]".green().bold(),
        top.registered_agents_count.to_string().cyan()
    );
    print_box_row(&app_row, 76);

    let sem_row = format!(
        "{} : {}  {} Cached Embeddings",
        pad_cell(&"Semantic Bus".bold().to_string(), 17),
        "[READY]".green().bold(),
        top.semantic_bus_cache_count.to_string().cyan()
    );
    print_box_row(&sem_row, 76);

    print_box_bottom(76);
    println!();
}

pub fn render_ps(ps: &PsResponse) {
    println!();
    print_box_top("RUNNING MODELS IN MEMORY", 108);
    print_box_blank(108);

    let header = format!(
        "{}  {}  {}  {}  {}  {}  {}  {}",
        pad_cell(&"MODEL".bold().to_string(), 18),
        pad_cell(&"COMPUTE".bold().to_string(), 14),
        pad_cell(&"STATUS".bold().to_string(), 10),
        pad_cell(&"REQS".bold().to_string(), 6),
        pad_cell(&"HOST RAM".bold().to_string(), 10),
        pad_cell(&"GPU VRAM".bold().to_string(), 10),
        pad_cell(&"LAST USED".bold().to_string(), 10),
        pad_cell(&"ACTIVE APP".bold().to_string(), 14),
    );
    print_box_row(&header, 108);
    print_box_divider(108);

    if ps.models.is_empty() {
        print_box_row("No models currently loaded in memory. Run 'ore run <model>' to load.", 108);
    } else {
        for m in &ps.models {
            let status_badge = if m.status == "ACTIVE" {
                "[ACTIVE]".green().bold()
            } else if m.status == "IDLE" {
                "[IDLE]".cyan().bold()
            } else if m.status == "LOADED" {
                "[LOADED]".green().bold()
            } else if m.status == "LOADING" {
                "[LOADING]".yellow().bold()
            } else {
                format!("[{}]", m.status).white()
            };

            let last_used = if m.last_used_secs == 0 {
                "Just now".to_string()
            } else if m.last_used_secs < 60 {
                format!("{}s ago", m.last_used_secs)
            } else {
                format!("{}m ago", m.last_used_secs / 60)
            };

            let active_app = m.current_app_id.as_deref().unwrap_or("-");

            let model_disp = if m.model_name.len() > 18 {
                format!("{}...", &m.model_name[..15])
            } else {
                m.model_name.clone()
            };

            let device_disp = if m.device.len() > 20 {
                format!("{}...", &m.device[..17])
            } else {
                m.device.clone()
            };

            let app_disp = if active_app.len() > 20 {
                format!("{}...", &active_app[..17])
            } else {
                active_app.to_string()
            };

            let row = format!(
                "{}  {}  {}  {}  {}  {}  {}  {}",
                pad_cell(&model_disp.cyan().bold().to_string(), 18),
                pad_cell(&device_disp.bright_black().to_string(), 14),
                pad_cell(&status_badge.to_string(), 10),
                pad_cell(&m.active_requests.to_string(), 6),
                pad_cell(&format!("{} MB", m.host_ram_mb), 10),
                pad_cell(&format!("{} MB", m.gpu_vram_mb).yellow().to_string(), 10),
                pad_cell(&last_used.bright_black().to_string(), 10),
                pad_cell(&app_disp.bright_blue().to_string(), 14),
            );
            print_box_row(&row, 108);
        }
    }

    print_box_blank(108);
    let summary = format!(
        "Total Memory: {} Host RAM  |  {} GPU VRAM Allocated",
        format!("{} MB", ps.total_ram_mb).cyan().bold(),
        format!("{} MB", ps.total_vram_mb).yellow().bold()
    );
    print_box_row(&summary, 108);
    print_box_bottom(108);
    println!();
}

pub fn render_ls(ls: &LsResponse) {
    println!();
    print_box_top("LOCAL INSTALLED MODELS", 102);
    print_box_blank(102);

    let header = format!(
        "{}  {}  {}  {}  {}",
        pad_cell(&"REPOSITORY".bold().to_string(), 28),
        pad_cell(&"FORMAT".bold().to_string(), 18),
        pad_cell(&"SIZE".bold().to_string(), 12),
        pad_cell(&"LAST MODIFIED".bold().to_string(), 22),
        pad_cell(&"STATE".bold().to_string(), 14),
    );
    print_box_row(&header, 102);
    print_box_divider(102);

    if ls.models.is_empty() {
        print_box_row("No models installed locally. Use 'ore pull <model>' to download.", 102);
    } else {
        let mut total_bytes: u64 = 0;
        for m in &ls.models {
            total_bytes += m.size_bytes;

            let size_disp = if m.size_bytes >= 1024 * 1024 * 1024 {
                format!("{:.2} GB", m.size_bytes as f64 / (1024.0 * 1024.0 * 1024.0))
            } else {
                format!("{:.1} MB", m.size_bytes as f64 / (1024.0 * 1024.0))
            };

            let state_badge = if m.is_loaded {
                "[IN VRAM]".green().bold()
            } else {
                "[ON DISK]".bright_black()
            };

            let row = format!(
                "{}  {}  {}  {}  {}",
                pad_cell(&m.name.cyan().bold().to_string(), 28),
                pad_cell(&m.format.bright_black().to_string(), 18),
                pad_cell(&size_disp.yellow().to_string(), 12),
                pad_cell(&m.modified_at.white().to_string(), 22),
                pad_cell(&state_badge.to_string(), 14),
            );
            print_box_row(&row, 102);
        }

        print_box_blank(102);
        let total_gb = total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let summary = format!(
            "Total Storage Utilized: {} ({:.2} GB across {} model packages)",
            format!("{:.2} GB", total_gb).green().bold(),
            total_gb,
            ls.models.len()
        );
        print_box_row(&summary, 102);
    }

    print_box_bottom(102);
    println!();
}

pub fn render_agents(agents: &AgentsResponse) {
    println!();
    print_box_top("REGISTERED ORE AGENTS & APPS", 104);
    print_box_blank(104);

    let header = format!(
        "{}  {}  {}  {}  {}  {}  {}",
        pad_cell(&"AGENT ID".bold().to_string(), 20),
        pad_cell(&"VERSION".bold().to_string(), 8),
        pad_cell(&"ALLOWED MODELS".bold().to_string(), 22),
        pad_cell(&"PRIORITY".bold().to_string(), 9),
        pad_cell(&"FILE I/O".bold().to_string(), 11),
        pad_cell(&"EXECUTION".bold().to_string(), 12),
        pad_cell(&"STATUS".bold().to_string(), 10),
    );
    print_box_row(&header, 104);
    print_box_divider(104);

    if agents.agents.is_empty() {
        print_box_row("No agents registered. Use 'ore manifest <name>' to scaffold an agent manifest.", 104);
    } else {
        for a in &agents.agents {
            let models_str = if a.allowed_models.is_empty() {
                "-".to_string()
            } else {
                a.allowed_models.join(", ")
            };
            let models_disp = if models_str.len() > 20 {
                format!("{}...", &models_str[..17])
            } else {
                models_str
            };

            let status_badge = if a.status == "SECURED" {
                "[SECURED]".green().bold()
            } else if a.status == "AIR-GAPPED" {
                "[AIR-GAP]".cyan().bold()
            } else {
                "[UNSAFE]".red().bold()
            };

            let row = format!(
                "{}  {}  {}  {}  {}  {}  {}",
                pad_cell(&a.app_id.cyan().bold().to_string(), 20),
                pad_cell(&a.version.bright_black().to_string(), 8),
                pad_cell(&models_disp.white().to_string(), 22),
                pad_cell(&a.priority.yellow().to_string(), 9),
                pad_cell(&a.filesystem_permissions.bright_black().to_string(), 11),
                pad_cell(&a.execution_engine.white().to_string(), 12),
                pad_cell(&status_badge.to_string(), 10),
            );
            print_box_row(&row, 104);
        }

        print_box_blank(104);
        let summary = format!(
            "Total Registered Agents: {}",
            agents.agents.len().to_string().cyan().bold()
        );
        print_box_row(&summary, 104);
    }

    print_box_bottom(104);
    println!();
}

pub fn render_manifests(m: &ManifestsResponse) {
    println!();
    print_box_top("AGENT MANIFEST AUDIT", 106);
    print_box_blank(106);

    let header = format!(
        "{}  {}  {}  {}  {}  {}",
        pad_cell(&"MANIFEST FILE".bold().to_string(), 24),
        pad_cell(&"APP ID".bold().to_string(), 20),
        pad_cell(&"NETWORK".bold().to_string(), 12),
        pad_cell(&"FILE I/O".bold().to_string(), 12),
        pad_cell(&"EXECUTION".bold().to_string(), 14),
        pad_cell(&"PII SCRUB".bold().to_string(), 14),
    );
    print_box_row(&header, 106);
    print_box_divider(106);

    if m.manifests.is_empty() {
        print_box_row("No manifests found in /manifests directory.", 106);
    } else {
        for item in &m.manifests {
            let net_badge = if item.network == "ENABLED" {
                "ENABLED".green().bold()
            } else {
                "BLOCKED".red().bold()
            };

            let pii_badge = if item.pii_scrubbing == "ACTIVE" {
                "ACTIVE".green().bold()
            } else {
                "OFF (RISK)".red().bold()
            };

            let row = format!(
                "{}  {}  {}  {}  {}  {}",
                pad_cell(&item.file_name.cyan().to_string(), 24),
                pad_cell(&item.app_id.white().to_string(), 20),
                pad_cell(&net_badge.to_string(), 12),
                pad_cell(&item.file_io.bright_black().to_string(), 12),
                pad_cell(&item.execution.white().to_string(), 14),
                pad_cell(&pii_badge.to_string(), 14),
            );
            print_box_row(&row, 106);
        }

        print_box_blank(106);
        let summary = format!(
            "Total Scanned Manifests: {}",
            m.manifests.len().to_string().cyan().bold()
        );
        print_box_row(&summary, 106);
    }

    print_box_bottom(106);
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visible_len_and_padding() {
        let plain = "llama3.2:1b";
        assert_eq!(visible_len(plain), 11);

        let colored_str = plain.cyan().bold().to_string();
        assert_eq!(visible_len(&colored_str), 11);

        let padded = pad_cell(&colored_str, 28);
        assert_eq!(visible_len(&padded), 28);
    }

    #[test]
    fn test_box_row_width() {
        let title_prefix = format!("╭─ {} ", "LOCAL INSTALLED MODELS");
        let dashes = 108 - visible_len(&title_prefix) - 1;
        let top = format!("{}{}╮", title_prefix, "─".repeat(dashes));
        assert_eq!(visible_len(&top), 108);

        let bot = format!("╰{}╯", "─".repeat(106));
        assert_eq!(visible_len(&bot), 108);
    }

    #[test]
    fn test_truncate_visible() {
        let long = "Very long text that should be truncated to fit nicely inside the box boundaries without overflowing";
        let truncated = truncate_visible(long, 40);
        assert_eq!(visible_len(&truncated), 40);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_render_sample_output() {
        let ls = LsResponse {
            models: vec![
                LocalModelInfo {
                    name: "all-minilm".to_string(),
                    format: "Safetensors".to_string(),
                    size_bytes: 87_100_000,
                    modified_at: "2026-03-24 14:22:36".to_string(),
                    is_loaded: false,
                },
                LocalModelInfo {
                    name: "llama3.2:1b".to_string(),
                    format: "GGUF (Quantized)".to_string(),
                    size_bytes: 786_700_000,
                    modified_at: "2026-03-18 15:06:23".to_string(),
                    is_loaded: true,
                },
                LocalModelInfo {
                    name: "qwen2.5:0.5b".to_string(),
                    format: "GGUF (Quantized)".to_string(),
                    size_bytes: 479_600_000,
                    modified_at: "2026-03-19 15:52:50".to_string(),
                    is_loaded: false,
                },
                LocalModelInfo {
                    name: "system-embedder".to_string(),
                    format: "Safetensors".to_string(),
                    size_bytes: 522_300_000,
                    modified_at: "2026-03-28 15:11:30".to_string(),
                    is_loaded: false,
                },
            ],
        };
        render_ls(&ls);

        let ps = PsResponse {
            models: vec![
                ModelProcessInfo {
                    model_name: "llama3.2:1b".to_string(),
                    engine: "Ollama Managed".to_string(),
                    device: "Host CPU".to_string(),
                    status: "IDLE".to_string(),
                    active_requests: 0,
                    host_ram_mb: 750,
                    gpu_vram_mb: 0,
                    last_used_secs: 42,
                    current_app_id: Some("agent_swarm".to_string()),
                },
            ],
            total_ram_mb: 750,
            total_vram_mb: 0,
        };
        render_ps(&ps);

        let health = KernelHealthResponse {
            status: "HEALTHY".to_string(),
            version: "0.1.0".to_string(),
            engine: "Native Candle HAL".to_string(),
            device: "NVIDIA GeForce RTX 4090".to_string(),
            ore_dir: "C:\\Users\\mahav\\.ore".to_string(),
            loaded_models: 1,
            registered_agents: 4,
            uptime_seconds: 3725,
        };
        render_status(&health);

        let top = TopTelemetryResponse {
            host: HostTelemetry {
                os_name: "Windows".to_string(),
                os_version: "11 Home".to_string(),
                host_name: "DESKTOP-ORE".to_string(),
                cpu_count: 16,
                cpu_brand: "AMD Ryzen 7 7800X3D".to_string(),
                cpu_usage_pct: 14.5,
                total_memory_mb: 32768,
                used_memory_mb: 14200,
                free_memory_mb: 18568,
                total_swap_mb: 4096,
                used_swap_mb: 512,
                uptime_seconds: 7300,
            },
            accelerator_device: "NVIDIA GeForce RTX 4090".to_string(),
            vram_total_mb: 24576,
            vram_used_mb: 12288,
            vram_free_mb: 12288,
            vram_reserved_mb: 1024,
            vram_safety_margin_mb: 512,
            engine_name: "Native Candle HAL".to_string(),
            scheduler_summary: "Active App: swarm-01 | Queue: 0".to_string(),
            active_models_count: 1,
            registered_agents_count: 4,
            context_firewall: "Active & Scrubbing".to_string(),
            semantic_bus_cache_count: 24,
            wasm_sandbox: "Wasmtime Memory Sandbox".to_string(),
        };
        render_top(&top);

        let agents = AgentsResponse {
            agents: vec![
                AgentTelemetryInfo {
                    app_id: "agent_swarm".to_string(),
                    version: "1.0.0".to_string(),
                    allowed_models: vec!["llama3.2:1b".to_string()],
                    priority: "HIGH".to_string(),
                    network_enabled: true,
                    filesystem_permissions: "SANDBOX".to_string(),
                    execution_engine: "WASM (V8)".to_string(),
                    pii_enforcement: true,
                    status: "SECURED".to_string(),
                },
            ],
        };
        render_agents(&agents);

        let manifests = ManifestsResponse {
            manifests: vec![
                ManifestTelemetryInfo {
                    file_name: "swarm_agent.json".to_string(),
                    app_id: "agent_swarm".to_string(),
                    network: "ENABLED".to_string(),
                    file_io: "SANDBOX".to_string(),
                    execution: "WASM".to_string(),
                    pii_scrubbing: "ACTIVE".to_string(),
                },
            ],
        };
        render_manifests(&manifests);
    }
}

