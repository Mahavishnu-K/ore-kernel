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

class ORE_HTTPError(Exception):
    """Base HTTP Error for all sandbox HTTP clients"""
    def __init__(self, message=None, response=None):
        super().__init__(message)
        self.response = response

class ORE_Response:
    def __init__(self, path, url="", meta=None):
        meta = meta or {}
        self.status_code = meta.get('status', 200)
        self.status = self.status_code
        self._client = 'requests'
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
            # Determine whether caller expects httpx or requests exception
            error_cls = ORE_HTTPError
            if hasattr(self, '_client') and self._client == 'httpx':
                error_cls = getattr(sys.modules.get('httpx'), 'HTTPStatusError', ORE_HTTPError)
            else:
                req_mod = sys.modules.get('requests')
                if req_mod and hasattr(req_mod, 'exceptions'):
                    error_cls = getattr(req_mod.exceptions, 'HTTPError', ORE_HTTPError)
            msg = f"{self.status_code} Error: {self.reason} for url: {self.url}"
            if 400 <= self.status_code < 500:
                msg = f"{self.status_code} Client Error: {self.reason} for url: {self.url}"
            elif 500 <= self.status_code < 600:
                msg = f"{self.status_code} Server Error: {self.reason} for url: {self.url}"
            err = error_cls(msg)
            err.response = self
            raise err

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
        class HTTPError(ORE_HTTPError): pass
        class ConnectionError(RequestException): pass
        class Timeout(RequestException): pass
        class URLRequired(RequestException): pass
        class TooManyRedirects(RequestException): pass

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
    def request(method, url, **kwargs): 
        res = ORE_Network_Portal.fetch(method, url, **kwargs)
        res._client = 'httpx'
        return res
    @staticmethod
    def get(url, **kwargs): return ORE_HTTPX_Module.request('GET', url, **kwargs)
    @staticmethod
    def post(url, **kwargs): return ORE_HTTPX_Module.request('POST', url, **kwargs)
    @staticmethod
    def put(url, **kwargs): return ORE_HTTPX_Module.request('PUT', url, **kwargs)
    @staticmethod
    def delete(url, **kwargs): return ORE_HTTPX_Module.request('DELETE', url, **kwargs)
    @staticmethod
    def patch(url, **kwargs): return ORE_HTTPX_Module.request('PATCH', url, **kwargs)
    @staticmethod
    def head(url, **kwargs): return ORE_HTTPX_Module.request('HEAD', url, **kwargs)
    @staticmethod
    def options(url, **kwargs): return ORE_HTTPX_Module.request('OPTIONS', url, **kwargs)

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
            res = ORE_Network_Portal.fetch(method, self._build_url(url), **kw)
            res._client = 'httpx'
            return res
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
            res = await ORE_Network_Portal.async_fetch(method, self._build_url(url), **kw)
            res._client = 'httpx'
            return res
        async def get(self, url, **kwargs): return await self.request('GET', url, **kwargs)
        async def post(self, url, **kwargs): return await self.request('POST', url, **kwargs)
        async def put(self, url, **kwargs): return await self.request('PUT', url, **kwargs)
        async def delete(self, url, **kwargs): return await self.request('DELETE', url, **kwargs)
        async def patch(self, url, **kwargs): return await self.request('PATCH', url, **kwargs)
        async def head(self, url, **kwargs): return await self.request('HEAD', url, **kwargs)
        async def options(self, url, **kwargs): return await self.request('OPTIONS', url, **kwargs)
        def stream(self, method, url, **kwargs):
            return ORE_HTTPX_Module._AsyncStreamContext(self, method, url, kwargs)

    class HTTPError(ORE_HTTPError): pass
    class RequestError(Exception): pass
    class HTTPStatusError(ORE_HTTPError): pass
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
