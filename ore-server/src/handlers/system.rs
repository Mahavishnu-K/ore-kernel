use crate::payloads::ExecuteRequest;
use crate::state::KernelState;
use axum::extract::{Json, Path, State};
use ore_core::crypto::KernelCrypto;
use ore_core::kprintln;
use ore_core::memory::Pager;
use ore_core::sandbox::ExecuteParams;
use std::fs;
use std::sync::Arc;

pub async fn health_check(State(state): State<Arc<KernelState>>) -> String {
    format!(
        "ORE Kernel is ALIVE. Powered by: {}",
        state.driver.engine_name()
    )
    .to_string()
}

pub async fn execute_tool(
    State(state): State<Arc<KernelState>>,
    Json(payload): Json<ExecuteRequest>,
) -> String {
    kprintln!(
        "-> [EXECUTION] Agent '{}' requested to run a sandbox.",
        payload.app_id,
    );

    let manifest = match state.registry.get_app(&payload.app_id) {
        Some(m) => m,
        None => {
            return format!(
                "KERNEL ALERT: Unregistered Agent '{}'. Access Denied.",
                payload.app_id
            );
        }
    };

    let has_wasm_tool =
        payload.tool_name.is_some() || payload.args.is_some() || payload.input_data.is_some();
    let has_wasm_script =
        payload.language.is_some() || payload.script.is_some() || payload.dependencies.is_some();
    let has_shell = payload.shell_command.is_some();

    if (has_wasm_tool as u8 + has_wasm_script as u8 + has_shell as u8) > 1 {
        kprintln!(
            "-> [BLOCKED] Ambiguous execution payload from Agent '{}'.",
            manifest.app_id
        );
        return "KERNEL ERROR: Ambiguous request. Choose one mode (tool, script, or shell)."
            .to_string();
    }

    // Raw host shell execution mode
    if let Some(cmd) = &payload.shell_command {
        // STRICT MANIFEST ENFORCEMENT
        if !manifest.execution.can_execute_shell {
            kprintln!(
                "-> [BLOCKED] Agent '{}' lacks raw SHELL execution permissions.",
                manifest.app_id
            );
            return "KERNEL ALERT: Permission Denied. can_execute_shell is false.".to_string();
        }

        kprintln!(
            "-> [WARN] Agent '{}' executing RAW HOST SHELL command...",
            manifest.app_id
        );

        // SPAWN THE HOST PROCESS (Bypasses Sandbox entirely)
        // Automatically uses 'cmd.exe' for Windows, and 'sh' for Linux/macOS
        let output = if cfg!(target_os = "windows") {
            std::process::Command::new("cmd").args(["/C", cmd]).output()
        } else {
            std::process::Command::new("sh").arg("-c").arg(cmd).output()
        };

        // CAPTURE AND RETURN HOST OUTPUT
        return match output {
            Ok(out) => {
                let mut final_output = String::from_utf8_lossy(&out.stdout).to_string();
                let error_output = String::from_utf8_lossy(&out.stderr).to_string();

                if !error_output.is_empty() {
                    final_output.push_str("\n--- STDERR ---\n");
                    final_output.push_str(&error_output);
                }

                kprintln!("-> [SHELL SUCCESS] Output returned to Agent.");
                final_output
            }
            Err(e) => {
                kprintln!("-> [SHELL FAILED] {}", e);
                format!("KERNEL ERROR: Host Shell execution failed: {}", e).to_string()
            }
        };
    }

    if !manifest.execution.can_execute_wasm {
        kprintln!(
            "-> [BLOCKED] Agent '{}' lacks WASM execution permissions.",
            manifest.app_id
        );
        return "KERNEL ALERT: Permission Denied. can_execute_wasm is false in manifest."
            .to_string();
    }

    let base_dir = match std::path::absolute(ore_core::get_ore_dir()) {
        Ok(p) => p,
        Err(e) => return format!("KERNEL ERROR: Cannot resolve ORE base dir: {}", e),
    };
    let wasm_path: std::path::PathBuf;
    let mut run_args = vec![];
    let mut inception_data = None;
    let mut dynamic_vfs_mounts = Vec::new();

    // Define a variable to hold the hash so we only calculate it ONCE.
    let mut resolved_req_hash: Option<String> = None;

    if let Some(script) = &payload.script {
        let lang = payload.language.as_deref().unwrap_or("python");
        kprintln!("-> [EXECUTION] Mode: Autonomous Script ({})", lang);

        if !manifest
            .execution
            .allowed_language_runtimes
            .contains(&lang.to_string())
            && !manifest
                .execution
                .allowed_language_runtimes
                .contains(&"*".to_string())
        {
            kprintln!(
                "-> [BLOCKED] Runtime '{}' is not in allowed_language_runtimes list.",
                lang
            );
            return format!(
                "KERNEL ALERT: Autonomous scripting in '{}' is not whitelisted. Add it to allowed_language_runtimes.",
                lang
            );
        }

        if lang == "python" || lang == "py" {
            wasm_path = base_dir.join("runtimes").join("system-py.wasm");

            // JIT PIP VENDORING FOR AUTONOMOUS SCRIPTS
            if let Some(deps) = &payload.dependencies
                && !deps.is_empty()
            {
                crate::kprintln!("-> [EXECUTION] AI requested dependencies: {:?}", deps);

                // Create a deterministic hash for this exact combination of packages
                let mut sorted_deps = deps.clone();
                sorted_deps.sort();
                let req_string = sorted_deps.join(",");

                // Mathematically perfect SHA-256
                let hash_bytes = KernelCrypto::sha256(req_string.as_bytes());
                // Native Rust Hex Conversion
                let req_hash: String = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect();

                resolved_req_hash = Some(req_hash.clone());

                let cache_dir = base_dir.join("cache").join("pip").join(&req_hash);

                // If it's not cached, tell the Host OS to download them!
                if !cache_dir.exists() {
                    crate::kprintln!("-> [KERNEL] Cache miss. Host OS downloading packages...");

                    if let Err(e) = fs::create_dir_all(&cache_dir) {
                        crate::kprintln!(
                            "-> [KERNEL ERROR] Failed to create pip cache directory '{}': {}",
                            cache_dir.display(),
                            e
                        );

                        return format!(
                            "KERNEL ERROR: Failed to create pip cache directory '{}': {}",
                            cache_dir.display(),
                            e
                        );
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
                        .args(deps); // Pass the AI's requested packages

                    let pip_cmd = match pip_install.output() {
                        Ok(output) => output,
                        Err(e) => {
                            let _ = fs::remove_dir_all(&cache_dir); // Cleanup
                            crate::kprintln!("-> [KERNEL ERROR] Failed to launch pip: {}", e);
                            return format!(
                                "KERNEL ERROR: Failed to launch pip: {}. \
                                Make sure Python and pip are installed and available in the ORE server PATH.",
                                e
                            );
                        }
                    };

                    if !pip_cmd.status.success() {
                        crate::kprintln!("-> [KERNEL ERROR] Failed to install AI dependencies.");
                        let _ = fs::remove_dir_all(&cache_dir); // Cleanup
                        return format!(
                            "KERNEL ERROR: Failed to resolve requirements: {}",
                            String::from_utf8_lossy(&pip_cmd.stderr)
                        );
                    }

                    // C-Extension Scanner (Fail-Fast Security)
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
                                    "KERNEL ALERT: The AI requested a package containing illegal C-Extensions ({}). Denied.",
                                    entry.path().display()
                                );

                                crate::kprintln!(
                                    "-> [JIT PIP] Stripping host binary to force Pure Python fallback: {}",
                                    entry.path().display()
                                );

                                // We don't crash! We just delete the illegal binary.
                                // Python will gracefully fall back to pure .py files!
                                let _ = std::fs::remove_file(entry.path());
                            }
                        }
                    }
                } else {
                    crate::kprintln!("-> [KERNEL] Packages found in ORE Cache.");
                }

                // Dynamically mount the Cache into the Sandbox!
                // We add the cache directory to `allowed_read_paths` so sandbox.rs automatically mounts it!
                dynamic_vfs_mounts.push(cache_dir.to_string_lossy().to_string());
            }

            let mut final_script = String::new();

            // We inject the Python VFS driver at the top of the script!
            final_script.push_str(r#"
import sys, os, json, time, random, asyncio, urllib.parse, selectors
from asyncio import events

# WASI ASYNCIO EVENT LOOP COMPATIBILITY SHIM (Bypasses socketpair completely)
try:
    class WasiSelector(selectors.BaseSelector):
        def __init__(self):
            super().__init__()
            self._map = {}
        def register(self, fileobj, events, data=None):
            key = selectors.SelectorKey(fileobj, 0, events, data)
            self._map[fileobj] = key
            return key
        def unregister(self, fileobj):
            return self._map.pop(fileobj, None)
        def select(self, timeout=None):
            if timeout and timeout > 0:
                time.sleep(timeout) # WASI poll_oneoff: Suspends VM without burning CPU fuel
            return []
        def get_map(self):
            return self._map
        def close(self):
            self._map.clear()
            super().close()

    class WasiEventLoop(asyncio.SelectorEventLoop):
        def __init__(self, selector=None):
            if selector is None:
                selector = WasiSelector()
            super().__init__(selector)

        def _make_self_pipe(self):
            # No-op: WASI has no socketpair; self-pipe is not needed for single-threaded WASM
            self._ssock = None
            self._csock = None
            self._internal_fds = 0

        def _close_self_pipe(self):
            pass

        def _write_to_self(self):
            pass

    class WasiEventLoopPolicy(events.BaseDefaultEventLoopPolicy):
        _loop_factory = WasiEventLoop

    asyncio.set_event_loop_policy(WasiEventLoopPolicy())

except Exception:
    pass

class CaseInsensitiveDict(dict):
    def __init__(self, data=None):
        super().__init__()
        self._store = {}
        if data:
            if isinstance(data, dict):
                for k, v in data.items():
                    self[k] = v
            elif isinstance(data, (list, tuple)):
                for k, v in data:
                    self[k] = v

    def __setitem__(self, key, value):
        self._store[str(key).lower()] = (key, value)
        super().__setitem__(str(key).lower(), value)

    def __getitem__(self, key):
        return super().__getitem__(str(key).lower())

    def __contains__(self, key):
        return super().__contains__(str(key).lower())

    def get(self, key, default=None):
        return super().get(str(key).lower(), default)

    def items(self):
        return [pair for pair in self._store.values()]

    def keys(self):
        return [pair[0] for pair in self._store.values()]

    def values(self):
        return [pair[1] for pair in self._store.values()]

class ORE_Network_Portal:
    @staticmethod
    def _prepare_payload(method, url, kwargs):
        headers = dict(kwargs.get('headers') or {})
        data = kwargs.get('data', '')
        json_data = kwargs.get('json', None)

        # 1. URL Query Parameter Serialization (params={'q': 'rust', 'page': 1})
        if kwargs.get('params'):
            params = kwargs['params']
            if isinstance(params, (dict, list, tuple)):
                query_str = urllib.parse.urlencode(params)
            else:
                query_str = str(params)
            if query_str:
                url = f"{url}?{query_str}" if '?' not in str(url) else f"{url}&{query_str}"

        # 2. Cookies Dictionary to Cookie Header
        if kwargs.get('cookies') and isinstance(kwargs['cookies'], dict):
            cookie_str = "; ".join(f"{k}={v}" for k, v in kwargs['cookies'].items())
            if 'Cookie' in headers:
                headers['Cookie'] = f"{headers['Cookie']}; {cookie_str}"
            elif 'cookie' in headers:
                headers['cookie'] = f"{headers['cookie']}; {cookie_str}"
            else:
                headers['Cookie'] = cookie_str

        # 3. JSON or URL-encoded Body
        if json_data is not None:
            data = json.dumps(json_data)
            headers['Content-Type'] = 'application/json'
        elif isinstance(data, dict):
            data = urllib.parse.urlencode(data)
            headers.setdefault('Content-Type', 'application/x-www-form-urlencoded')

        req_id = f"{int(time.time())}_{random.randint(0, 1000000)}"
        target_filename = f".ore_network/dl_{req_id}.bin"
        req_payload = json.dumps({
            "method": method.upper(),
            "url": str(url),
            "headers": headers,
            "body": str(data) if data else "",
            "filename": target_filename
        })

        req_file = f'/ore_tmp/.ore_network/req_{req_id}.json'
        res_file = f'/ore_tmp/.ore_network/res_{req_id}.bin'
        meta_file = f'/ore_tmp/.ore_network/res_{req_id}.meta'

        return req_payload, req_file, res_file, meta_file, target_filename, str(url)

    # Synchronous fetch
    @staticmethod
    def fetch(method, url, **kwargs):
        req_payload, req_file, res_file, meta_file, target_filename, resolved_url = ORE_Network_Portal._prepare_payload(method, url, kwargs)

        with open(req_file, 'w') as f:
            f.write(req_payload)

        attempts = 0
        while attempts < 3000:
            raw_res = None
            try:
                if os.path.exists(res_file) and os.path.getsize(res_file) > 0:
                    with open(res_file, 'rb') as f:
                        raw_res = f.read()
                    try: os.remove(res_file)
                    except Exception: pass
            except (OSError, IOError):
                pass

            if raw_res:
                if raw_res[0:1] != b'0':
                    raise Exception(f"ORE Firewall Blocked Request: {raw_res[1:].decode('utf-8', errors='ignore')}")

                # Read response metadata packet (status, headers, cookies)
                meta = {}
                meta_attempts = 0
                while meta_attempts < 200:
                    try:
                        if os.path.exists(meta_file):
                            with open(meta_file, 'r', encoding='utf-8') as f:
                                meta = json.loads(f.read())
                            try: os.remove(meta_file)
                            except Exception: pass
                            break
                    except Exception:
                        pass
                    time.sleep(0.005)
                    meta_attempts += 1

                return ORE_Response(f"/ore_tmp/{target_filename}", resolved_url, meta)

            time.sleep(0.01)
            attempts += 1

        raise Exception(f"ORE Network Portal Timeout on {url}")

    # Asynchronous fetch
    @staticmethod
    async def async_fetch(method, url, **kwargs):
        req_payload, req_file, res_file, meta_file, target_filename, resolved_url = ORE_Network_Portal._prepare_payload(method, url, kwargs)

        with open(req_file, 'w') as f:
            f.write(req_payload)

        attempts = 0
        while attempts < 3000:
            raw_res = None
            try:
                if os.path.exists(res_file) and os.path.getsize(res_file) > 0:
                    with open(res_file, 'rb') as f:
                        raw_res = f.read()
                    try: os.remove(res_file)
                    except Exception: pass
            except (OSError, IOError):
                pass

            if raw_res:
                if raw_res[0:1] != b'0':
                    raise Exception(f"ORE Firewall Blocked Request: {raw_res[1:].decode('utf-8', errors='ignore')}")

                # Read response metadata packet
                meta = {}
                meta_attempts = 0
                while meta_attempts < 200:
                    try:
                        if os.path.exists(meta_file):
                            with open(meta_file, 'r', encoding='utf-8') as f:
                                meta = json.loads(f.read())
                            try: os.remove(meta_file)
                            except Exception: pass
                            break
                    except Exception:
                        pass
                    await asyncio.sleep(0.005)
                    meta_attempts += 1

                return ORE_Response(f"/ore_tmp/{target_filename}", resolved_url, meta)

            await asyncio.sleep(0.01)
            attempts += 1

        raise Exception(f"ORE Network Portal Timeout on {url}")


class ORE_Response:
    def __init__(self, path, url="", meta=None):
        meta = meta or {}
        self.status_code = meta.get('status', 200)
        self.status = self.status_code
        self.ok = (200 <= self.status_code < 300)
        self.is_success = self.ok
        self._path = path
        self._done_path = f"{path}.done"
        self.url = url
        self.reason = meta.get('status_text', 'OK')
        self.reason_phrase = self.reason
        self.headers = CaseInsensitiveDict(meta.get('headers') or {'content-type': 'application/json'})
        self.cookies = meta.get('cookies') or {}
        self.encoding = 'utf-8'

    def _wait_done(self, timeout_secs=30):
        start = time.time()
        while time.time() - start < timeout_secs:
            try:
                if os.path.exists(self._done_path):
                    with open(self._done_path, 'r', encoding='utf-8', errors='ignore') as f:
                        content = f.read().strip()
                    if content.startswith('error:'):
                        raise Exception(f"ORE Streaming Error: {content[6:]}")
                    return True
            except (OSError, IOError):
                pass
            time.sleep(0.01)
        return False

    @property
    def text(self):
        self._wait_done()
        with open(self._path, 'r', encoding='utf-8', errors='replace') as f:
            return f.read()

    def json(self):
        return json.loads(self.text)

    @property
    def content(self):
        self._wait_done()
        with open(self._path, 'rb') as f:
            return f.read()

    def read(self):
        return self.content

    def raise_for_status(self):
        if not self.ok:
            if 400 <= self.status_code < 500:
                raise requests.exceptions.HTTPError(f"{self.status_code} Client Error: {self.reason} for url: {self.url}")
            elif 500 <= self.status_code < 600:
                raise requests.exceptions.HTTPError(f"{self.status_code} Server Error: {self.reason} for url: {self.url}")
            else:
                raise requests.exceptions.HTTPError(f"HTTP Error: {self.status_code} for url: {self.url}")

    # Real-Time Token Streaming / SSE Generators
    def iter_content(self, chunk_size=1, decode_unicode=False):
        c_size = chunk_size or 8192
        offset = 0
        attempts = 0
        while attempts < 3000:
            read_any = False
            try:
                if os.path.exists(self._path):
                    with open(self._path, 'rb') as f:
                        f.seek(offset)
                        chunk = f.read(c_size)
                        if chunk:
                            offset += len(chunk)
                            read_any = True
                            attempts = 0
                            yield chunk.decode('utf-8', errors='replace') if decode_unicode else chunk
            except (OSError, IOError):
                pass

            if not read_any:
                if os.path.exists(self._done_path):
                    try:
                        with open(self._path, 'rb') as f:
                            f.seek(offset)
                            chunk = f.read()
                            if chunk:
                                yield chunk.decode('utf-8', errors='replace') if decode_unicode else chunk
                    except Exception: pass
                    break
                time.sleep(0.01)
                attempts += 1

    def iter_lines(self, chunk_size=512, decode_unicode=True, delimiter=None):
        pending = ""
        for chunk in self.iter_content(chunk_size=chunk_size, decode_unicode=True):
            pending += chunk
            while '\n' in pending:
                line, pending = pending.split('\n', 1)
                line = line.rstrip('\r')
                if line:
                    yield line
        if pending:
            line = pending.rstrip('\r')
            if line:
                yield line

    # Async generators for httpx
    async def aiter_bytes(self, chunk_size=8192):
        c_size = chunk_size or 8192
        offset = 0
        attempts = 0
        while attempts < 3000:
            read_any = False
            try:
                if os.path.exists(self._path):
                    with open(self._path, 'rb') as f:
                        f.seek(offset)
                        chunk = f.read(c_size)
                        if chunk:
                            offset += len(chunk)
                            read_any = True
                            attempts = 0
                            yield chunk
            except (OSError, IOError):
                pass

            if not read_any:
                if os.path.exists(self._done_path):
                    try:
                        with open(self._path, 'rb') as f:
                            f.seek(offset)
                            chunk = f.read()
                            if chunk:
                                yield chunk
                    except Exception: pass
                    break
                await asyncio.sleep(0.01)
                attempts += 1

    async def aiter_lines(self):
        pending = ""
        async for chunk in self.aiter_bytes():
            pending += chunk.decode('utf-8', errors='replace')
            while '\n' in pending:
                line, pending = pending.split('\n', 1)
                line = line.rstrip('\r')
                if line:
                    yield line
        if pending:
            line = pending.rstrip('\r')
            if line:
                yield line

    async def aiter_text(self):
        async for chunk in self.aiter_bytes():
            yield chunk.decode('utf-8', errors='replace')

    # Lifecycle & context management
    def __enter__(self): return self
    def __exit__(self, *args): pass
    async def __aenter__(self): return self
    async def __aexit__(self, *args): pass
    def close(self): pass
    async def aclose(self): pass

# TRANSPARENT 'requests' MODULE SHIM
class ORE_Requests_Module:
    @staticmethod
    def request(method, url, **kwargs): return ORE_Network_Portal.fetch(method, url, **kwargs)
    @staticmethod
    def get(url, **kwargs): return ORE_Network_Portal.fetch('GET', url, **kwargs)
    @staticmethod
    def post(url, **kwargs): return ORE_Network_Portal.fetch('POST', url, **kwargs)
    @staticmethod
    def put(url, **kwargs): return ORE_Network_Portal.fetch('PUT', url, **kwargs)
    @staticmethod
    def delete(url, **kwargs): return ORE_Network_Portal.fetch('DELETE', url, **kwargs)
    @staticmethod
    def patch(url, **kwargs): return ORE_Network_Portal.fetch('PATCH', url, **kwargs)
    @staticmethod
    def head(url, **kwargs): return ORE_Network_Portal.fetch('HEAD', url, **kwargs)
    @staticmethod
    def options(url, **kwargs): return ORE_Network_Portal.fetch('OPTIONS', url, **kwargs)

    class Session:
        def __init__(self, *args, **kwargs):
            self.headers = {}
            self.cookies = {}
        def __enter__(self): return self
        def __exit__(self, *args): pass
        def request(self, method, url, **kwargs):
            kw = dict(kwargs)
            kw['headers'] = {**self.headers, **(kw.get('headers') or {})}
            if self.cookies and 'cookies' not in kw:
                kw['cookies'] = self.cookies
            return ORE_Network_Portal.fetch(method, url, **kw)
        def get(self, url, **kwargs): return self.request('GET', url, **kwargs)
        def post(self, url, **kwargs): return self.request('POST', url, **kwargs)
        def put(self, url, **kwargs): return self.request('PUT', url, **kwargs)
        def delete(self, url, **kwargs): return self.request('DELETE', url, **kwargs)
        def patch(self, url, **kwargs): return self.request('PATCH', url, **kwargs)
        def head(self, url, **kwargs): return self.request('HEAD', url, **kwargs)
        def options(self, url, **kwargs): return self.request('OPTIONS', url, **kwargs)

    class exceptions:
        class RequestException(Exception): pass
        class HTTPError(Exception): pass
        class ConnectionError(Exception): pass
        class Timeout(Exception): pass
        class URLRequired(Exception): pass
        class TooManyRedirects(Exception): pass

    Response = ORE_Response
    codes = type('Codes', (), {
        'ok': 200, 'created': 201, 'accepted': 202, 'no_content': 204,
        'bad_request': 400, 'unauthorized': 401, 'forbidden': 403, 'not_found': 404,
        'internal_server_error': 500
    })

sys.modules['requests'] = ORE_Requests_Module()

# TRANSPARENT 'httpx' MODULE SHIM (Sync Client + AsyncClient)
class ORE_HTTPX_Module:
    @staticmethod
    def request(method, url, **kwargs): return ORE_Network_Portal.fetch(method, url, **kwargs)
    @staticmethod
    def get(url, **kwargs): return ORE_Network_Portal.fetch('GET', url, **kwargs)
    @staticmethod
    def post(url, **kwargs): return ORE_Network_Portal.fetch('POST', url, **kwargs)
    @staticmethod
    def put(url, **kwargs): return ORE_Network_Portal.fetch('PUT', url, **kwargs)
    @staticmethod
    def delete(url, **kwargs): return ORE_Network_Portal.fetch('DELETE', url, **kwargs)
    @staticmethod
    def patch(url, **kwargs): return ORE_Network_Portal.fetch('PATCH', url, **kwargs)
    @staticmethod
    def head(url, **kwargs): return ORE_Network_Portal.fetch('HEAD', url, **kwargs)
    @staticmethod
    def options(url, **kwargs): return ORE_Network_Portal.fetch('OPTIONS', url, **kwargs)

    class _SyncStreamContext:
        def __init__(self, response):
            self.response = response
        def __enter__(self):
            return self.response
        def __exit__(self, *args):
            self.response.close()

    class _AsyncStreamContext:
        def __init__(self, client, method, url, kwargs):
            self.client = client
            self.method = method
            self.url = url
            self.kwargs = kwargs
            self.response = None
        async def __aenter__(self):
            self.response = await self.client.request(self.method, self.url, **self.kwargs)
            return self.response
        async def __aexit__(self, *args):
            if self.response:
                await self.response.aclose()

    class Client:
        def __init__(self, *args, **kwargs):
            self.headers = dict(kwargs.get('headers') or {})
            self.base_url = kwargs.get('base_url', '')
        def __enter__(self): return self
        def __exit__(self, *args): pass
        def _build_url(self, url):
            return f"{self.base_url.rstrip('/')}/{str(url).lstrip('/')}" if self.base_url else str(url)
        def request(self, method, url, **kwargs):
            kw = dict(kwargs)
            kw['headers'] = {**self.headers, **(kw.get('headers') or {})}
            return ORE_Network_Portal.fetch(method, self._build_url(url), **kw)
        def get(self, url, **kwargs): return self.request('GET', url, **kwargs)
        def post(self, url, **kwargs): return self.request('POST', url, **kwargs)
        def put(self, url, **kwargs): return self.request('PUT', url, **kwargs)
        def delete(self, url, **kwargs): return self.request('DELETE', url, **kwargs)
        def patch(self, url, **kwargs): return self.request('PATCH', url, **kwargs)
        def head(self, url, **kwargs): return self.request('HEAD', url, **kwargs)
        def options(self, url, **kwargs): return self.request('OPTIONS', url, **kwargs)
        def stream(self, method, url, **kwargs):
            resp = self.request(method, url, **kwargs)
            return ORE_HTTPX_Module._SyncStreamContext(resp)

    class AsyncClient:
        def __init__(self, *args, **kwargs):
            self.headers = dict(kwargs.get('headers') or {})
            self.base_url = kwargs.get('base_url', '')
        async def __aenter__(self): return self
        async def __aexit__(self, *args): pass
        def _build_url(self, url):
            return f"{self.base_url.rstrip('/')}/{str(url).lstrip('/')}" if self.base_url else str(url)
        async def request(self, method, url, **kwargs):
            kw = dict(kwargs)
            kw['headers'] = {**self.headers, **(kw.get('headers') or {})}
            return await ORE_Network_Portal.async_fetch(method, self._build_url(url), **kw)
        async def get(self, url, **kwargs): return await self.request('GET', url, **kwargs)
        async def post(self, url, **kwargs): return await self.request('POST', url, **kwargs)
        async def put(self, url, **kwargs): return await self.request('PUT', url, **kwargs)
        async def delete(self, url, **kwargs): return await self.request('DELETE', url, **kwargs)
        async def patch(self, url, **kwargs): return await self.request('PATCH', url, **kwargs)
        async def head(self, url, **kwargs): return await self.request('HEAD', url, **kwargs)
        async def options(self, url, **kwargs): return await self.request('OPTIONS', url, **kwargs)
        def stream(self, method, url, **kwargs):
            return ORE_HTTPX_Module._AsyncStreamContext(self, method, url, kwargs)

    class HTTPError(Exception): pass
    class RequestError(Exception): pass
    class HTTPStatusError(Exception): pass
    Response = ORE_Response
    Headers = CaseInsensitiveDict

sys.modules['httpx'] = ORE_HTTPX_Module()


# TRANSPARENT 'urllib.request' SHIM
import urllib.request
class ORE_Urllib_Response:
    def __init__(self, res):
        self.res = res
        self.status = res.status_code
        self.headers = res.headers
    def read(self): return self.res.content
    def decode(self, *args): return self.res.text
    def info(self): return self.res.headers
    def getcode(self): return self.status
    def __enter__(self): return self
    def __exit__(self, *args): pass

def ore_urlopen(url, data=None, timeout=None, **kwargs):
    method = 'POST' if data else 'GET'
    res = ORE_Network_Portal.fetch(method, url, data=data)
    return ORE_Urllib_Response(res)

urllib.request.urlopen = ore_urlopen

# ==================== AI AGENT SCRIPT BEGINS ====================

"#);

            // Append the actual AI script below our hijack
            final_script.push_str(script);

            run_args.push("python".to_string());
            run_args.push("/ore_tmp/inception.py".to_string()); // Point interpreter to VFS
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

            // THE ORE KERNEL ROUTER (Force custom VFS Polyfills)
            // A comprehensive, O(1) lookup table of every polyfill in your js_modules folder
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
            // 1. import { x } from 'fs'
            // 2. import fs from "node:fs"
            // 3. await import('fs')
            // 4. require('fs')
            let import_re = regex::Regex::new(
                r#"(?m)(import\s+(?:[a-zA-Z0-9_\{\}\*,\s]+\s+from\s+)?|import\s*\(\s*|require\s*\(\s*)['"](?:node:)?([a-zA-Z0-9_/-]+)['"](\s*\)?)"#
            ).unwrap();

            let routed_script = import_re
                .replace_all(script, |caps: &regex::Captures| {
                    let prefix = &caps[1]; // e.g., "import fs from "
                    let mut mod_name = &caps[2]; // e.g., "fs", "fs/promises", "axios"
                    let suffix = &caps[3]; // e.g., ")" for requires, or "" for imports

                    // Automatically route 'https' to your 'http.js' polyfill
                    if mod_name == "https" {
                        mod_name = "http";
                    }

                    // Only rewrite the path if it is one of our ORE polyfills!
                    if core_modules.contains(mod_name) {
                        format!("{}'/modules/{}.js'{}", prefix, mod_name, suffix)
                    } else {
                        caps[0].to_string() // Not a core module (e.g. 'lodash'), leave it untouched for NPM!
                    }
                })
                .to_string();

            // Inject an unhandled rejection catcher and full CJS require shim
            let cjs_bridge = r#"
import * as _ore_constants from '/modules/constants.js';
import * as _ore_http from '/modules/http.js';
import * as _ore_node_fetch from '/modules/node-fetch.js';
import * as _ore_fs from '/modules/fs.js';
import * as _ore_fs_promises from '/modules/fs/promises.js';
import * as _ore_path from '/modules/path.js';
import * as _ore_crypto from '/modules/crypto.js';
import * as _ore_buffer from '/modules/buffer.js';
import * as _ore_events from '/modules/events.js';
import * as _ore_util from '/modules/util.js';
import * as _ore_util_types from '/modules/util/types.js';
import * as _ore_os from '/modules/os.js';
import * as _ore_url from '/modules/url.js';
import * as _ore_stream from '/modules/stream.js';
import * as _ore_stream_promises from '/modules/stream/promises.js';
import * as _ore_stream_consumers from '/modules/stream/consumers.js';
import * as _ore_assert from '/modules/assert.js';
import * as _ore_qs from '/modules/querystring.js';
import * as _ore_process from '/modules/process.js';
import * as _ore_string_decoder from '/modules/string_decoder.js';
import * as _ore_timers from '/modules/timers.js';
import * as _ore_timers_promises from '/modules/timers/promises.js';
import * as _ore_punycode from '/modules/punycode.js';
import * as _ore_encoding from '/modules/encoding.js';

// Establish Node.js Core Globals
globalThis.nextTick = (fn, ...args) => {
    if (typeof queueMicrotask === 'function') {
        queueMicrotask(() => fn(...args));
    } else {
        Promise.resolve().then(() => fn(...args));
    }
};
globalThis.process = _ore_process.default || _ore_process;
globalThis.Buffer = _ore_buffer.Buffer || _ore_buffer.default?.Buffer || _ore_buffer;
globalThis.fetch = _ore_http.fetch;
globalThis.Headers = _ore_node_fetch.Headers;
globalThis.Request = _ore_node_fetch.Request;
globalThis.Response = _ore_node_fetch.Response;
globalThis.TextEncoder = _ore_encoding.TextEncoder || _ore_encoding.default?.TextEncoder;
globalThis.TextDecoder = _ore_encoding.TextDecoder || _ore_encoding.default?.TextDecoder;
globalThis.URL = _ore_url.URL || _ore_url.default?.URL;
globalThis.URLSearchParams = _ore_url.URLSearchParams || _ore_url.default?.URLSearchParams;
globalThis.setImmediate = globalThis.setImmediate || _ore_timers.setImmediate || ((fn, ...args) => setTimeout(fn, 0, ...args));
globalThis.clearImmediate = globalThis.clearImmediate || _ore_timers.clearImmediate || clearTimeout;

// The Master Dynamic Linker Table
const _ore_c_mods = {
    'constants': _ore_constants.default || _ore_constants,
    'http': _ore_http,
    'https': _ore_http,
    "node-fetch": _ore_node_fetch,
    'fs': _ore_fs,
    'fs/promises': _ore_fs_promises,
    'path': _ore_path,
    'path/posix': _ore_path,
    'crypto': _ore_crypto,
    'buffer': _ore_buffer,
    'events': _ore_events,
    'util': _ore_util,
    'util/types': _ore_util_types,
    'os': _ore_os,
    'url': _ore_url,
    'stream': _ore_stream,
    'stream/promises': _ore_stream_promises,
    'stream/consumers': _ore_stream_consumers,
    'assert': _ore_assert,
    'querystring': _ore_qs,
    'process': _ore_process,
    'string_decoder': _ore_string_decoder,
    'timers': _ore_timers,
    'timers/promises': _ore_timers_promises,
    'punycode': _ore_punycode,
    'encoding': _ore_encoding,
};

// Fallback Proxy for Unsupported Optional Built-ins (e.g. tty, zlib, cluster)
const _emptyProxy = new Proxy(() => false, {
    get: (target, prop) => {
        // Critical: Never report as a Thenable, or 'await require(...)' freezes forever!
        if (prop === 'then') return undefined;
        if (prop === Symbol.iterator) return undefined;
        if (prop === 'isatty' || prop === 'isIP') return () => false;
        if (prop === Symbol.toPrimitive) return () => '';
        if (prop === 'default') return _emptyProxy;
        return _emptyProxy;
    },
    apply: () => _emptyProxy,
    construct: () => _emptyProxy
});

// Universal CommonJS require() Bridge
globalThis.require = function(name) {
    if (typeof name !== 'string') return _emptyProxy;

    // Normalizes:
    // 1. 'node:fs'           -> 'fs'
    // 2. '/modules/fs.js'    -> 'fs' (Matches what your Rust regex injects!)
    // 3. 'fs/promises'       -> 'fs/promises'
    let clean = name.replace(/^node:/, '');
    if (clean.startsWith('/modules/')) {
        clean = clean.slice(9).replace(/\.js$/, '');
    }

    const m = _ore_c_mods[clean] || _ore_c_mods[name];
    if (m) return m.default || m;
    return _emptyProxy;
};
"#;

            // JAVASCRIPT: JIT NPM CACHING & ESBUILD INJECTION
            // Direct expression evaluation (Eliminates unused assignment warning & redundant clone)
            let final_script = if let Some(deps) = &payload.dependencies
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
                            return format!(
                                "KERNEL ERROR: Failed to launch npm: {}. \
                                Make sure Node.js/npm is installed and available in the ORE server PATH.",
                                e
                            );
                        }
                    };

                    if !npm_output.status.success() {
                        let _ = fs::remove_dir_all(&cache_dir);
                        crate::kprintln!(
                            "-> [JIT NPM ERROR] npm install failed.\nSTDOUT:\n{}\nSTDERR:\n{}",
                            String::from_utf8_lossy(&npm_output.stdout),
                            String::from_utf8_lossy(&npm_output.stderr),
                        );
                        return format!(
                            "KERNEL ERROR: Failed to install NPM dependencies:\n{}",
                            String::from_utf8_lossy(&npm_output.stderr)
                        );
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

                let dummy_file = base_dir
                    .join("runtimes")
                    .join("js_modules")
                    .join("_empty.js");

                let _ = std::fs::write(
                    &dummy_file,
                    r#"
const noop = () => false;
export const isatty = noop;
export const isIP = noop;
const proxy = new Proxy(noop, {
    get: (t, p) => (p === 'isatty' || p === 'isIP') ? noop : proxy,
    apply: () => proxy,
    construct: () => proxy
});
export default proxy;
"#,
                );

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

                    // let _ = std::fs::remove_file(entry_file);
                    // let _ = std::fs::remove_file(out_file);
                    let _ = fs::write(
                        cache_dir.join(format!("final_bundle_{}.js", run_id)),
                        format!("{}\n{}", cjs_bridge, bundled_code),
                    );

                    // Return the value directly to final_script
                    format!("{}\n{}", cjs_bridge, bundled_code)
                } else {
                    let _ = std::fs::remove_file(entry_file);
                    let _ = std::fs::remove_file(out_file);
                    return format!(
                        "KERNEL ERROR: JIT NPM Bundler failed: {}",
                        String::from_utf8_lossy(&build_res.stderr)
                    );
                }
            } else {
                format!("{}\n{}", cjs_bridge, routed_script)
            };

            run_args.push(format!("/ore_tmp/{}", filename));
            inception_data = Some((filename, final_script));
        } else {
            return format!("KERNEL ERROR: Unsupported language '{}'", lang);
        }
    } else if let Some(tool) = &payload.tool_name {
        kprintln!("-> [EXECUTION] Mode: Fixed Tool ({}.wasm)", tool);

        if !manifest.execution.allowed_tools.contains(tool)
            && !manifest.execution.allowed_tools.contains(&"*".to_string())
        {
            kprintln!("-> [BLOCKED] Tool '{}' is not in allowed_tools list.", tool);
            return format!(
                "KERNEL ALERT: Tool '{}' is not whitelisted in manifest. Add it to allowed_tools.",
                tool
            );
        }

        // LOAD THE CARTRIDGE ("The Console-Cartridge Architecture")
        // We look for the pre-compiled .wasm file in a local /tools directory
        wasm_path = base_dir.join("tools").join(format!("{}.wasm", tool));
        run_args.push(tool.clone()); // argv[0]

        let args_path = base_dir.join("tools").join(format!("{}.args", tool));
        if args_path.exists()
            && let Ok(default_args_str) = std::fs::read_to_string(&args_path)
        {
            // We read line-by-line so arguments with spaces don't get broken
            for line in default_args_str.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    run_args.push(trimmed.to_string());
                }
            }
        }

        if let Some(args) = &payload.args {
            run_args.extend(args.clone());
        }
    } else {
        return "KERNEL ERROR: Must provide either 'script' or 'tool_name'.".to_string();
    }

    if !wasm_path.exists() {
        return format!(
            "KERNEL ERROR: Tool binary '{}' not found. Run 'ore pull <tool>' or install the tool.",
            wasm_path.display()
        );
    }

    let wasm_binary = match fs::read(&wasm_path) {
        Ok(b) => b,
        Err(e) => return format!("KERNEL ERROR: Failed to read WASM binary: {}", e),
    };

    let resolve_path = |p: &String| -> String {
        let path = std::path::Path::new(p);
        if path.is_absolute() {
            p.clone()
        } else {
            // Anchor relative paths to the ORE Base Dir!
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
        tool_name: wasm_path.file_stem().unwrap().to_str().unwrap().to_string(),
        wasm_binary,
        fuel_limit: manifest.execution.max_cpu_instructions, // Dynamic fuel limit per manifest (Default: 5 Billion ≈ 2 seconds of pure compute)
        args: run_args,
        stdin: payload.input_data.map(|s| s.into_bytes()),
        inception: inception_data,
        allowed_read_paths: resolved_read_paths,
        allowed_write_paths: resolved_write_paths,
        network_enabled: manifest.network.network_enabled,
        allow_localhost_access: manifest.network.allow_localhost_access,
        network_rules: manifest.network.rules.clone(),
        dynamic_vfs_path: resolved_req_hash.map(|hash| format!("/workspace/{}", hash)),
        wasm_path: wasm_path.clone(),
    };

    let sandbox = state.sandbox.clone();

    let exec_result = tokio::task::spawn_blocking(move || sandbox.execute(params)).await;

    match exec_result {
        Ok(Ok(output)) => {
            ore_core::kprintln!("-> [EXECUTION SUCCESS] Output returned to Agent.");
            output
        }
        Ok(Err(e)) => {
            ore_core::kprintln!("-> [EXECUTION FAILED] {}", e);
            format!("KERNEL ERROR: {}", e).to_string()
        }
        Err(e) => {
            ore_core::kprintln!("-> [KERNEL PANIC] Sandbox thread crashed: {}", e);
            format!("KERNEL PANIC: {}", e).to_string()
        }
    }
}

