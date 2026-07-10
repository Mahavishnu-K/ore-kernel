use colored::*;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use inquire::ui::{Color, RenderConfig, StyleSheet, Styled};
use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::Deserialize;
use std::path::PathBuf;
use std::{cmp::min, fs, io::Write, path::Path, process::exit};

#[derive(Deserialize)]
pub struct OreConfig {
    system: SystemConfig,
}

#[derive(Deserialize)]
pub struct SystemConfig {
    engine: String,
}

pub fn get_ore_dir() -> PathBuf {
    if let Ok(custom_dir) = std::env::var("ORE_DIR") {
        return PathBuf::from(custom_dir);
    }

    let local_dev_path = PathBuf::from("..");
    if local_dev_path.join("ore.toml").exists() {
        return local_dev_path;
    }

    let home = std::env::var("USERPROFILE") // Windows
        .or_else(|_| std::env::var("HOME")) // Linux/macOS
        .expect("FATAL: Could not determine user home directory.");

    let ore_path = PathBuf::from(home).join(".ore");

    if !ore_path.exists() {
        fs::create_dir_all(&ore_path).expect("FATAL: Failed to create ~/.ore directory.");
    }

    ore_path
}

fn visible_len(text: &str) -> usize {
    let mut len = 0;
    let mut in_ansi = false;
    for c in text.chars() {
        if c == '\x1B' {
            in_ansi = true;
        } else if in_ansi {
            // ANSI escape sequences typically end with an alphabetic character (like 'm' for colors)
            if c.is_ascii_alphabetic() {
                in_ansi = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

/// Generates the premium Hermes/Claude-style TUI theme
pub fn get_ore_theme() -> RenderConfig<'static> {
    RenderConfig {
        prompt_prefix: Styled::new("?").with_fg(Color::LightYellow),
        answered_prompt_prefix: Styled::new("✔").with_fg(Color::LightGreen),
        help_message: StyleSheet::new().with_fg(Color::DarkGrey),
        answer: StyleSheet::new().with_fg(Color::LightCyan),
        selected_checkbox: Styled::new("[x]").with_fg(Color::LightGreen),
        unselected_checkbox: Styled::new("[ ]").with_fg(Color::DarkGrey),
        highlighted_option_prefix: Styled::new(">").with_fg(Color::LightCyan),
        selected_option: Some(StyleSheet::new().with_fg(Color::LightCyan)),
        ..RenderConfig::default()
    }
}

/// Prints a sleek unicode border box
pub fn print_panel(title: &str, subtitle: &str) {
    println!();
    let width: usize = 65; // Fixed terminal width for the boxes

    // Top border
    let top_filler = "─".repeat(width.saturating_sub(title.len() + 5));
    println!(
        "{} {} {}{}",
        "╭─".bright_black(),
        title.bold(),
        top_filler.bright_black(),
        "╮".bright_black()
    );

    // Middle section (centered subtitle)
    if !subtitle.is_empty() {
        let sub_len = visible_len(subtitle);
        let available_space = width.saturating_sub(2); // Space inside the borders

        let total_padding = available_space.saturating_sub(sub_len);
        let left_pad = " ".repeat(total_padding / 2);
        let right_pad = " ".repeat(total_padding - (total_padding / 2)); // Catches odd numbers

        println!(
            "{}{}{}{}{}",
            "│".bright_black(),
            left_pad,
            subtitle.bright_black(),
            right_pad,
            "│".bright_black()
        );
    }

    // Bottom border
    let bottom_filler = "─".repeat(width - 2);
    println!(
        "{}{}{}",
        "╰".bright_black(),
        bottom_filler.bright_black(),
        "╯".bright_black()
    );
    println!();
}

/// Prints a section divider that aligns perfectly with the panel width
pub fn print_section_divider(num: &str, title: &str) {
    let width: usize = 65;
    let filler_len = width.saturating_sub(title.len() + num.len() + 7);
    let filler = "─".repeat(filler_len);
    println!(
        "\n{}",
        format!("─── {}. {} {}", num, title, filler).bright_black()
    );
}

pub fn get_system_engine() -> String {
    let config_path = "../ore.toml";
    match fs::read_to_string(config_path) {
        Ok(contents) => match toml::from_str::<OreConfig>(&contents) {
            Ok(config) => config.system.engine,
            Err(_) => {
                println!("{} FATAL: ore.toml is corrupted.", "[-]".red().bold());
                println!("       Please run 'ore init' to regenerate it.");
                exit(1);
            }
        },
        Err(_) => {
            println!(
                "{} FATAL: ORE System is not initialized.",
                "[-]".red().bold()
            );
            println!("       Please run 'ore init' first.");
            exit(1);
        }
    }
}

pub enum OreAsset {
    Gguf {
        gguf_repo: &'static str,
        gguf_file: &'static str,
        base_repo: &'static str,
    },
    Safetensors {
        repo: &'static str,
    },
    Wasm {
        url: &'static str,
        folder: &'static str,
        filename: &'static str,
    },
}

/// Maps a simple user alias to Hugging Face repositories
pub fn get_asset_map(alias: &str) -> Option<OreAsset> {
    match alias {
        // QWEN 2.5 INSTRUCT (For General Chat & Agent Swarms)
        // The Tiny Models (Ultra-fast, fits anywhere)
        "qwen2.5:0.5b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-0.5B-Instruct-GGUF",
            gguf_file: "qwen2.5-0.5b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-0.5B-Instruct",
        }),
        "qwen2.5:1.5b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
            gguf_file: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-1.5B-Instruct",
        }),
        "qwen2.5:3b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-3B-Instruct-GGUF",
            gguf_file: "qwen2.5-3b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-3B-Instruct",
        }),

        // The Workhorses (8GB - 16GB VRAM)
        "qwen2.5:7b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-7B-Instruct-GGUF",
            gguf_file: "qwen2.5-7b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-7B-Instruct",
        }),
        "qwen2.5:7b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-7B-Instruct-GGUF",
            gguf_file: "qwen2.5-7b-instruct-q8_0.gguf",
            base_repo: "Qwen/Qwen2.5-7B-Instruct",
        }),
        "qwen2.5:14b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-14B-Instruct-GGUF",
            gguf_file: "qwen2.5-14b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-14B-Instruct",
        }),
        "qwen2.5:14b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-14B-Instruct-GGUF",
            gguf_file: "qwen2.5-14b-instruct-q8_0.gguf",
            base_repo: "Qwen/Qwen2.5-14B-Instruct",
        }),

        // The Heavyweights (24GB+ VRAM / Mac Studios)
        "qwen2.5:32b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-32B-Instruct-GGUF",
            gguf_file: "qwen2.5-32b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-32B-Instruct",
        }),
        "qwen2.5:72b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-72B-Instruct-GGUF",
            gguf_file: "qwen2.5-72b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-72B-Instruct",
        }),

        // 2. QWEN 2.5 CODER (For Strict Software Engineering Agents)
        "qwen2.5-coder:0.5b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF",
            gguf_file: "qwen2.5-coder-0.5b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-Coder-0.5B-Instruct",
        }),
        "qwen2.5-coder:1.5b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF",
            gguf_file: "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-Coder-1.5B-Instruct",
        }),
        "qwen2.5-coder:3b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-Coder-3B-Instruct-GGUF",
            gguf_file: "qwen2.5-coder-3b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-Coder-3B-Instruct",
        }),
        "qwen2.5-coder:7b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF",
            gguf_file: "qwen2.5-coder-7b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-Coder-7B-Instruct",
        }),
        "qwen2.5-coder:7b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF",
            gguf_file: "qwen2.5-coder-7b-instruct-q8_0.gguf",
            base_repo: "Qwen/Qwen2.5-Coder-7B-Instruct",
        }),
        "qwen2.5-coder:14b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-Coder-14B-Instruct-GGUF",
            gguf_file: "qwen2.5-coder-14b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-Coder-14B-Instruct",
        }),
        "qwen2.5-coder:14b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-Coder-14B-Instruct-GGUF",
            gguf_file: "qwen2.5-coder-14b-instruct-q8_0.gguf",
            base_repo: "Qwen/Qwen2.5-Coder-14B-Instruct",
        }),
        "qwen2.5-coder:32b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-Coder-32B-Instruct-GGUF",
            gguf_file: "qwen2.5-coder-32b-instruct-q4_k_m.gguf",
            base_repo: "Qwen/Qwen2.5-Coder-32B-Instruct",
        }),
        "qwen2.5-coder:32b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen2.5-Coder-32B-Instruct-GGUF",
            gguf_file: "qwen2.5-coder-32b-instruct-q8_0.gguf",
            base_repo: "Qwen/Qwen2.5-Coder-32B-Instruct",
        }),

        // Qwen's official QwQ Reasoning Model
        "qwq:32b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/QwQ-32B-Preview-GGUF",
            gguf_file: "QwQ-32B-Preview-Q4_K_M.gguf",
            base_repo: "unsloth/QwQ-32B-Preview",
        }),
        "qwq:32b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/QwQ-32B-Preview-GGUF",
            gguf_file: "QwQ-32B-Preview-Q8_0.gguf",
            base_repo: "unsloth/QwQ-32B-Preview",
        }),

        // QWEN 3 INSTRUCT (The Next-Gen Chat & Agents)
        "qwen3:0.6b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-0.6B-GGUF",
            gguf_file: "Qwen3-0.6B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-0.6B",
        }),
        "qwen3:0.6b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-0.6B-GGUF",
            gguf_file: "Qwen3-0.6B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-0.6B",
        }),
        "qwen3:1.7b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-1.7B-GGUF",
            gguf_file: "Qwen3-1.7B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-1.7B",
        }),
        "qwen3:1.7b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-1.7B-GGUF",
            gguf_file: "Qwen3-1.7B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-1.7B",
        }),
        "qwen3:4b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-4B-GGUF",
            gguf_file: "Qwen3-4B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-4B",
        }),
        "qwen3:4b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-4B-GGUF",
            gguf_file: "Qwen3-4B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-4B",
        }),

        // The Workhorses (8GB - 16GB VRAM)
        "qwen3:8b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-8B-GGUF",
            gguf_file: "Qwen3-8B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-8B",
        }),
        "qwen3:8b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-8B-GGUF",
            gguf_file: "Qwen3-8B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-8B",
        }),
        "qwen3:14b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-14B-GGUF",
            gguf_file: "Qwen3-14B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-14B",
        }),
        "qwen3:14b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-14B-GGUF",
            gguf_file: "Qwen3-14B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-14B",
        }),

        // The Heavyweights (24GB+ VRAM / Mac Studios)
        "qwen3:32b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-32B-GGUF",
            gguf_file: "Qwen3-32B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-32B",
        }),
        "qwen3:32b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-32B-GGUF",
            gguf_file: "Qwen3-32B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-32B",
        }),

        "qwen3:235b-a22b" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-235B-A22B-GGUF",
            gguf_file: "Qwen3-235B-A22B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-235B-A22B",
        }),
        "qwen3:235b-a22b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-235B-A22B-GGUF",
            gguf_file: "Qwen3-235B-A22B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-235B-A22B",
        }),

        // 2. QWEN 3 CODER (The Bleeding-Edge Software Agents)
        "qwen3-coder:30b-a3b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF",
            gguf_file: "Qwen3-Coder-30B-A3B-Instruct-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-Coder-30B-A3B-Instruct",
        }),
        "qwen3-coder:30b-a3b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF",
            gguf_file: "Qwen3-Coder-30B-A3B-Instruct-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-Coder-30B-A3B-Instruct",
        }),

        "qwen3-coder:480b-a35b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3-Coder-480B-A35B-Instruct-GGUF",
            gguf_file: "Qwen3-Coder-480B-A35B-Instruct-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-Coder-480B-A35B-Instruct",
        }),
        "qwen3-coder:480b-a35b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3-Coder-480B-A35B-Instruct-GGUF",
            gguf_file: "Qwen3-Coder-480B-A35B-Instruct-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-Coder-480B-A35B-Instruct",
        }),

        // QWEN 3.5 FAMILY (Instruct & Coder)
        "qwen3.5:0.8b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-0.8B-GGUF",
            gguf_file: "Qwen3.5-0.8B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3.5-0.8B",
        }),
        "qwen3.5:0.8b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-0.8B-GGUF",
            gguf_file: "Qwen3.5-0.8B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3.5-0.8B",
        }),
        "qwen3.5:2b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-2B-GGUF",
            gguf_file: "Qwen3.5-2B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3.5-2B",
        }),
        "qwen3.5:2b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-2B-GGUF",
            gguf_file: "Qwen3.5-2B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3.5-2B",
        }),
        "qwen3.5:4b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-4B-GGUF",
            gguf_file: "Qwen3.5-4B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3.5-4B",
        }),
        "qwen3.5:4b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-4B-GGUF",
            gguf_file: "Qwen3.5-4B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3.5-4B",
        }),
        "qwen3.5:9b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-9B-GGUF",
            gguf_file: "Qwen3.5-9B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3.5-9B",
        }),
        "qwen3.5:9b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-9B-GGUF",
            gguf_file: "Qwen3.5-9B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3.5-9B",
        }),

        // --- Medium (dense) ---
        "qwen3.5:27b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-27B-GGUF",
            gguf_file: "Qwen3.5-27B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3.5-27B",
        }),
        "qwen3.5:27b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-27B-GGUF",
            gguf_file: "Qwen3.5-27B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3.5-27B",
        }),

        // --- Medium (MoE) ---
        "qwen3.5:35b-a3b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-35B-A3B-GGUF",
            gguf_file: "Qwen3.5-35B-A3B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3.5-35B-A3B",
        }),
        "qwen3.5:35b-a3b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-35B-A3B-GGUF",
            gguf_file: "Qwen3.5-35B-A3B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3.5-35B-A3B",
        }),
        "qwen3.5:122b-a10b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-122B-A10B-GGUF",
            gguf_file: "Qwen3.5-122B-A10B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3.5-122B-A10B",
        }),
        "qwen3.5:122b-a10b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.5-122B-A10B-GGUF",
            gguf_file: "Qwen3.5-122B-A10B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3.5-122B-A10B",
        }),

        // QWEN3.6 FAMILY (Instruct & Coder)
        "qwen3.6:27b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.6-27B-GGUF",
            gguf_file: "Qwen3.6-27B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3.6-27B",
        }),
        "qwen3.6:27b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.6-27B-GGUF",
            gguf_file: "Qwen3.6-27B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3.6-27B",
        }),

        // "Qwen3.6-35B-A3B: Agentic Coding Power, Now Open to All"
        "qwen3.6:35b-a3b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.6-35B-A3B-GGUF",
            gguf_file: "Qwen3.6-35B-A3B-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3.6-35B-A3B",
        }),
        "qwen3.6:35b-a3b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Qwen3.6-35B-A3B-GGUF",
            gguf_file: "Qwen3.6-35B-A3B-Q8_0.gguf",
            base_repo: "Qwen/Qwen3.6-35B-A3B",
        }),

        "qwen3-coder-next" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-Coder-Next-GGUF",
            gguf_file: "Qwen3-Coder-Next-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-Coder-Next",
        }),
        // WARNING: Q8_0 here is ~85GB. Confirmed present on the official repo
        "qwen3-coder-next-q8" => Some(OreAsset::Gguf {
            gguf_repo: "Qwen/Qwen3-Coder-Next-GGUF",
            gguf_file: "Qwen3-Coder-Next-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-Coder-Next",
        }),

        // QWEN3-VL (vision-language family, collection: huggingface.co/collections/Qwen/qwen3-vl)
        //
        // ⚠️ STRUCTURAL ISSUE — READ BEFORE USING ⚠️
        // Every VL GGUF repo ships TWO files per quant level, not one:
        //   1. the language-model weights (what gguf_file already points to below)
        //   2. a separate vision-encoder file, "mmproj-...gguf", required to do
        //      anything with images/video at all.
        // Confirmed directly off the live file tree for the 8B-Instruct repo:
        //   Qwen3VL-8B-Instruct-Q4_K_M.gguf      (LLM,    5.03 GB)
        //   Qwen3VL-8B-Instruct-Q8_0.gguf        (LLM,    8.71 GB)
        //   mmproj-Qwen3VL-8B-Instruct-F16.gguf  (vision, 1.16 GB)
        //   mmproj-Qwen3VL-8B-Instruct-Q8_0.gguf (vision, 752 MB)
        // The existing OreAsset::Gguf variant used throughout this file only has
        // a single `gguf_file` slot. As written below, every entry is INCOMPLETE
        // without its mmproj counterpart — a downloader using only `gguf_file` will
        // fetch the LLM but the model won't be able to see images. This needs a
        // second field (e.g. `mmproj_file: &'static str`) added to the struct, or
        // these entries need to be split into LLM/mmproj pairs, before this is
        // actually usable for VL models. Flagging rather than silently omitting it.
        // mmproj filenames included as comments next to each entry below.
        //
        // FILENAME QUIRK: the repo name uses "Qwen3-VL-" (hyphenated) but every
        // actual filename inside uses "Qwen3VL-" (no hyphen). Don't copy the repo
        // name pattern into the filename.
        //
        // SIZES & MODES — two independent axes, confirmed directly off the
        // official collection page (12 GGUF repos total, full matrix, no gaps):
        //   Sizes: 2B, 4B, 8B, 32B (all dense) · 30B-A3B, 235B-A22B (both MoE)
        //   Modes: Instruct (non-thinking) and Thinking — both exist for every size
        //
        // SHARDING: at 235B-A22B, LLM weights for larger quants ship as multi-file
        // shards (e.g. "...-Q4_K_M-split-00001-of-00003.gguf"), confirmed via the
        // official repo's own usage example. The Q4_K_M filename used below for
        // 235B-A22B may need verifying as single-file-vs-sharded before wiring into
        // an auto-downloader — same caveat as flagged for Qwen3-Coder-Next earlier.

        // --- 2B ---
        "qwen3-vl:2b-instruct" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-2B-Instruct-F16.gguf
            gguf_repo: "Qwen/Qwen3-VL-2B-Instruct-GGUF",
            gguf_file: "Qwen3VL-2B-Instruct-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-2B-Instruct",
        }),
        "qwen3-vl:2b-instruct-q8" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-2B-Instruct-Q8_0.gguf
            gguf_repo: "Qwen/Qwen3-VL-2B-Instruct-GGUF",
            gguf_file: "Qwen3VL-2B-Instruct-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-VL-2B-Instruct",
        }),
        "qwen3-vl:2b-thinking" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-2B-Thinking-F16.gguf
            gguf_repo: "Qwen/Qwen3-VL-2B-Thinking-GGUF",
            gguf_file: "Qwen3VL-2B-Thinking-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-2B-Thinking",
        }),
        "qwen3-vl:2b-thinking-q8" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-2B-Thinking-Q8_0.gguf
            gguf_repo: "Qwen/Qwen3-VL-2B-Thinking-GGUF",
            gguf_file: "Qwen3VL-2B-Thinking-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-VL-2B-Thinking",
        }),

        // --- 4B ---
        "qwen3-vl:4b-instruct" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-4B-Instruct-F16.gguf
            gguf_repo: "Qwen/Qwen3-VL-4B-Instruct-GGUF",
            gguf_file: "Qwen3VL-4B-Instruct-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-4B-Instruct",
        }),
        "qwen3-vl:4b-instruct-q8" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-4B-Instruct-Q8_0.gguf
            gguf_repo: "Qwen/Qwen3-VL-4B-Instruct-GGUF",
            gguf_file: "Qwen3VL-4B-Instruct-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-VL-4B-Instruct",
        }),
        "qwen3-vl:4b-thinking" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-4B-Thinking-F16.gguf
            gguf_repo: "Qwen/Qwen3-VL-4B-Thinking-GGUF",
            gguf_file: "Qwen3VL-4B-Thinking-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-4B-Thinking",
        }),
        "qwen3-vl:4b-thinking-q8" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-4B-Thinking-Q8_0.gguf
            gguf_repo: "Qwen/Qwen3-VL-4B-Thinking-GGUF",
            gguf_file: "Qwen3VL-4B-Thinking-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-VL-4B-Thinking",
        }),

        // --- 8B ---
        // (filenames here directly confirmed against the live HF file tree)
        "qwen3-vl:8b-instruct" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-8B-Instruct-F16.gguf
            gguf_repo: "Qwen/Qwen3-VL-8B-Instruct-GGUF",
            gguf_file: "Qwen3VL-8B-Instruct-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-8B-Instruct",
        }),
        "qwen3-vl:8b-instruct-q8" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-8B-Instruct-Q8_0.gguf
            gguf_repo: "Qwen/Qwen3-VL-8B-Instruct-GGUF",
            gguf_file: "Qwen3VL-8B-Instruct-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-VL-8B-Instruct",
        }),
        "qwen3-vl:8b-thinking" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-8B-Thinking-F16.gguf
            gguf_repo: "Qwen/Qwen3-VL-8B-Thinking-GGUF",
            gguf_file: "Qwen3VL-8B-Thinking-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-8B-Thinking",
        }),
        "qwen3-vl:8b-thinking-q8" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-8B-Thinking-Q8_0.gguf
            gguf_repo: "Qwen/Qwen3-VL-8B-Thinking-GGUF",
            gguf_file: "Qwen3VL-8B-Thinking-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-VL-8B-Thinking",
        }),

        // --- 32B ---
        "qwen3-vl:32b-instruct" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-32B-Instruct-F16.gguf
            gguf_repo: "Qwen/Qwen3-VL-32B-Instruct-GGUF",
            gguf_file: "Qwen3VL-32B-Instruct-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-32B-Instruct",
        }),
        "qwen3-vl:32b-instruct-q8" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-32B-Instruct-Q8_0.gguf
            gguf_repo: "Qwen/Qwen3-VL-32B-Instruct-GGUF",
            gguf_file: "Qwen3VL-32B-Instruct-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-VL-32B-Instruct",
        }),
        "qwen3-vl:32b-thinking" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-32B-Thinking-F16.gguf
            gguf_repo: "Qwen/Qwen3-VL-32B-Thinking-GGUF",
            gguf_file: "Qwen3VL-32B-Thinking-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-32B-Thinking",
        }),
        "qwen3-vl:32b-thinking-q8" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-32B-Thinking-Q8_0.gguf
            gguf_repo: "Qwen/Qwen3-VL-32B-Thinking-GGUF",
            gguf_file: "Qwen3VL-32B-Thinking-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-VL-32B-Thinking",
        }),

        // --- 30B-A3B (MoE) ---
        "qwen3-vl:30b-a3b-instruct" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-30B-A3B-Instruct-F16.gguf
            gguf_repo: "Qwen/Qwen3-VL-30B-A3B-Instruct-GGUF",
            gguf_file: "Qwen3VL-30B-A3B-Instruct-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-30B-A3B-Instruct",
        }),
        "qwen3-vl:30b-a3b-instruct-q8" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-30B-A3B-Instruct-Q8_0.gguf
            gguf_repo: "Qwen/Qwen3-VL-30B-A3B-Instruct-GGUF",
            gguf_file: "Qwen3VL-30B-A3B-Instruct-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-VL-30B-A3B-Instruct",
        }),
        "qwen3-vl:30b-a3b-thinking" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-30B-A3B-Thinking-F16.gguf
            gguf_repo: "Qwen/Qwen3-VL-30B-A3B-Thinking-GGUF",
            gguf_file: "Qwen3VL-30B-A3B-Thinking-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-30B-A3B-Thinking",
        }),
        "qwen3-vl:30b-a3b-thinking-q8" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-30B-A3B-Thinking-Q8_0.gguf
            gguf_repo: "Qwen/Qwen3-VL-30B-A3B-Thinking-GGUF",
            gguf_file: "Qwen3VL-30B-A3B-Thinking-Q8_0.gguf",
            base_repo: "Qwen/Qwen3-VL-30B-A3B-Thinking",
        }),

        // Gate behind explicit user confirmation regardless.
        "qwen3-vl:235b-a22b-instruct" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-235B-A22B-Instruct-Q8_0.gguf (per official usage example)
            gguf_repo: "Qwen/Qwen3-VL-235B-A22B-Instruct-GGUF",
            gguf_file: "Qwen3VL-235B-A22B-Instruct-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-235B-A22B-Instruct",
        }),
        "qwen3-vl:235b-a22b-thinking" => Some(OreAsset::Gguf {
            // mmproj: mmproj-Qwen3VL-235B-A22B-Thinking-F16.gguf
            gguf_repo: "Qwen/Qwen3-VL-235B-A22B-Thinking-GGUF",
            gguf_file: "Qwen3VL-235B-A22B-Thinking-Q4_K_M.gguf",
            base_repo: "Qwen/Qwen3-VL-235B-A22B-Thinking",
        }),

        // DEEPSEEK-R1
        // 3. REASONING & THINKING MODELS (The <think> tag generators)
        // DeepSeek-R1 Distilled onto Qwen architecture (The most popular right now)
        "deepseek-r1:7b" | "deepseek-r1-qwen:7b" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/DeepSeek-R1-Distill-Qwen-7B-GGUF",
            gguf_file: "DeepSeek-R1-Distill-Qwen-7B-Q4_K_M.gguf",
            base_repo: "deepseek-ai/DeepSeek-R1-Distill-Qwen-7B",
        }),
        "deepseek-r1:14b" | "deepseek-r1-qwen:14b" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/DeepSeek-R1-Distill-Qwen-14B-GGUF",
            gguf_file: "DeepSeek-R1-Distill-Qwen-14B-Q4_K_M.gguf",
            base_repo: "deepseek-ai/DeepSeek-R1-Distill-Qwen-14B",
        }),
        "deepseek-r1:32b" | "deepseek-r1-qwen:32b" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/DeepSeek-R1-Distill-Qwen-32B-GGUF",
            gguf_file: "DeepSeek-R1-Distill-Qwen-32B-Q4_K_M.gguf",
            base_repo: "deepseek-ai/DeepSeek-R1-Distill-Qwen-32B",
        }),

        // ==================== LLAMA FAMILY MODELS ====================
        // LLAMA 2 (The Classic)
        "llama2:7b" => Some(OreAsset::Gguf {
            gguf_repo: "TheBloke/Llama-2-7B-GGUF",
            gguf_file: "llama-2-7b.Q4_K_M.gguf",
            base_repo: "meta-llama/Llama-2-7b-hf",
        }),
        "llama2:7b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "TheBloke/Llama-2-7B-GGUF",
            gguf_file: "llama-2-7b.Q8_0.gguf",
            base_repo: "meta-llama/Llama-2-7b-hf",
        }),
        "llama2:13b" => Some(OreAsset::Gguf {
            gguf_repo: "TheBloke/Llama-2-13B-GGUF",
            gguf_file: "llama-2-13b.Q4_K_M.gguf",
            base_repo: "meta-llama/Llama-2-13b-hf",
        }),
        "llama2:13b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "TheBloke/Llama-2-13B-GGUF",
            gguf_file: "llama-2-13b.Q8_0.gguf",
            base_repo: "meta-llama/Llama-2-13b-hf",
        }),

        // LLAMA 3 (Original - First Major Release)
        // The Workhorses (8GB - 16GB VRAM)
        "llama3:8b" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/Meta-Llama-3-8B-Instruct-GGUF",
            gguf_file: "Meta-Llama-3-8B-Instruct-Q4_K_M.gguf",
            base_repo: "meta-llama/Meta-Llama-3-8B-Instruct",
        }),
        "llama3:8b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/Meta-Llama-3-8B-Instruct-GGUF",
            gguf_file: "Meta-Llama-3-8B-Instruct-Q8_0.gguf",
            base_repo: "meta-llama/Meta-Llama-3-8B-Instruct",
        }),
        "llama3:70b" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/Meta-Llama-3-70B-Instruct-GGUF",
            gguf_file: "Meta-Llama-3-70B-Instruct-Q4_K_M.gguf",
            base_repo: "meta-llama/Meta-Llama-3-70B-Instruct",
        }),
        // llama3:70b-q8 is not available in GGUF format. It is sharded. Will add support in future.

        // LLAMA 3.1 (Next-Gen with 128K Context)
        // The Workhorses (8GB - 16GB VRAM)
        "llama3.1:8b" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF",
            gguf_file: "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
            base_repo: "meta-llama/Llama-3.1-8B-Instruct",
        }),
        "llama3.1:8b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF",
            gguf_file: "Meta-Llama-3.1-8B-Instruct-Q8_0.gguf",
            base_repo: "meta-llama/Llama-3.1-8B-Instruct",
        }),
        "llama3.1:70b" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/Meta-Llama-3.1-70B-Instruct-GGUF",
            gguf_file: "Meta-Llama-3.1-70B-Instruct-Q4_K_M.gguf",
            base_repo: "meta-llama/Llama-3.1-70B-Instruct",
        }),
        // "llama3.1:70b-q8" is sharded and not available in GGUF format. Will add support in future.

        // LLAMA 3.2 (Mobile/Optimized + Multilingual)
        // The Tiny Models (Ultra-fast, fits anywhere)
        "llama3.2:1b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Llama-3.2-1B-Instruct-GGUF",
            gguf_file: "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
            base_repo: "unsloth/Llama-3.2-1B-Instruct",
        }),
        "llama3.2:1b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Llama-3.2-1B-Instruct-GGUF",
            gguf_file: "Llama-3.2-1B-Instruct-Q8_0.gguf",
            base_repo: "unsloth/Llama-3.2-1B-Instruct",
        }),
        "llama3.2:3b" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Llama-3.2-3B-Instruct-GGUF",
            gguf_file: "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
            base_repo: "unsloth/Llama-3.2-3B-Instruct",
        }),
        "llama3.2:3b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "unsloth/Llama-3.2-3B-Instruct-GGUF",
            gguf_file: "Llama-3.2-3B-Instruct-Q8_0.gguf",
            base_repo: "unsloth/Llama-3.2-3B-Instruct",
        }),

        // 4. LLAMA 3.2 VISION — Multimodal cross-attention (image + text → text).
        //
        // ⚠️ TWO STRUCTURAL ISSUES — READ BEFORE USING ⚠️
        //
        // A) MMPROJ REQUIREMENT: like Qwen3-VL, every Llama 3.2 Vision GGUF repo
        //    ships TWO required files:
        //      1. The LLM weights  → gguf_file below
        //      2. A vision encoder → "Llama-3.2-11B-Vision-Instruct-mmproj.f16.gguf"
        //    Without the mmproj file the model cannot process images at all.
        //    OreAsset::Gguf only has a single gguf_file slot; the same
        //    mmproj_file fix needed for Qwen3-VL applies here too.
        //
        // B) mllama ARCHITECTURE: Llama 3.2 Vision uses `mllama` (cross-attention
        //    multimodal), which is NOT supported by mainline llama.cpp as of mid-2025.
        //    It works in Ollama (Meta's private llama.cpp fork) but NOT in LM Studio
        //    (confirmed broken as of LM Studio 3.6) and may fail in other backends.
        //    Verify mllama support in your inference engine before loading.
        //
        // FILENAME QUIRK: the leafspark repo uses dot-separated quant extensions
        // ("…-Instruct.Q4_K_M.gguf") unlike the hyphen style used by bartowski
        // ("…-Instruct-Q4_K_M.gguf"). Do not conflate the two patterns.
        "llama3.2:11b-vision" => Some(OreAsset::Gguf {
            // mmproj: Llama-3.2-11B-Vision-Instruct-mmproj.f16.gguf
            gguf_repo: "leafspark/Llama-3.2-11B-Vision-Instruct-GGUF",
            gguf_file: "Llama-3.2-11B-Vision-Instruct.Q4_K_M.gguf",
            base_repo: "meta-llama/Llama-3.2-11B-Vision-Instruct",
        }),
        "llama3.2:11b-vision-q8" => Some(OreAsset::Gguf {
            // mmproj: Llama-3.2-11B-Vision-Instruct-mmproj.f16.gguf
            gguf_repo: "leafspark/Llama-3.2-11B-Vision-Instruct-GGUF",
            gguf_file: "Llama-3.2-11B-Vision-Instruct.Q8_0.gguf",
            base_repo: "meta-llama/Llama-3.2-11B-Vision-Instruct",
        }),
        // "llama3.2-vl:90b" is sharded and requires a separate mmproj file. Will be supported in future.

        // LLAMA 3.3 (Latest with improved reasoning)
        // The Heavyweight (24GB+ VRAM) - Best 70B model overall
        "llama3.3:70b" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/Llama-3.3-70B-Instruct-GGUF",
            gguf_file: "Llama-3.3-70B-Instruct-Q4_K_M.gguf",
            base_repo: "meta-llama/Llama-3.3-70B-Instruct",
        }),
        // "llama3.3:70b-q8" is sharded and not available in GGUF format. Will add support in future.

        // LLAMA GUARD 2 (Safety/Fine-tuning models)
        "llama-guard:8b" => Some(OreAsset::Gguf {
            gguf_repo: "QuantFactory/Meta-Llama-Guard-2-8B-GGUF",
            gguf_file: "Meta-Llama-Guard-2-8B.Q4_K_M.gguf",
            base_repo: "meta-llama/Meta-Llama-Guard-2-8B",
        }),
        "llama-guard:8b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "QuantFactory/Meta-Llama-Guard-2-8B-GGUF",
            gguf_file: "Meta-Llama-Guard-2-8B.Q8_0.gguf",
            base_repo: "meta-llama/Meta-Llama-Guard-2-8B",
        }),

        // -------------------------------------------------------------
        // ⚠️ ORE V0.1 LIMITATION: SHARDED & MULTIMODAL MODELS
        // -------------------------------------------------------------
        // Models like Llama-4 (Scout/Maverick) and Qwen3-VL ship as
        // multi-part SHARDED GGUF files and require separate mmproj
        // vision encoders.
        //
        // The ORE NativeDriver currently expects a single contiguous
        // Memory Map (`mmap`). Sharded models are temporarily disabled
        // in `ore pull` until Phase 4 (The FUSE Virtual Filesystem)
        // is implemented. Use the Ollama Engine to run these models.
        // -------------------------------------------------------------

        // DEEPSEEK-R1 DISTILL LLAMA (Reasoning distilled into Llama architecture)
        "deepseek-r1-llama:8b" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/DeepSeek-R1-Distill-Llama-8B-GGUF",
            gguf_file: "DeepSeek-R1-Distill-Llama-8B-Q4_K_M.gguf",
            base_repo: "deepseek-ai/DeepSeek-R1-Distill-Llama-8B",
        }),
        "deepseek-r1-llama:8b-q8" => Some(OreAsset::Gguf {
            gguf_repo: "bartowski/DeepSeek-R1-Distill-Llama-8B-GGUF",
            gguf_file: "DeepSeek-R1-Distill-Llama-8B-Q8_0.gguf",
            base_repo: "deepseek-ai/DeepSeek-R1-Distill-Llama-8B",
        }),

        // --- WASM RUNTIMES (The Inception Interpreters) ---
        "system-py" => Some(OreAsset::Wasm {
            // Using a highly-optimized WASI Python 3.11 build
            url: "https://github.com/astral-sh/python-wasm/releases/download/3.11.4/python-3.11.4-wasi_snapshot_preview1.wasm",
            folder: "runtimes",
            filename: "system-py.wasm",
        }),
        "system-js" => Some(OreAsset::Wasm {
            // QuickJS compiled to WASI
            url: "https://github.com/bytecodealliance/javy/releases/download/v0.1.0/javy.wasm",
            folder: "runtimes",
            filename: "system-js.wasm",
        }),

        // --- SYSTEM EMBEDDERS (SAFETENSORS) ---
        "system-embedder" => Some(OreAsset::Safetensors {
            repo: "nomic-ai/nomic-embed-text-v1.5",
        }),
        "all-minilm" => Some(OreAsset::Safetensors {
            repo: "sentence-transformers/all-MiniLM-L6-v2",
        }),
        _ => None,
    }
}

