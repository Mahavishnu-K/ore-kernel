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