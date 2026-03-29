"""
ore_client.py - Lightweight Python client for the ORE Kernel API.

Usage:
    from ore_client import OreClient

    ore = OreClient()
    response = ore.run("qwen2.5:0.5b", "Hello, world!")
    print(response)
"""

import os
import sys
import requests

# ─── Configuration ───────────────────────────────────────────────

ORE_BASE_URL = os.environ.get("ORE_URL", "http://127.0.0.1:3000")
TOKEN_PATHS = [
    os.path.join("..", "ore-server", "ore-kernel.token"),   # from examples/
    os.path.join("ore-server", "ore-kernel.token"),         # from repo root
    os.path.join("..", "..", "ore-server", "ore-kernel.token"),  # from examples/subdir/
    "ore-kernel.token",                                     # current dir
]


class OreClient:
    """Minimal HTTP client for the ORE Kernel."""

    def __init__(self, base_url: str = None):
        self.base_url = (base_url or ORE_BASE_URL).rstrip("/")
        self.token = self._read_token()
        self.headers = {
            "Authorization": f"Bearer {self.token}",
            "Content-Type": "application/json",
        }

    # ─── Authentication ──────────────────────────────────────────

    def _read_token(self) -> str:
        for path in TOKEN_PATHS:
            try:
                with open(path, "r") as f:
                    return f.read().strip()
            except FileNotFoundError:
                continue

        print("ERROR: Could not find ore-kernel.token.")
        print("Make sure the ORE Kernel is running (cargo run -p ore-server).")
        sys.exit(1)

    # ─── Inference ───────────────────────────────────────────────

    def run(self, model: str, prompt: str, stream: bool = False) -> str:
        """Executes a prompt against the ORE Kernel."""
        payload = {
            "model": model,
            "prompt": prompt
        }
        
        response = requests.post(f"{self.base_url}/run", json=payload, headers=self.headers, stream=True)
        
        if response.status_code != 200:
            print(f"\n[!] ORE KERNEL ERROR: {response.status_code}")
            print(response.text)
            return response.text

        full_text = ""
        
        for chunk in response.iter_content(chunk_size=1024):
            if chunk:
                text_chunk = chunk.decode('utf-8', errors='replace')
                
                if stream:
                    # Print to terminal without a newline, and flush immediately
                    print(text_chunk, end="", flush=True)
                
                full_text += text_chunk
                
        if stream:
            print() 
            
        return full_text

    def ask(self, prompt: str) -> str:
        """GET /ask/:prompt - Secured inference with firewall + SSD paging."""
        safe = prompt.replace(" ", "_")
        r = requests.get(
            f"{self.base_url}/ask/{safe}",
            headers=self.headers,
        )
        r.raise_for_status()
        return r.text

    # ─── System ──────────────────────────────────────────────────

    def health(self) -> str:
        """GET /health - Kernel health check."""
        r = requests.get(f"{self.base_url}/health", headers=self.headers)
        r.raise_for_status()
        return r.text

    def ps(self) -> str:
        """GET /ps - Models in VRAM."""
        r = requests.get(f"{self.base_url}/ps", headers=self.headers)
        r.raise_for_status()
        return r.text

    def ls(self) -> str:
        """GET /ls - Models on disk."""
        r = requests.get(f"{self.base_url}/ls", headers=self.headers)
        r.raise_for_status()
        return r.text

    def agents(self) -> str:
        """GET /agents - Agent security dashboard."""
        r = requests.get(f"{self.base_url}/agents", headers=self.headers)
        r.raise_for_status()
        return r.text

    def load(self, model: str) -> str:
        """GET /load/:model - Pre-load model into VRAM."""
        r = requests.get(f"{self.base_url}/load/{model}", headers=self.headers)
        r.raise_for_status()
        return r.text

    def expel(self, model: str) -> str:
        """GET /expel/:model - Evict model from VRAM."""
        r = requests.get(f"{self.base_url}/expel/{model}", headers=self.headers)
        r.raise_for_status()
        return r.text

    def clear(self, app_id: str) -> str:
        """GET /clear/:app_id - Wipe agent swap memory."""
        r = requests.get(f"{self.base_url}/clear/{app_id}", headers=self.headers)
        r.raise_for_status()
        return r.text

    # ─── IPC: Semantic Bus ───────────────────────────────────────

    def ipc_share(
        self,
        source_app: str,
        target_pipe: str,
        knowledge_text: str,
        chunk_size: int = 100,
        chunk_overlap: int = 20,
    ) -> str:
        """POST /ipc/share - Write knowledge to a Semantic Bus pipe."""
        r = requests.post(
            f"{self.base_url}/ipc/share",
            json={
                "source_app": source_app,
                "target_pipe": target_pipe,
                "knowledge_text": knowledge_text,
                "chunk_size": chunk_size,
                "chunk_overlap": chunk_overlap,
            },
            headers=self.headers,
        )
        r.raise_for_status()
        return r.text

    def ipc_search(
        self,
        source_app: str,
        target_pipe: str,
        query: str,
        top_k: int = 3,
        filter_app: str = None,
    ) -> list:
        """POST /ipc/search - Search a Semantic Bus pipe."""
        payload = {
            "source_app": source_app,
            "target_pipe": target_pipe,
            "query": query,
            "top_k": top_k,
        }
        if filter_app:
            payload["filter_app"] = filter_app

        r = requests.post(
            f"{self.base_url}/ipc/search",
            json=payload,
            headers=self.headers,
        )
        r.raise_for_status()
        return r.json()

    # ─── IPC: Message Bus ────────────────────────────────────────

    def ipc_send(self, from_app: str, to_app: str, payload: str) -> str:
        """POST /ipc/send - Send a direct agent message."""
        import time

        r = requests.post(
            f"{self.base_url}/ipc/send",
            json={
                "from_app": from_app,
                "to_app": to_app,
                "payload": payload,
                "timestamp": int(time.time()),
            },
            headers=self.headers,
        )
        r.raise_for_status()
        return r.text

    def ipc_listen(self, app_id: str) -> dict | None:
        """GET /ipc/listen/:app_id - Poll for incoming messages."""
        r = requests.get(
            f"{self.base_url}/ipc/listen/{app_id}",
            headers=self.headers,
        )
        r.raise_for_status()
        return r.json()


# ─── CLI Quick Test ──────────────────────────────────────────────

if __name__ == "__main__":
    ore = OreClient()
    print(ore.health())
