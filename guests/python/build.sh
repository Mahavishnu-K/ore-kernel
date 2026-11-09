#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== Building Ore system-py.wasm (CPython 3.12 + Native Linker + Stdlib) ==="

# Check Docker daemon availability
if ! docker info >/dev/null 2>&1; then
    echo "[-] Error: Docker daemon is not running. Please start Docker Desktop first."
    exit 1
fi

# 1. Build the image
docker build -f Dockerfile.build -t ore-python-wasm-builder .

# 2. Extract self-contained system-py.wasm
CONTAINER_ID=$(docker create ore-python-wasm-builder)
docker cp "${CONTAINER_ID}:/build/system-py.wasm" ./system-py.wasm
docker rm "${CONTAINER_ID}"

echo "[+] Successfully built system-py.wasm ($(du -h system-py.wasm | cut -f1))"

# 3. Deploy artifact to both repo runtimes/ and ~/.ore/runtimes/
REPO_RUNTIMES="${SCRIPT_DIR}/../../runtimes"
mkdir -p "$REPO_RUNTIMES"
cp system-py.wasm "${REPO_RUNTIMES}/system-py.wasm"
echo "[+] Deployed to repo runtimes: ${REPO_RUNTIMES}/system-py.wasm"

GLOBAL_RUNTIMES="${HOME}/.ore/runtimes"
if [ -d "$GLOBAL_RUNTIMES" ]; then
    cp system-py.wasm "${GLOBAL_RUNTIMES}/system-py.wasm"
    echo "[+] Deployed to global runtimes: ${GLOBAL_RUNTIMES}/system-py.wasm"
fi