/// Streams a file from a URL directly to the disk with a professional progress bar
pub async fn download_with_progress(
    url: &str,
    dest: &Path,
    token: &Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let mut req = client.get(url);

    if let Some(t) = token.as_ref() {
        req = req.bearer_auth(t);
    }

    let res = req.send().await?;

    if res.status() == reqwest::StatusCode::UNAUTHORIZED
        || res.status() == reqwest::StatusCode::FORBIDDEN
    {
        println!(
            "\n{} {}",
            "[-]".red().bold(),
            "ACCESS DENIED: Hugging Face License Gate".red().bold()
        );

        if token.is_some() {
            println!("    [!] Your HF_TOKEN was detected, but access was still denied.");
            println!(
                "    This usually means you haven't clicked 'Agree' on the specific model's page,"
            );
            println!("    or your token is invalid/expired.\n");
        } else {
            println!(
                "    This model (e.g., Llama, Gemma) is gated by its creator and requires an HF_TOKEN."
            );
            println!(
                "    Fully open models like Qwen do NOT require this. (Try: `ore pull qwen2.5:1.5b`)\n"
            );
        }

        println!("    To unlock gated models:");
        println!(
            "    1. Go to the model's page on Hugging Face (e.g., huggingface.co/meta-llama) and click 'Agree'"
        );
        println!("    2. Get your access token from https://huggingface.co/settings/tokens");
        println!("    3. Set it in your terminal:");
        println!("       - Linux/macOS: export HF_TOKEN=\"your_token_here\"");
        println!("       - Windows:     $env:HF_TOKEN=\"your_token_here\"");
        println!("    4. Try `ore pull` again.\n");
        std::process::exit(1);
    }

    if !res.status().is_success() {
        return Err(format!("HTTP Error: {}", res.status()).into());
    }

    let total_size = res.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green}[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes:>7}/{total_bytes:7} ({bytes_per_sec}, ETA: {eta})")
            .unwrap()
            .progress_chars("=>-")
    );

    let mut file = fs::File::create(dest)?;
    let mut downloaded: u64 = 0;

    // Stream the data directly to the NVMe/SSD (Zero RAM bloat)
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        let new = min(downloaded + (chunk.len() as u64), total_size);
        downloaded = new;
        pb.set_position(new);
    }

    pb.finish_and_clear();
    Ok(())
}

/// Attempts to securely fetch the user's Hugging Face token if it exists
pub fn get_hf_token() -> Option<String> {
    std::env::var("HF_TOKEN").ok().or_else(|| {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        let token_path = Path::new(&home)
            .join(".cache")
            .join("huggingface")
            .join("token");
        fs::read_to_string(token_path)
            .ok()
            .map(|s| s.trim().to_string())
    })
}

pub fn build_secure_client() -> Client {
    let token_path = "../ore-server/ore-kernel.token";
    let auth_token = match fs::read_to_string(token_path) {
        Ok(t) => t,
        Err(_) => {
            println!(
                "{} FATAL: Could not read Kernel Security Token.",
                "[-]".red().bold()
            );
            println!("    Is the ORE Kernel running? Did you start `ore-server`?");
            exit(1);
        }
    };

    let mut headers = HeaderMap::new();
    let mut auth_value = HeaderValue::from_str(&format!("Bearer {}", auth_token)).unwrap();
    auth_value.set_sensitive(true);
    headers.insert(AUTHORIZATION, auth_value);

    Client::builder()
        .default_headers(headers)
        .build()
        .expect("Failed to build HTTP client")
}
