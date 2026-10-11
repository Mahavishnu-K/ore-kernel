use super::super::params::OreResponseMeta;
use crate::registry::NetworkRule;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub fn execute_ore_fetch<F>(
    method: &str,
    parsed_url: &reqwest::Url,
    headers: Option<&HashMap<String, String>>,
    body: &[u8],
    target_filename: &str,
    network_enabled: bool,
    localhost_access: bool,
    rules: &[NetworkRule],
    tmp_dir: &Path,
    on_headers: F,
) -> Result<OreResponseMeta, (i32, String)>
where
    F: FnOnce(&OreResponseMeta),
{
    // Global Network Checks
    if !network_enabled {
        crate::kprintln!("-> [FIREWALL] Network access globally disabled.");
        return Err((
            -1,
            "ORE Firewall: Network access globally disabled".to_string(),
        ));
    }

    // Security: Path Traversal Check
    let safe_filename = target_filename
        .strip_prefix(".ore_network/")
        .unwrap_or(target_filename);

    if safe_filename.contains('/') || safe_filename.contains('\\') || safe_filename.contains("..") {
        crate::kprintln!("-> [SANDBOX ERROR] Invalid filename. Path traversal blocked.");
        return Err((-1, "ORE Security: Path traversal blocked".to_string()));
    }

    let host_only = parsed_url.host_str().unwrap_or("");
    let is_local = host_only == "localhost"
        || host_only == "127.0.0.1"
        || host_only == "0.0.0.0"
        || host_only == "[::1]";

    if !localhost_access && is_local {
        crate::kprintln!("-> [FIREWALL] Localhost access is disabled.");
        return Err((
            -1,
            format!("ORE Firewall: Localhost access blocked for '{}'", host_only),
        ));
    }

    // Domain & Method Whitelist Check
    let mut is_allowed = false;
    for rule in rules {
        if rule.domain == host_only || rule.domain == "*" {
            if rule.allowed_methods.contains(&method.to_string())
                || rule.allowed_methods.contains(&"*".to_string())
            {
                is_allowed = true;
                break;
            } else {
                crate::kprintln!(
                    "-> [FIREWALL] Domain matched, but Method '{}' is FORBIDDEN. (Allowed: {:?})",
                    method,
                    rule.allowed_methods
                );
                return Err((
                    -2,
                    format!(
                        "ORE Firewall: Method '{}' not allowed for '{}'",
                        method, host_only
                    ),
                ));
            }
        }
    }

    if !is_allowed {
        crate::kprintln!("-> [FIREWALL] Domain '{}' is not whitelisted.", host_only);
        return Err((
            -1,
            format!("ORE Firewall: Domain '{}' is not whitelisted", host_only),
        ));
    }

    crate::kprintln!(
        "-> [SANDBOX APPROVED] Routing {} to {} safely via ORE...",
        method,
        parsed_url
    );

    // Execute Request
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            crate::kprintln!("-> [SANDBOX HTTP ERROR] Could not build HTTP client: {}", e);
            return Err((-3, format!("Could not build HTTP client: {}", e)));
        }
    };

    let http_method = match reqwest::Method::from_bytes(method.as_bytes()) {
        Ok(m) => m,
        Err(e) => {
            crate::kprintln!(
                "-> [SANDBOX HTTP ERROR] Invalid HTTP method '{}': {}",
                method,
                e
            );
            return Err((-2, format!("Invalid HTTP method: {}", method)));
        }
    };

    let mut request = client.request(http_method, parsed_url.clone());

    // Forward request headers
    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            if let (Ok(h_name), Ok(h_val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                request = request.header(h_name, h_val);
            }
        }
    }

    if !body.is_empty() {
        request = request.body(body.to_vec());
    }

    let mut response = match request.send() {
        Ok(res) => res,
        Err(e) => {
            crate::kprintln!("-> [SANDBOX HTTP ERROR] {}", e);
            return Err((-3, format!("HTTP request failed: {}", e)));
        }
    };

    // Extract Real Status & Response Headers & Cookies
    let status_u16 = response.status().as_u16();
    let status_text = response
        .status()
        .canonical_reason()
        .unwrap_or("OK")
        .to_string();

    let mut headers_map = HashMap::new();
    for (k, v) in response.headers().iter() {
        if let Ok(v_str) = v.to_str() {
            headers_map.insert(k.as_str().to_string(), v_str.to_string());
        }
    }

    let mut cookies_map = HashMap::new();
    for val in response.headers().get_all(reqwest::header::SET_COOKIE) {
        if let Ok(val_str) = val.to_str()
            && let Some(cookie_pair) = val_str.split(';').next()
        {
            let mut parts = cookie_pair.splitn(2, '=');
            if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                cookies_map.insert(name.trim().to_string(), value.trim().to_string());
            }
        }
    }

    let meta = OreResponseMeta {
        status: status_u16,
        status_text: status_text.clone(),
        headers: headers_map,
        cookies: cookies_map,
    };

    // Notify caller that response metadata is ready (unblocks guest for real-time streaming)
    on_headers(&meta);

    // Streaming to VFS with flush
    let file_dest = tmp_dir.join(target_filename);
    let done_dest = tmp_dir.join(format!("{}.done", target_filename));

    if let Some(parent) = file_dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut file = match std::fs::File::create(&file_dest) {
        Ok(f) => f,
        Err(e) => {
            crate::kprintln!(
                "-> [SANDBOX I/O ERROR] Error creating network request file: {}",
                e
            );
            let _ = std::fs::write(&done_dest, format!("error:{}", e));
            return Err((-4, format!("Error creating file: {}", e)));
        }
    };

    let mut buf = [0u8; 8192];
    loop {
        match response.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if let Err(e) = file.write_all(&buf[..n]) {
                    crate::kprintln!("-> [SANDBOX I/O ERROR] Failed to save chunk: {}", e);
                    let _ = std::fs::write(&done_dest, format!("error:{}", e));
                    return Err((-4, format!("Failed to write chunk: {}", e)));
                }
                let _ = file.flush();
            }
            Err(e) => {
                crate::kprintln!("-> [SANDBOX I/O ERROR] Stream interrupted: {}", e);
                let _ = std::fs::write(&done_dest, format!("error:{}", e));
                return Err((-3, format!("Stream interrupted: {}", e)));
            }
        }
    }
    let _ = file.flush();

    // Write stream completion marker
    let _ = std::fs::write(&done_dest, b"ok");

    crate::kprintln!(
        "-> [SANDBOX HTTP] Success ({} {}). Saved response securely to VFS as '{}'.",
        status_u16,
        status_text,
        target_filename
    );

    Ok(meta)
}
