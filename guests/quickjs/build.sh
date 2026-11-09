#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== Building Ore system-js.wasm (QuickJS-NG v0.18.0 + Native Linker) ==="

# Locate Zig compiler
ZIG_CMD="${ORE_ZIG:-$(which zig || echo "${HOME}/.ore/toolchains/zig-0.17.0/zig")}"
if [ ! -x "$(command -v "$ZIG_CMD")" ]; then
    echo "[-] Error: Zig compiler not found. Ensure Zig is on PATH or installed by Ore."
    exit 1
fi

# Clone pinned release v0.18.0 (avoids upstream master breakages)
if [ ! -d "quickjs-src" ]; then
    echo "[*] Cloning QuickJS-NG v0.18.0..."
    git clone --depth 1 --branch v0.18.0 https://github.com/quickjs-ng/quickjs.git quickjs-src
fi

# Copy bridge files
cp quickjs-ore.c quickjs-src/
cp quickjs-ore.h quickjs-src/

cd quickjs-src

# Reset qjs.c (and the shipped gen/ files, in case an older build overwrote them)
git checkout -- qjs.c gen/repl.c gen/standalone.c 2>/dev/null || true

# v0.18.0 ships gen/repl.c and gen/standalone.c; nothing to generate.

# Patch qjs.c: register the bridge inside JS_NewCustomContext, after its NULL check.
# The anchor occurs once in qjs.c, so every context qjs creates gets globalThis.__ore__.
echo "[*] Patching qjs.c..."
sed -i '1i #include "quickjs-ore.h"' qjs.c
sed -i '/js_init_module_std(ctx, "qjs:std");/i \    js_init_ore_bridge(ctx);' qjs.c
if [ "$(grep -c 'js_init_ore_bridge(ctx);' qjs.c)" != "1" ]; then
    echo "[-] Error: qjs.c patch did not apply exactly once. Upstream qjs.c changed?"
    exit 1
fi

# Compile to wasm32-wasi with required WASI clock & signal emulation and 2MB stack
echo "[*] Compiling with Zig to wasm32-wasi..."
$ZIG_CMD cc -target wasm32-wasi -O3 \
    -D_GNU_SOURCE \
    -D_WASI_EMULATED_PROCESS_CLOCKS \
    -D_WASI_EMULATED_SIGNAL \
    -Wl,-z,stack-size=2097152 \
    quickjs.c \
    libregexp.c \
    libunicode.c \
    dtoa.c \
    quickjs-libc.c \
    quickjs-ore.c \
    gen/repl.c \
    gen/standalone.c \
    qjs.c \
    -lwasi-emulated-process-clocks \
    -lwasi-emulated-signal \
    -o system-js.wasm

echo "[+] Successfully built system-js.wasm ($(du -h system-js.wasm | cut -f1))"

# Deploy artifact to both repo runtimes/ and ~/.ore/runtimes/
REPO_RUNTIMES="${SCRIPT_DIR}/../../runtimes"
mkdir -p "$REPO_RUNTIMES"
cp system-js.wasm "${REPO_RUNTIMES}/system-js.wasm"
echo "[+] Deployed to repo runtimes: ${REPO_RUNTIMES}/system-js.wasm"

GLOBAL_RUNTIMES="${HOME}/.ore/runtimes"
if [ -d "$GLOBAL_RUNTIMES" ]; then
    cp system-js.wasm "${GLOBAL_RUNTIMES}/system-js.wasm"
    echo "[+] Deployed to global runtimes: ${GLOBAL_RUNTIMES}/system-js.wasm"
fi