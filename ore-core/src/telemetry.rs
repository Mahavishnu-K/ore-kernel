use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use sysinfo::System;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

static SYS_MONITOR: std::sync::LazyLock<Mutex<System>> = std::sync::LazyLock::new(|| {
    let mut sys = System::new_all();
    sys.refresh_all();
    Mutex::new(sys)
});

pub fn get_host_telemetry() -> HostTelemetry {
    let mut sys = SYS_MONITOR.lock().unwrap();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpus = sys.cpus();
    let cpu_brand = cpus
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string());
    let cpu_usage_pct = sys.global_cpu_usage();

    HostTelemetry {
        os_name: System::name().unwrap_or_else(|| "Unknown OS".to_string()),
        os_version: System::os_version().unwrap_or_default(),
        host_name: System::host_name().unwrap_or_default(),
        cpu_count: cpus.len(),
        cpu_brand,
        cpu_usage_pct,
        total_memory_mb: sys.total_memory() / (1024 * 1024),
        used_memory_mb: sys.used_memory() / (1024 * 1024),
        free_memory_mb: sys.available_memory() / (1024 * 1024),
        total_swap_mb: sys.total_swap() / (1024 * 1024),
        used_swap_mb: sys.used_swap() / (1024 * 1024),
        uptime_seconds: System::uptime(),
    }
}