pub async fn process_status(State(state): State<Arc<KernelState>>) -> String {
    match state.driver.get_running_models().await {
        Ok(models) => {
            let mut output = format!(
                "{:<25} | {:<12} | {:<12}\n",
                "MODEL", "TOTAL RAM", "GPU VRAM"
            );
            output.push_str("----------------------------------------------------------\n");

            if models.is_empty() {
                output.push_str("No models currently loaded in memory.\n");
            } else {
                for m in models {
                    output.push_str(&format!(
                        "{:<25} | {:<9} MB | {:<9} MB\n",
                        m.model_name,
                        m.size_bytes / 1024 / 1024,
                        m.size_vram_bytes / 1024 / 1024
                    ));
                }
            }
            output
        }
        Err(e) => format!("Kernel Error: {}", e),
    }
}

pub async fn list_models(State(state): State<Arc<KernelState>>) -> String {
    match state.driver.list_local_models().await {
        Ok(models) => {
            let mut output = format!("{:<25} | {:<10} | {}\n", "REPOSITORY", "SIZE", "UPDATED");
            output.push_str("------------------------------------------------------\n");
            if models.is_empty() {
                output.push_str("No models installed. Use 'ore pull <model>'.\n");
            } else {
                for m in models {
                    output.push_str(&format!(
                        "{:<25} | {:.2} GB   | {}\n",
                        m.name,
                        m.size_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
                        m.modified_at
                    ));
                }
            }
            output
        }
        Err(e) => format!("Kernel Error: {}", e),
    }
}

