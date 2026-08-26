#[macro_export]
macro_rules! kprintln {
    ($($arg:tt)*) => {{
        // Format & Clean the Thread ID using the stable Debug trait
        let thread_desc = format!("{:?}", std::thread::current().id());
        let clean_thread = thread_desc.replace("ThreadId(", "T").replace(")", "");

        // Lock stdout manually to prevent thread write contention
        use std::io::Write;
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();

        // Tokio's Task ID implements Display natively! We print it directly.
        if let Some(task_id) = tokio::task::try_id() {
            let _ = write!(handle, "[{:<4} | Task:{:<3}] ", clean_thread, task_id);
        } else {
            let _ = write!(handle, "[{:<4} | Task:main] ", clean_thread);
        }
        let _ = writeln!(handle, $($arg)*);
    }};
}

use std::path::PathBuf;

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
        std::fs::create_dir_all(&ore_path).expect("FATAL: Failed to create ~/.ore directory.");
    }

    ore_path
}

pub mod driver;
pub mod external;
pub mod firewall;
pub mod ipc;
pub mod linker;
pub mod memory;
pub mod native;
pub mod registry;
pub mod sandbox;
pub mod scheduler;
