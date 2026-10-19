use super::super::params::OreResponseMeta;
use crate::registry::NetworkRule;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

/// Max redirect hops; every hop is re-checked against the egress rules.
const MAX_REDIRECTS: usize = 10;

/// Loopback, unspecified, and link-local addresses (incl. cloud metadata at 169.254.169.254).
fn is_local_v4(ip: &Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_unspecified() || ip.is_link_local()
}

fn is_local_v6(ip: &Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_local_v4(&v4);
    }
    // fe80::/10 is link-local
    ip.is_loopback() || ip.is_unspecified() || (ip.segments()[0] & 0xffc0) == 0xfe80
}

/// True if the URL points at this machine (or its link-local network).
fn is_local_host(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };
    // IPv6 hosts come back bracketed ("[::1]"). The URL parser already normalizes
    // alternate IPv4 spellings such as "2130706433" or "0x7f.1" to dotted form.
    match host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        Ok(IpAddr::V4(ip)) => is_local_v4(&ip),
        Ok(IpAddr::V6(ip)) => is_local_v6(&ip),
        Err(_) => {
            let d = host.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost")
        }
    }
}

/// `"*"` allows every path, `"/prefix*"` allows paths starting with `/prefix`,
/// and a plain entry allows that exact path and everything below it.
fn path_allowed(path: &str, allowed_paths: &[String]) -> bool {
    allowed_paths.iter().any(|p| {
        if p == "*" {
            true
        } else if let Some(prefix) = p.strip_suffix('*') {
            path.starts_with(prefix)
        } else {
            let p = p.trim_end_matches('/');
            path == p || path.starts_with(&format!("{}/", p))
        }
    })
}