pub async fn expel_model(
    State(state): State<Arc<KernelState>>,
    Path(model_name): Path<String>,
) -> String {
    match state.driver.unload_model(&model_name).await {
        Ok(_) => format!(
            "SUCCESS: Model '{}' has been forcefully evicted from GPU VRAM.",
            model_name
        ),
        Err(e) => format!("KERNEL ERROR: {}", e),
    }
}

pub async fn pull_model(
    State(state): State<Arc<KernelState>>,
    Path(model_name): Path<String>,
) -> String {
    match state.driver.pull_model(&model_name).await {
        Ok(_) => format!("SUCCESS: Model '{}' installed.", model_name),
        Err(e) => format!("KERNEL ERROR: {}", e),
    }
}

pub async fn load_model(
    State(state): State<Arc<KernelState>>,
    Path(model_name): Path<String>,
) -> String {
    match state.driver.preload_model(&model_name).await {
        Ok(_) => format!("SUCCESS: Model '{}' loaded.", model_name),
        Err(e) => format!("KERNEL ERROR: {}", e),
    }
}

pub async fn list_agents(State(state): State<Arc<KernelState>>) -> String {
    let apps = state.registry.list_apps();

    let mut output = format!(
        "{:<20} | {:<10} | {:<20} | {:<10} | {}\n",
        "AGENT ID", "VERSION", "ALLOWED MODELS", "PRIORITY", "STATUS"
    );
    output.push_str(
        "----------------------------------------------------------------------------------\n",
    );

    if apps.is_empty() {
        output.push_str("No agents registered. Use 'ore manifest <name>' to create one.\n");
    } else {
        for app in apps {
            // 1. Handle Empty Models
            let models = if app.resources.allowed_models.is_empty() {
                "-".to_string()
            } else {
                app.resources.allowed_models.join(", ")
            };

            // Truncate if too long
            let models_disp = if models.len() > 17 {
                format!("{}...", &models[..17]).to_string()
            } else {
                models
            };

            // Handle Empty Priority
            // If the string is empty, show "-", otherwise UPPERCASE it.
            let priority = if app.resources.gpu_priority.trim().is_empty() {
                "-".to_string()
            } else {
                app.resources.gpu_priority.to_uppercase()
            };

            let status = if app.execution.can_execute_shell || !app.privacy.enforce_pii_redaction {
                "UNSAFE"
            } else if app.resources.allowed_models.is_empty() && !app.network.network_enabled {
                "DORMANT"
            } else {
                "SECURED"
            };

            output.push_str(&format!(
                "{:<20} | {:<10} | {:<20} | {:<10} | {}\n",
                app.app_id, app.version, models_disp, priority, status
            ));
        }
    }
    output
}

