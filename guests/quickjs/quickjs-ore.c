#include "quickjs-ore.h"
#include <string.h>

// Raw 10-parameter WebAssembly Host Imports from ORE Kernel (shims.rs)
__attribute__((import_module("ore"), import_name("fetch")))
extern int ore_fetch(
    const char* method_ptr, int method_len,
    const char* url_ptr, int url_len,
    const char* headers_ptr, int headers_len,
    const char* body_ptr, int body_len,
    const char* filename_ptr, int filename_len
);

__attribute__((import_module("ore"), import_name("fetch_async")))
extern int ore_fetch_async(
    const char* method_ptr, int method_len,
    const char* url_ptr, int url_len,
    const char* headers_ptr, int headers_len,
    const char* body_ptr, int body_len,
    const char* filename_ptr, int filename_len
);

// Synchronous Bridge Function: __ore__.fetch(method, url, headers, body, filename)
static JSValue js_ore_fetch(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv) {
    if (argc < 5) {
        return JS_ThrowTypeError(ctx, "__ore__.fetch requires 5 arguments: (method, url, headers, body, filename)");
    }

    size_t method_len, url_len, headers_len, body_len, filename_len;

    const char *method = JS_ToCStringLen(ctx, &method_len, argv[0]);
    const char *url = JS_ToCStringLen(ctx, &url_len, argv[1]);
    const char *headers = JS_ToCStringLen(ctx, &headers_len, argv[2]);
    // Uses JS_ToCStringLen to protect binary bodies containing \0 bytes!
    const char *body = JS_ToCStringLen(ctx, &body_len, argv[3]);
    const char *filename = JS_ToCStringLen(ctx, &filename_len, argv[4]);

    if (!method || !url || !headers || !body || !filename) {
        if (method) JS_FreeCString(ctx, method);
        if (url) JS_FreeCString(ctx, url);
        if (headers) JS_FreeCString(ctx, headers);
        if (body) JS_FreeCString(ctx, body);
        if (filename) JS_FreeCString(ctx, filename);
        return JS_ThrowTypeError(ctx, "Failed to serialize arguments to __ore__.fetch");
    }

    int status = ore_fetch(
        method, (int)method_len,
        url, (int)url_len,
        headers, (int)headers_len,
        body, (int)body_len,
        filename, (int)filename_len
    );

    JS_FreeCString(ctx, method);
    JS_FreeCString(ctx, url);
    JS_FreeCString(ctx, headers);
    JS_FreeCString(ctx, body);
    JS_FreeCString(ctx, filename);

    return JS_NewInt32(ctx, status);
}

// Asynchronous Bridge Function: __ore__.fetchAsync(method, url, headers, body, filename)
static JSValue js_ore_fetch_async(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv) {
    if (argc < 5) {
        return JS_ThrowTypeError(ctx, "__ore__.fetchAsync requires 5 arguments: (method, url, headers, body, filename)");
    }

    size_t method_len, url_len, headers_len, body_len, filename_len;

    const char *method = JS_ToCStringLen(ctx, &method_len, argv[0]);
    const char *url = JS_ToCStringLen(ctx, &url_len, argv[1]);
    const char *headers = JS_ToCStringLen(ctx, &headers_len, argv[2]);
    const char *body = JS_ToCStringLen(ctx, &body_len, argv[3]);
    const char *filename = JS_ToCStringLen(ctx, &filename_len, argv[4]);

    if (!method || !url || !headers || !body || !filename) {
        if (method) JS_FreeCString(ctx, method);
        if (url) JS_FreeCString(ctx, url);
        if (headers) JS_FreeCString(ctx, headers);
        if (body) JS_FreeCString(ctx, body);
        if (filename) JS_FreeCString(ctx, filename);
        return JS_ThrowTypeError(ctx, "Failed to serialize arguments to __ore__.fetchAsync");
    }

    int res = ore_fetch_async(
        method, (int)method_len,
        url, (int)url_len,
        headers, (int)headers_len,
        body, (int)body_len,
        filename, (int)filename_len
    );

    JS_FreeCString(ctx, method);
    JS_FreeCString(ctx, url);
    JS_FreeCString(ctx, headers);
    JS_FreeCString(ctx, body);
    JS_FreeCString(ctx, filename);

    return JS_NewInt32(ctx, res);
}

// Inject globalThis.__ore__
void js_init_ore_bridge(JSContext *ctx) {
    JSValue global_obj = JS_GetGlobalObject(ctx);
    JSValue ore_obj = JS_NewObject(ctx);

    JS_SetPropertyStr(ctx, ore_obj, "fetch", JS_NewCFunction(ctx, js_ore_fetch, "fetch", 5));
    JS_SetPropertyStr(ctx, ore_obj, "fetchAsync", JS_NewCFunction(ctx, js_ore_fetch_async, "fetchAsync", 5));
    JS_SetPropertyStr(ctx, global_obj, "__ore__", ore_obj);

    JS_FreeValue(ctx, global_obj);
}