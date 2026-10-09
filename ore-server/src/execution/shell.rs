pub fn execute_shell(cmd: &str) -> String {
    // SPAWN THE HOST PROCESS (Bypasses Sandbox entirely)
    // Automatically uses 'cmd.exe' for Windows, and 'sh' for Linux/macOS
    let output = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", cmd]).output()
    } else {
        std::process::Command::new("sh").arg("-c").arg(cmd).output()
    };

    // CAPTURE AND RETURN HOST OUTPUT
    match output {
        Ok(out) => {
            let mut final_output = String::from_utf8_lossy(&out.stdout).to_string();
            let error_output = String::from_utf8_lossy(&out.stderr).to_string();

            if !error_output.is_empty() {
                final_output.push_str("\n--- STDERR ---\n");
                final_output.push_str(&error_output);
            }

            crate::kprintln!("-> [SHELL SUCCESS] Output returned to Agent.");
            final_output
        }
        Err(e) => {
            crate::kprintln!("-> [SHELL FAILED] {}", e);
            format!("KERNEL ERROR: Host Shell execution failed: {}", e).to_string()
        }
    }
}