pub async fn list_manifests(State(state): State<Arc<KernelState>>) -> String {
    let apps = state.registry.list_apps();

    let mut output = format!(
        "{:<20} | {:<10} | {:<12} | {:<15} | {}\n",
        "MANIFEST FILE", "NETWORK", "FILE I/O", "EXECUTION", "PII SCRUBBING"
    );
    output.push_str(
        "------------------------------------------------------------------------------------\n",
    );

    if apps.is_empty() {
        output.push_str("No manifests found in /manifests directory.\n");
    } else {
        for app in apps {
            let can_read = !app.file_system.allowed_read_paths.is_empty();
            let can_write = !app.file_system.allowed_write_paths.is_empty();
            let fs_status = match (can_read, can_write) {
                (true, true) => "Read/Write",
                (true, false) => "Read-Only",
                (false, true) => "Write-Only",
                (false, false) => "Air-gapped",
            };

            let exec_status = if app.execution.can_execute_shell {
                "SHELL (RISK)"
            } else if app.execution.can_execute_wasm {
                "WASM Sandbox"
            } else {
                "Disabled"
            };

            let pii_status = if app.privacy.enforce_pii_redaction {
                "ACTIVE"
            } else {
                "OFF (RISK)"
            };

            output.push_str(&format!(
                "{:<20} | {:<10} | {:<12} | {:<15} | {}\n",
                format!("{}.toml", app.app_id),
                if app.network.network_enabled {
                    "ENABLED"
                } else {
                    "BLOCKED"
                },
                fs_status,
                exec_status,
                pii_status
            ));
        }
    }
    output
}

