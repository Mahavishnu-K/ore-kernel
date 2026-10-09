use ore_core::crypto::KernelCrypto;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ScriptPreparationContext {
    pub wasm_path: PathBuf,
    pub run_args: Vec<String>,
    pub inception_data: Option<(String, String)>,
    pub resolved_req_hash: Option<String>,
    pub dynamic_vfs_mounts: Vec<String>,
}

pub fn prepare_script(
    base_dir: &Path,
    lang: &str,
    script: &str,
    dependencies: Option<&Vec<String>>,
) -> Result<ScriptPreparationContext, String> {
    let wasm_path;
    let mut run_args = vec![];
    let inception_data;
    let mut dynamic_vfs_mounts = Vec::new();
    let mut resolved_req_hash = None;

    // Embedded directly into the binary at zero runtime disk I/O cost:
    const PYTHON_PREAMBLE: &str = include_str!("../../shims/python/bootstrap.py");
    const CJS_BRIDGE: &str = include_str!("../../shims/javascript/commonjs.js");

    if lang == "python" || lang == "py" {
        wasm_path = base_dir.join("runtimes").join("system-py.wasm");

        // JIT PIP VENDORING FOR AUTONOMOUS SCRIPTS
        if let Some(deps) = dependencies
            && !deps.is_empty()
        {
            crate::kprintln!("-> [EXECUTION] AI requested dependencies: {:?}", deps);

            let mut sorted_deps = deps.clone();
            sorted_deps.sort();
            let req_string = sorted_deps.join(",");

            let hash_bytes = KernelCrypto::sha256(req_string.as_bytes());
            let req_hash: String = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect();

            resolved_req_hash = Some(req_hash.clone());
            let cache_dir = base_dir.join("cache").join("pip").join(&req_hash);

            if !cache_dir.exists() {
                crate::kprintln!("-> [KERNEL] Cache miss. Host OS downloading packages...");

                if let Err(e) = fs::create_dir_all(&cache_dir) {
                    crate::kprintln!(
                        "-> [KERNEL ERROR] Failed to create pip cache directory '{}': {}",
                        cache_dir.display(),
                        e
                    );

                    return Err(format!(
                        "KERNEL ERROR: Failed to create pip cache directory '{}': {}",
                        cache_dir.display(),
                        e
                    ));
                }

                let python_cmd = if cfg!(target_os = "windows") {
                    "python"
                } else {
                    "python3"
                };

                let cache_path = cache_dir.to_string_lossy().to_string();

                crate::kprintln!(
                    "-> [JIT PIP] Launching '{}' for dependencies: {:?}",
                    python_cmd,
                    deps
                );

                crate::kprintln!("-> [JIT PIP] Target directory: {}", cache_path);

                let mut pip_install = std::process::Command::new(python_cmd);
                pip_install
                    .args(["-m", "pip", "install", "--target", &cache_path])
                    .args(deps);

                let pip_cmd = match pip_install.output() {
                    Ok(output) => output,
                    Err(e) => {
                        let _ = fs::remove_dir_all(&cache_dir);
                        return Err(format!(
                            "KERNEL ERROR: Failed to launch pip: {}. Make sure Python and pip are installed.",
                            e
                        ));
                    }
                };

                if !pip_cmd.status.success() {
                    crate::kprintln!("-> [KERNEL ERROR] Failed to install AI dependencies.");
                    let _ = fs::remove_dir_all(&cache_dir);
                    return Err(format!(
                        "KERNEL ERROR: Failed to resolve requirements: {}",
                        String::from_utf8_lossy(&pip_cmd.stderr)
                    ));
                }

                for entry in walkdir::WalkDir::new(&cache_dir)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    if entry.path().is_file() {
                        let ext = entry
                            .path()
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("");
                        if ["so", "pyd", "dylib", "dll"].contains(&ext) {
                            crate::kprintln!(
                                "[KERNEL ALERT] Stripping illegal C-Extension: {}",
                                entry.path().display()
                            );
                            crate::kprintln!(
                                "-> [JIT PIP] Stripping host binary to force Pure Python fallback: {}",
                                entry.path().display()
                            );
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            } else {
                crate::kprintln!("-> [KERNEL] Packages found in ORE Cache.");
            }

            dynamic_vfs_mounts.push(cache_dir.to_string_lossy().to_string());
        }

        let mut final_script = String::with_capacity(PYTHON_PREAMBLE.len() + script.len() + 64);
        final_script.push_str(PYTHON_PREAMBLE);
        final_script.push('\n');
        final_script.push_str(script);

        run_args.push("python".to_string());
        run_args.push("/ore_tmp/inception.py".to_string());
        inception_data = Some(("inception.py".to_string(), final_script));
    } else if lang == "javascript" || lang == "js" || lang == "ts" || lang == "typescript" {
        wasm_path = base_dir.join("runtimes").join("system-js.wasm");
        run_args.push("quickjs".to_string());

        let ext = if lang.starts_with("ts") || lang == "typescript" {
            "ts"
        } else {
            "js"
        };
        let filename = format!("inception.{}", ext);

        let core_modules: std::collections::HashSet<&str> = [
            "assert",
            "buffer",
            "constants",
            "crypto",
            "encoding",
            "events",
            "fs",
            "fs/promises",
            "http",
            "https",
            "node-fetch",
            "os",
            "path",
            "process",
            "punycode",
            "querystring",
            "stream",
            "stream/consumers",
            "stream/promises",
            "string_decoder",
            "timers",
            "timers/promises",
            "url",
            "util",
            "util/types",
            "whatwg_url",
        ]
        .iter()
        .cloned()
        .collect();

        // A SINGLE Regex that catches all variations in one pass:
        // import { x } from 'fs'
        // import fs from "node:fs"
        // await import('fs')
        // require('fs')
        let import_re = regex::Regex::new(
            r#"(?m)(import\s+(?:[a-zA-Z0-9_\{\}\*,\s]+\s+from\s+)?|import\s*\(\s*|require\s*\(\s*)['"](?:node:)?([a-zA-Z0-9_/-]+)['"](\s*\)?)"#
        ).unwrap();

        let routed_script = import_re
            .replace_all(script, |caps: &regex::Captures| {
                let prefix = &caps[1];
                let mut mod_name = &caps[2];
                let suffix = &caps[3];

                if mod_name == "https" {
                    mod_name = "http";
                }

                if core_modules.contains(mod_name) {
                    format!("{}'/modules/{}.js'{}", prefix, mod_name, suffix)
                } else {
                    caps[0].to_string()
                }
            })
            .to_string();

        let final_script = if let Some(deps) = dependencies
            && !deps.is_empty()
        {
            crate::kprintln!("-> [JIT NPM] AI requested dependencies: {:?}", deps);

            let mut sorted = deps.clone();
            sorted.sort();
            let req_string = sorted.join(",");

            let hash_bytes = KernelCrypto::sha256(req_string.as_bytes());
            let req_hash: String = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect();
            let cache_dir = base_dir.join("cache").join("npm").join(&req_hash);

            if !cache_dir.exists() {
                crate::kprintln!("-> [JIT NPM] Cache miss. Host OS downloading packages...");
                fs::create_dir_all(&cache_dir).unwrap();
                fs::write(
                    cache_dir.join("package.json"),
                    r#"{"name":"ore-jit","version":"1.0.0"}"#,
                )
                .unwrap();

                let mut npm_install = if cfg!(target_os = "windows") {
                    let mut cmd = std::process::Command::new("cmd");
                    cmd.arg("/C").arg("npm").arg("install");
                    cmd
                } else {
                    let mut cmd = std::process::Command::new("npm");
                    cmd.arg("install");
                    cmd
                };

                npm_install.current_dir(&cache_dir);
                for dep in deps {
                    npm_install.arg(dep);
                }

                let npm_output = match npm_install.output() {
                    Ok(output) => output,
                    Err(e) => {
                        let _ = fs::remove_dir_all(&cache_dir);
                        crate::kprintln!("-> [JIT NPM ERROR] Failed to launch npm: {}", e);
                        return Err(format!(
                            "KERNEL ERROR: Failed to launch npm: {}. \
                                Make sure Node.js/npm is installed and available in the ORE server PATH.",
                            e
                        ));
                    }
                };

                if !npm_output.status.success() {
                    let _ = fs::remove_dir_all(&cache_dir);
                    crate::kprintln!(
                        "-> [JIT NPM ERROR] npm install failed.\nSTDOUT:\n{}\nSTDERR:\n{}",
                        String::from_utf8_lossy(&npm_output.stdout),
                        String::from_utf8_lossy(&npm_output.stderr),
                    );
                    return Err(format!(
                        "KERNEL ERROR: Failed to install NPM dependencies:\n{}",
                        String::from_utf8_lossy(&npm_output.stderr)
                    ));
                }
            } else {
                crate::kprintln!("-> [JIT NPM] Cache hit! Bypassing npm install.");
            }

            let run_id = uuid::Uuid::new_v4().to_string();
            let entry_file = cache_dir.join(format!("index_{}.{}", run_id, ext));
            let out_file = cache_dir.join(format!("bundle_{}.js", run_id));

            fs::write(&entry_file, &routed_script).unwrap();

            let mut esbuild = if cfg!(target_os = "windows") {
                let mut cmd = std::process::Command::new("cmd");
                cmd.arg("/C").arg("npx").arg("esbuild");
                cmd
            } else {
                let mut cmd = std::process::Command::new("npx");
                cmd.arg("esbuild");
                cmd
            };

            esbuild
                .current_dir(&cache_dir)
                .args([
                    &format!("index_{}.{}", run_id, ext),
                    "--bundle",
                    "--format=esm",
                    "--platform=neutral",
                    "--main-fields=module,main",
                ])
                .arg(format!("--outfile={}", out_file.to_string_lossy()));

            for core in core_modules.iter() {
                let polyfill_path = if *core == "https" {
                    "/modules/http.js".to_string()
                } else {
                    format!("/modules/{}.js", core)
                };
                esbuild.arg(format!("--alias:{}={}", core, polyfill_path));
                esbuild.arg(format!("--alias:node:{}={}", core, polyfill_path));
            }

            let unpolyfilled = [
                "tty",
                "zlib",
                "net",
                "tls",
                "dns",
                "child_process",
                "dgram",
                "readline",
                "http2",
                "vm",
                "v8",
                "worker_threads",
                "cluster",
                "repl",
                "perf_hooks",
                "async_hooks",
                "diagnostics_channel",
                "inspector",
                "trace_events",
                "wasi",
            ];

            for unp in unpolyfilled.iter() {
                esbuild.arg(format!("--alias:{}={}", unp, "/modules/_empty.js"));
                esbuild.arg(format!("--alias:node:{}={}", unp, "/modules/_empty.js"));
            }
            esbuild.arg("--external:/modules/*");

            let build_res = esbuild.output().unwrap();
            if build_res.status.success() {
                let bundled_code = std::fs::read_to_string(&out_file).unwrap();
                crate::kprintln!("-> [JIT NPM] Script successfully bundled.");
                let _ = std::fs::remove_file(entry_file);
                let _ = std::fs::remove_file(out_file);
                format!("{}\n{}", CJS_BRIDGE, bundled_code)
            } else {
                let _ = std::fs::remove_file(entry_file);
                let _ = std::fs::remove_file(out_file);
                return Err(format!(
                    "KERNEL ERROR: JIT NPM Bundler failed: {}",
                    String::from_utf8_lossy(&build_res.stderr)
                ));
            }
        } else {
            format!("{}\n{}", CJS_BRIDGE, routed_script)
        };

        run_args.push(format!("/ore_tmp/{}", filename));
        inception_data = Some((filename, final_script));
    } else {
        return Err(format!("KERNEL ERROR: Unsupported language '{}'", lang));
    }

    if !wasm_path.exists() {
        return Err(format!(
            "KERNEL ERROR: Tool binary '{}' not found. Run 'ore pull <tool>' or install the tool.",
            wasm_path.display()
        ));
    }

    Ok(ScriptPreparationContext {
        wasm_path,
        run_args,
        inception_data,
        resolved_req_hash,
        dynamic_vfs_mounts,
    })
}