/// Checks one URL against the agent's egress policy. Used for the original request
/// and again for every redirect hop, so a whitelisted host can't bounce a request elsewhere.
pub(crate) fn check_egress(
    method: &str,
    url: &reqwest::Url,
    localhost_access: bool,
    rules: &[NetworkRule],
) -> Result<(), (i32, String)> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err((
            -1,
            format!("ORE Firewall: Scheme '{}' is not allowed", url.scheme()),
        ));
    }

    let host = url.host_str().unwrap_or("");

    if !localhost_access && is_local_host(url) {
        return Err((
            -1,
            format!("ORE Firewall: Localhost access blocked for '{}'", host),
        ));
    }

    let host_rules: Vec<&NetworkRule> = rules
        .iter()
        .filter(|r| r.domain == "*" || r.domain.eq_ignore_ascii_case(host))
        .collect();

    if host_rules.is_empty() {
        return Err((
            -1,
            format!("ORE Firewall: Domain '{}' is not whitelisted", host),
        ));
    }

    let method_rules: Vec<&NetworkRule> = host_rules
        .into_iter()
        .filter(|r| {
            r.allowed_methods
                .iter()
                .any(|m| m == "*" || m.eq_ignore_ascii_case(method))
        })
        .collect();

    if method_rules.is_empty() {
        return Err((
            -2,
            format!(
                "ORE Firewall: Method '{}' not allowed for '{}'",
                method, host
            ),
        ));
    }

    if !method_rules
        .iter()
        .any(|r| path_allowed(url.path(), &r.allowed_paths))
    {
        return Err((
            -1,
            format!(
                "ORE Firewall: Path '{}' not allowed for '{}'",
                url.path(),
                host
            ),
        ));
    }

    Ok(())
}

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

    // Domain, Method, Path & Localhost Checks
    if let Err((code, msg)) = check_egress(method, parsed_url, localhost_access, rules) {
        crate::kprintln!("-> [FIREWALL] {}", msg);
        return Err((code, msg));
    }

    crate::kprintln!(
        "-> [SANDBOX APPROVED] Routing {} to {} safely via ORE...",
        method,
        parsed_url
    );

    // Re-check every redirect hop against the same policy.
    let redirect_method = method.to_string();
    let redirect_rules = rules.to_vec();
    let redirect_policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            return attempt.error(format!(
                "ORE Firewall: Too many redirects (max {})",
                MAX_REDIRECTS
            ));
        }
        match check_egress(
            &redirect_method,
            attempt.url(),
            localhost_access,
            &redirect_rules,
        ) {
            Ok(()) => attempt.follow(),
            Err((_, msg)) => attempt.error(format!("Redirect blocked. {}", msg)),
        }
    });

    // Execute Request
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .redirect(redirect_policy)
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
            if e.is_redirect() {
                let msg = std::error::Error::source(&e)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| e.to_string());
                crate::kprintln!("-> [FIREWALL] {}", msg);
                return Err((-1, msg));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(domain: &str, methods: &[&str], paths: &[&str]) -> NetworkRule {
        NetworkRule {
            domain: domain.to_string(),
            allowed_methods: methods.iter().map(|s| s.to_string()).collect(),
            allowed_paths: paths.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn check(method: &str, url: &str, localhost: bool, rules: &[NetworkRule]) -> Result<(), i32> {
        let url = reqwest::Url::parse(url).unwrap();
        check_egress(method, &url, localhost, rules).map_err(|(code, _)| code)
    }

    #[test]
    fn allows_whitelisted_domain_method_and_path() {
        let rules = [rule("api.github.com", &["GET"], &["/repos*"])];
        assert!(check("GET", "https://api.github.com/repos/x/y", false, &rules).is_ok());
    }

    #[test]
    fn rejects_unlisted_domain_and_subdomain() {
        let rules = [rule("github.com", &["*"], &["*"])];
        assert_eq!(check("GET", "https://evil.com/", false, &rules), Err(-1));
        assert_eq!(
            check("GET", "https://api.github.com/", false, &rules),
            Err(-1)
        );
    }

    #[test]
    fn rejects_forbidden_method() {
        let rules = [rule("example.com", &["GET"], &["*"])];
        assert_eq!(
            check("POST", "https://example.com/", false, &rules),
            Err(-2)
        );
    }

    #[test]
    fn enforces_allowed_paths() {
        let rules = [rule("example.com", &["GET"], &["/api", "/static/*"])];
        assert!(check("GET", "https://example.com/api", false, &rules).is_ok());
        assert!(check("GET", "https://example.com/api/v1/users", false, &rules).is_ok());
        assert!(check("GET", "https://example.com/static/a.js", false, &rules).is_ok());
        assert_eq!(
            check("GET", "https://example.com/apiary", false, &rules),
            Err(-1)
        );
        assert_eq!(
            check("GET", "https://example.com/admin", false, &rules),
            Err(-1)
        );
        // ".." is normalized by the URL parser before the check
        assert_eq!(
            check("GET", "https://example.com/api/../admin", false, &rules),
            Err(-1)
        );
    }

    #[test]
    fn empty_allowed_paths_allows_nothing() {
        let rules = [rule("example.com", &["GET"], &[])];
        assert_eq!(check("GET", "https://example.com/", false, &rules), Err(-1));
    }

    #[test]
    fn blocks_local_addresses_even_with_wildcard_domain() {
        let rules = [rule("*", &["*"], &["*"])];
        for url in [
            "http://localhost/",
            "http://LOCALHOST./",
            "http://app.localhost/",
            "http://127.0.0.1/",
            "http://127.0.0.2/",
            "http://2130706433/", // 127.0.0.1 as a decimal integer
            "http://0.0.0.0/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[fe80::1]/",
        ] {
            assert_eq!(check("GET", url, false, &rules), Err(-1), "{}", url);
        }
        assert!(check("GET", "http://127.0.0.1/", true, &rules).is_ok());
    }

    #[test]
    fn rejects_non_http_schemes() {
        let rules = [rule("*", &["*"], &["*"])];
        assert_eq!(check("GET", "file:///etc/passwd", true, &rules), Err(-1));
    }

    /// Serves one request on 127.0.0.1 with a 302 to `location`; returns the base URL.
    fn redirect_once(location: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 302 Found
Location: {}
Content-Length: 0
Connection: close

",
                    location
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        format!("http://{}", addr)
    }

    #[test]
    fn redirects_are_rechecked_against_the_rules() {
        // The first hop is allowed (/start); the redirect target is not.
        let base = redirect_once("http://169.254.169.254/latest/meta-data/");
        let rules = [rule("127.0.0.1", &["GET"], &["/start"])];
        let url = reqwest::Url::parse(&format!("{}/start", base)).unwrap();
        let tmp = std::env::temp_dir();

        let result = execute_ore_fetch(
            "GET",
            &url,
            None,
            &[],
            "redirect_test.bin",
            true,
            true,
            &rules,
            &tmp,
            |_| {},
        );

        let (code, msg) = result.expect_err("redirect to a non-whitelisted host must fail");
        assert_eq!(code, -1);
        assert!(msg.contains("Redirect blocked"), "{}", msg);
    }
}
