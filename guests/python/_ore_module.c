// _ore_module.c - Embedded directly into system-py.wasm
#define PY_SSIZE_T_CLEAN
#include <Python.h>

// Declare the raw WebAssembly Host Imports from Rust Kernel
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

// Python Wrapper for Synchronous ore_fetch
static PyObject* py_ore_fetch(PyObject* self, PyObject* args) {
    const char *method, *url, *headers, *filename;
    Py_ssize_t method_len, url_len, headers_len, filename_len;
    Py_buffer body_buf;

    // Parses: (method: str, url: str, headers: str, body: bytes/str, filename: str)
    // s* accepts both string and raw binary bytes without copying
    if (!PyArg_ParseTuple(args, "s#s#s#s*s#", 
                          &method, &method_len, 
                          &url, &url_len, 
                          &headers, &headers_len, 
                          &body_buf, 
                          &filename, &filename_len)) {
        return NULL;
    }

    // Direct, instant CPU instruction trap to Rust! Zero polling!
    int status_code = ore_fetch(
        method, (int)method_len,
        url, (int)url_len,
        headers, (int)headers_len,
        (const char*)body_buf.buf, (int)body_buf.len,
        filename, (int)filename_len
    );

    PyBuffer_Release(&body_buf);
    return PyLong_FromLong(status_code);
}

// Python Wrapper for Asynchronous ore_fetch_async
static PyObject* py_ore_fetch_async(PyObject* self, PyObject* args) {
    const char *method, *url, *headers, *filename;
    Py_ssize_t method_len, url_len, headers_len, filename_len;
    Py_buffer body_buf;

    if (!PyArg_ParseTuple(args, "s#s#s#s*s#", 
                          &method, &method_len, 
                          &url, &url_len, 
                          &headers, &headers_len, 
                          &body_buf, 
                          &filename, &filename_len)) {
        return NULL;
    }

    int result = ore_fetch_async(
        method, (int)method_len,
        url, (int)url_len,
        headers, (int)headers_len,
        (const char*)body_buf.buf, (int)body_buf.len,
        filename, (int)filename_len
    );

    PyBuffer_Release(&body_buf);
    return PyLong_FromLong(result);
}

// Register Methods
static PyMethodDef OreMethods[] = {
    {"fetch", py_ore_fetch, METH_VARARGS, "Synchronous HTTP fetch via Ore Kernel"},
    {"fetch_async", py_ore_fetch_async, METH_VARARGS, "Asynchronous background HTTP fetch via Ore Kernel"},
    {NULL, NULL, 0, NULL}
};

static struct PyModuleDef oremodule = {
    PyModuleDef_HEAD_INIT,
    "_ore", // Module name in Python: import _ore
    "Ore Kernel WASM Linker Bridge",
    -1,
    OreMethods
};

PyMODINIT_FUNC PyInit__ore(void) {
    return PyModule_Create(&oremodule);
}