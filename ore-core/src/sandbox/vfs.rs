use super::params::ExecuteParams;
use super::state::TempDirGuard;
use anyhow::Result;
use ore_common::paths;
use std::path::{Path, PathBuf};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};

/// Strips Windows \\?\ UNC prefixes that break WASI path lookups
pub fn sanitize_unc(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    if let Some(stripped) = s.strip_prefix(r#"\\?\"#) {
        PathBuf::from(stripped)
    } else {
        p
    }
}

/// Sets up the ephemeral host temporary directory with UNC sanitization and RAII guard
pub fn setup_tmp_directory() -> Result<(PathBuf, TempDirGuard)> {
    let exec_id = uuid::Uuid::new_v4().to_string();
    let host_tmp_dir = paths::get_tmp_dir().join(&exec_id);
    std::fs::create_dir_all(&host_tmp_dir)?;

    let canon_tmp = std::fs::canonicalize(&host_tmp_dir).unwrap_or(host_tmp_dir);
    let sanitized = sanitize_unc(canon_tmp);

    let guard = TempDirGuard::new(sanitized.clone());
    Ok((sanitized, guard))
}

/// Configures WASI boundaries, arguments, pipes, mounts, and environment variables
pub fn build_wasi_context(
    host_tmp_dir: &Path,
    params: &ExecuteParams,
) -> Result<(WasiP1Ctx, MemoryOutputPipe, MemoryOutputPipe)> {
    // Write dynamic inception script to VFS if provided
    if let Some((filename, content)) = &params.inception {
        let script_path = host_tmp_dir.join(filename);
        std::fs::write(&script_path, content)?;
        crate::kprintln!(
            "-> [INCEPTION] Dynamic AI script materialized to VFS: /ore_tmp/{}",
            filename
        );
    }

    let stdout_buf = MemoryOutputPipe::new(10 * 1024 * 1024);
    let stderr_buf = MemoryOutputPipe::new(10 * 1024 * 1024);

    let mut wasi_builder = WasiCtxBuilder::new();

    wasi_builder
        .stdout(stdout_buf.clone())
        .stderr(stderr_buf.clone())
        .args(&params.args);

    if params
        .args
        .iter()
        .any(|arg| arg == "python" || arg.contains("system-py") || arg.ends_with(".py"))
    {
        wasi_builder.env("PYTHONUNBUFFERED", "1");

        let mut pypath = String::from("/packages:/app");
        if let Some(dyn_path) = &params.dynamic_vfs_path {
            pypath.push(':');
            pypath.push_str(dyn_path);
        }

        wasi_builder.env("PYTHONPATH", &pypath);

        crate::kprintln!(
            "-> [SANDBOX] Python runtime detected. Enabling unbuffered output and setting PYTHONPATH to '{}'.",
            pypath
        );
    }

    if let Some(input_bytes) = &params.stdin {
        let stdin_buf = MemoryInputPipe::new(bytes::Bytes::from(input_bytes.clone()));
        wasi_builder.stdin(stdin_buf);
    }

    match wasi_builder.preopened_dir(host_tmp_dir, "/ore_tmp", DirPerms::all(), FilePerms::all()) {
        Ok(_) => {
            crate::kprintln!("-> [SANDBOX] Mounted ephemeral network cache to '/ore_tmp'");
        }
        Err(e) => {
            crate::kprintln!(
                "-> [SANDBOX WARN] Failed to inject ephemeral network cache: {}",
                e
            );
        }
    }

    // HOST WRITE PATHS (Mounted inside /workspace)
    for path in &params.allowed_write_paths {
        std::fs::create_dir_all(path)?;

        let folder_name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("write_dir");

        let guest_path = format!("/workspace/{}", folder_name);
        let canon_path = std::fs::canonicalize(path).unwrap_or(PathBuf::from(path));
        let safe_path = sanitize_unc(canon_path);

        match wasi_builder.preopened_dir(&safe_path, &guest_path, DirPerms::all(), FilePerms::all())
        {
            Ok(_) => {
                crate::kprintln!(
                    "-> [SANDBOX] Mounted Host Write Path '{}' to Guest '{}'",
                    path,
                    guest_path
                );
            }
            Err(e) => {
                crate::kprintln!(
                    "-> [SANDBOX WARN] Failed to inject Write Path '{}': {}",
                    path,
                    e
                );
            }
        }
    }

    // GLOBAL STANDARD LIBRARY MOUNT (For JS/TS Node.js API Polyfills)
    if params.args.iter().any(|arg| arg == "quickjs") {
        let js_modules_dir = paths::get_runtimes_dir().join("modules");
        if js_modules_dir.exists() {
            let canon_modules = std::fs::canonicalize(&js_modules_dir).unwrap_or(js_modules_dir);
            let safe_modules = sanitize_unc(canon_modules);
            match wasi_builder.preopened_dir(
                &safe_modules,
                "/modules",
                DirPerms::READ,
                FilePerms::READ,
            ) {
                Ok(_) => crate::kprintln!(
                    "-> [SANDBOX] Mounted JS Standard Library (Node.js Polyfills) to '/modules'"
                ),
                Err(e) => {
                    crate::kprintln!("-> [SANDBOX WARN] Failed to mount JS Modules: {}", e)
                }
            }
        }

        wasi_builder.env("QUICKJS_MODULE_PATH", "/modules");
        wasi_builder.env("NODE_PATH", "/modules");
    }

    // HOST READ PATHS (STRICTLY READ-ONLY inside /workspace)
    for path in &params.allowed_read_paths {
        std::fs::create_dir_all(path).unwrap_or_default();

        let folder_name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("read_dir");

        let guest_path = format!("/workspace/{}", folder_name);
        let canon_path = std::fs::canonicalize(path).unwrap_or(PathBuf::from(path));
        let safe_path = sanitize_unc(canon_path);

        match wasi_builder.preopened_dir(&safe_path, &guest_path, DirPerms::READ, FilePerms::READ) {
            Ok(_) => {
                crate::kprintln!(
                    "-> [SANDBOX] Mounted Host Read Path (STRICT READ-ONLY): '{}' to '{}'",
                    path,
                    guest_path
                );
            }
            Err(e) => {
                crate::kprintln!(
                    "-> [SANDBOX WARN] Failed to inject Read-Only Path '{}': {}",
                    path,
                    e
                );
            }
        }
    }

    let wasi_ctx = wasi_builder.build_p1();
    Ok((wasi_ctx, stdout_buf, stderr_buf))
}
