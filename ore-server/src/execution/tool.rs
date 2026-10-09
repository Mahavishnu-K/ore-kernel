use std::fs;
use std::path::{Path, PathBuf};

pub struct ToolPreparationContext {
    pub wasm_path: PathBuf,
    pub run_args: Vec<String>,
    pub input_data: Option<String>,
}

pub fn prepare_tool(
    base_dir: &Path,
    tool_name: &str,
    extra_args: Option<&Vec<String>>,
    input_data: Option<&String>,
) -> Result<ToolPreparationContext, String> {
    let mut run_args = vec![];
    let wasm_path = base_dir.join("tools").join(format!("{}.wasm", tool_name));

    run_args.push(tool_name.to_string()); // argv[0]

    let args_path = base_dir.join("tools").join(format!("{}.args", tool_name));
    if args_path.exists()
        && let Ok(default_args_str) = fs::read_to_string(&args_path)
    {
        for line in default_args_str.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                run_args.push(trimmed.to_string());
            }
        }
    }

    if let Some(args) = extra_args {
        run_args.extend(args.clone());
    }

    if !wasm_path.exists() {
        return Err(format!(
            "KERNEL ERROR: Tool binary '{}' not found. Run 'ore pull <tool>' or install the tool.",
            wasm_path.display()
        ));
    }

    Ok(ToolPreparationContext {
        wasm_path,
        run_args,
        input_data: input_data.cloned(),
    })
}
