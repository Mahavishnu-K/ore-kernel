pub mod script;
pub mod shell;
pub mod tool;

use crate::payloads::{ExecuteRequest, ExecutionMode};
use crate::state::KernelState;
use ore_core::sandbox::ExecuteParams;
use std::fs;
use std::sync::Arc;

pub async fn execute_request(
    state: Arc<KernelState>,
    payload: ExecuteRequest,
) -> Result<String, String> {
    crate::kprintln!(
        "-> [EXECUTION] Agent '{}' requested to run a sandbox.",
        payload.app_id,
    );

    let manifest = match state.registry.get_app(&payload.app_id) {
        Some(m) => m,
        None => {
            return Err(format!(
                "KERNEL ALERT: Unregistered Agent '{}'. Access Denied.",
                payload.app_id
            ));
        }
    };

    let mode = match payload.execution_mode() {
        Ok(m) => m,
        Err(e) => return Err(format!("KERNEL ERROR: {}", e)),
    };

    let base_dir = match std::path::absolute(ore_core::get_ore_dir()) {
        Ok(p) => p,
        Err(e) => return Err(format!("KERNEL ERROR: Cannot resolve ORE base dir: {}", e)),
    };

    // Shell Execution Bypass
    if let ExecutionMode::Shell { command } = &mode {
        if !manifest.execution.can_execute_shell {
            crate::kprintln!(
                "-> [BLOCKED] Agent '{}' lacks raw SHELL execution permissions.",
                manifest.app_id
            );
            return Err("KERNEL ALERT: Permission Denied. can_execute_shell is false.".to_string());
        }

        crate::kprintln!(
            "-> [WARN] Agent '{}' executing RAW HOST SHELL command...",
            manifest.app_id
        );

        return Ok(shell::execute_shell(command));
    }

    if !manifest.execution.can_execute_wasm {
        crate::kprintln!(
            "-> [BLOCKED] Agent '{}' lacks WASM execution permissions.",
            manifest.app_id
        );
        return Err(
            "KERNEL ALERT: Permission Denied. can_execute_wasm is false in manifest.".to_string(),
        );
    }

    let wasm_path;
    let run_args;
    let input_data;
    let mut inception_data = None;
    let mut dynamic_vfs_mounts = Vec::new();
    let mut dynamic_vfs_path = None;
    let tool_name: String;

    match &mode {
        ExecutionMode::Tool {
            name,
            args,
            input_data: i_data,
        } => {
            if !manifest.execution.allowed_tools.contains(name)
                && !manifest.execution.allowed_tools.contains(&"*".to_string())
            {
                return Err(format!(
                    "KERNEL ALERT: Tool '{}' is not whitelisted in manifest. Add it to allowed_tools.",
                    name
                ));
            }

            let ctx = tool::prepare_tool(&base_dir, name, Some(args), i_data.as_ref())?;
            wasm_path = ctx.wasm_path;
            run_args = ctx.run_args;
            input_data = ctx.input_data;
            tool_name = name.clone();
        }
        ExecutionMode::Script {
            language,
            script,
            dependencies,
            input_data: i_data,
        } => {
            if !manifest
                .execution
                .allowed_language_runtimes
                .contains(language)
                && !manifest
                    .execution
                    .allowed_language_runtimes
                    .contains(&"*".to_string())
            {
                return Err(format!(
                    "KERNEL ALERT: Autonomous scripting in '{}' is not whitelisted. Add it to allowed_language_runtimes.",
                    language
                ));
            }

            let ctx = script::prepare_script(&base_dir, language, script, Some(dependencies))?;
            wasm_path = ctx.wasm_path;
            run_args = ctx.run_args;
            inception_data = ctx.inception_data;
            dynamic_vfs_mounts = ctx.dynamic_vfs_mounts;
            dynamic_vfs_path = ctx
                .resolved_req_hash
                .map(|hash| format!("/workspace/{}", hash));
            input_data = i_data.clone();
            tool_name = wasm_path.file_stem().unwrap().to_str().unwrap().to_string();
        }
        ExecutionMode::Shell { .. } => unreachable!(),
    }

    let wasm_binary = match fs::read(&wasm_path) {
        Ok(b) => b,
        Err(e) => return Err(format!("KERNEL ERROR: Failed to read WASM binary: {}", e)),
    };

    let resolve_path = |p: &String| -> String {
        let path = std::path::Path::new(p);
        if path.is_absolute() {
            p.clone()
        } else {
            base_dir.join(path).to_string_lossy().to_string()
        }
    };

    let mut resolved_read_paths: Vec<String> = manifest
        .file_system
        .allowed_read_paths
        .iter()
        .map(resolve_path)
        .collect();

    resolved_read_paths.extend(dynamic_vfs_mounts);

    let resolved_write_paths: Vec<String> = manifest
        .file_system
        .allowed_write_paths
        .iter()
        .map(resolve_path)
        .collect();

    let params = ExecuteParams {
        tool_name,
        wasm_binary,
        fuel_limit: manifest.execution.max_cpu_instructions,
        args: run_args,
        stdin: input_data.map(|s| s.into_bytes()),
        inception: inception_data,
        allowed_read_paths: resolved_read_paths,
        allowed_write_paths: resolved_write_paths,
        network_enabled: manifest.network.network_enabled,
        allow_localhost_access: manifest.network.allow_localhost_access,
        network_rules: manifest.network.rules.clone(),
        dynamic_vfs_path,
        wasm_path,
    };

    let sandbox = state.sandbox.clone();

    let exec_result = tokio::task::spawn_blocking(move || sandbox.execute(params)).await;

    match exec_result {
        Ok(Ok(output)) => {
            ore_core::kprintln!("-> [EXECUTION SUCCESS] Output returned to Agent.");
            Ok(output)
        }
        Ok(Err(e)) => {
            ore_core::kprintln!("-> [EXECUTION FAILED] {}", e);
            Err(format!("KERNEL ERROR: {}", e))
        }
        Err(e) => {
            ore_core::kprintln!("-> [KERNEL PANIC] Sandbox thread crashed: {}", e);
            Err(format!("KERNEL PANIC: {}", e))
        }
    }
}