pub async fn compact_memory(
    State(state): State<Arc<KernelState>>,
    Path(app_id): Path<String>,
) -> String {
    kprintln!(
        "-> [KERNEL COMMAND] Manual Memory Compaction triggered for Agent '{}'",
        app_id
    );

    let manifest = match state.registry.get_app(&app_id) {
        Some(m) => m.clone(),
        None => return format!("KERNEL ERROR: Unregistered Agent '{}'.", app_id),
    };

    if !manifest.resources.json_history {
        return format!(
            "KERNEL ERROR: Agent '{}' does not use JSON history. Cannot compact.",
            app_id
        );
    }

    let history = Pager::page_in_history(&app_id);
    if history.len() <= 2 {
        return "SUCCESS: History is already too short to compact.".to_string();
    }

    let target_model = manifest
        .resources
        .allowed_models
        .first()
        .map(|s| s.as_str())
        .unwrap_or("llama3.2:1b");
    let lease = match state.scheduler.request_gpu(target_model, &app_id).await {
        Ok(l) => l,
        Err(e) => return format!("ORE KERNEL ALERT: GPU unavailable - {}", e),
    };

    let text_to_summarize = history
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<String>>()
        .join("\n");

    let summary_prompt = format!(
        "You are a system memory compressor. Condense the following conversation log into an ultra-short, dense summary. Keep ALL names, numbers, decisions, and strict facts. Discard all conversational filler. Output ONLY the raw facts in as few words as mathematically possible.\n\nRAW LOG:\n{}\n\nCOMPRESSED FACTS:",
        text_to_summarize
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let driver_clone = Arc::clone(&state.driver);
    let m_clone = target_model.to_string();
    let a_clone = app_id.clone();

    // Spawn the generation task
    tokio::spawn(async move {
        let _ = driver_clone
            .generate_text(&m_clone, &a_clone, false, &summary_prompt, None, tx, "")
            .await;
    });

    let mut summary = String::new();
    while let Some(word) = rx.recv().await {
        summary.push_str(&word);
    }

    drop(lease); // Release GPU

    let mut compacted_history = Vec::new();
    compacted_history.push(ore_core::memory::ContextMessage {
        role: "system".to_string(),
        content: format!(
            "You are a helpful AI assistant. Previous context summary:\n{}",
            summary.trim()
        ),
    });

    let len = history.len();
    compacted_history.push(history[len - 2].clone());
    compacted_history.push(history[len - 1].clone());

    // Overwrite the SSD files
    Pager::page_out_history(&app_id, &compacted_history);

    if manifest.resources.stateful_paging {
        Pager::delete_kv_cache(&app_id);
    }

    format!("SUCCESS: Memory for Agent '{}' manually compacted.", app_id).to_string()
}

pub async fn clear_memory(
    State(state): State<Arc<KernelState>>,
    Path(app_id): Path<String>,
) -> String {
    kprintln!(
        "-> [KERNEL COMMAND] Wiping SSD Memory for Agent '{}'",
        app_id
    );
    Pager::clear_page(&app_id);
    let _ = state.driver.invalidate_agent_cache(&app_id).await;
    format!(
        "SUCCESS: Memory for Agent '{}' has been wiped clean from SSD and RAM.",
        app_id
    )
    .to_string()
}

pub async fn top_telemetry(State(state): State<Arc<KernelState>>) -> String {
    let scheduler_status = state.scheduler.get_status().await;
    let apps_count = state.registry.list_apps().len();

    let mut output = "=== ORE KERNEL TELEMETRY ===\n".to_string();
    output.push_str(&format!("{:<20} | Status\n", "Subsystem"));
    output.push_str(&format!("{:<20} | ------\n", "-------------------"));
    output.push_str(&format!(
        "{:<20} | ACTIVE\n",
        format!("Driver ({})", state.driver.engine_name())
    ));
    output.push_str(&format!(
        "{:<20} | {}\n",
        "Scheduler (VRAM)", scheduler_status
    ));
    output.push_str(&format!("{:<20} | ENFORCING\n", "Context Firewall"));
    output.push_str(&format!("{:<20} | {}\n", "Connected Apps", apps_count));

    output
}

pub async fn kill_app(State(state): State<Arc<KernelState>>, Path(app_id): Path<String>) -> String {
    kprintln!(
        "-> [KERNEL COMMAND] SIGTERM received for Agent '{}'",
        app_id
    );
    let _ = state.driver.invalidate_agent_cache(&app_id).await;
    format!("SUCCESS: App '{}' context wiped from GPU Memory.", app_id).to_string()
}
