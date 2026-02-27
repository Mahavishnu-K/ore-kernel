# 🛠️ Contributing to ORE

So, you want to hack on the Kernel for AI? Welcome aboard. 🦀

We are building the **POSIX standard for Local Intelligence**. This isn't just another wrapper; we are building the scheduler, the firewall, and the memory systems that will stop our GPUs from melting when 10 agents try to run at once.

If you are reading this, you are probably annoyed by how fragile local AI is right now. Good. Let's fix it.

---

## 🚨 The "It Works on My Machine" Disclaimer

Look, `ore-kernel` is young. It was born in a caffeine-fueled sprint.

- Some code is **beautiful Rust**. (Preemptive async scheduling? *Chef's kiss*).
- Some code **sucks**. (We might, nah.. we *have* `.unwrap()` in places we shouldn't. The error handling might be a bit dramatic).

If you see garbage code, don't just judge it - **fix it :)**. 

Refactors are not just welcome; they are **celebrated** 🎉. Make this kernel bulletproof.

---

## ⚡ How to Jump In

### 1. The "Good First Issue" Hunt

If you're new here or just want to dip your toes in:

- Check the **Issues** tab.
- Look for the `good first issue` label. These are usually:
  - Adding CLI colors (because plain text is boring).
  - Adding a new Regex rule to the firewall.
  - Fixing a typo in the docs (we can't spell).

### 2. The Hardcore Engineering

If you want to **architect subsystems**:

- **The Scheduler:** We need better priority queues.
- **The Firewall:** We need smarter heuristics for Prompt Injection.
- **IPC:** We need Unix Domain Sockets for speed.

> **Please open an Issue first** before rewriting the entire scheduler. We don't want you to waste a weekend building something we're already halfway through refactoring.

---

## 💻 The Stack (aka The Toolbelt)

- **Language:** Rust 🦀 (Strictly. No Python in the kernel core).
- **Async Runtime:** `tokio` (The heartbeat).
- **Web Framework:** `axum` (For the syscall interface).
- **CLI:** `clap` (Because parsing flags manually is pain).

### Setup

1. **Fork it.**
2. `git clone` your fork.
3. `cargo build --workspace` (Go grab a coffee, compiling takes a minute).
4. `cargo test` (If this fails, yell at us in Discord).

---

## 📜 The Rules of the Road

- **No `unwrap()` in Production:** The Kernel cannot crash. If a file is missing, return a `Result<Error>`. If you panic, the user's agent dies. Don't kill the agents.
- **Format Your Code:** Run `cargo fmt` before pushing. If the CI fails because of whitespace, a kitten cries somewhere.
- **Clippy is God:** Run `cargo clippy`. If Clippy says your code is inefficient, listen to the crab.
- **Commit Messages:**
  - ✅ Good: `feat: add IPv4 redaction to firewall`
  - ❌ Bad: `fixed stuff` or `wip`

---

## 🧠 Philosophy

1. **Security First:** We assume the user creates Agents that are dangerously stupid. ORE protects the user from their own creations.
2. **Performance Second:** We are a Kernel. We must be lighter than the apps we run.
3. **Features Third:** Don't bloat the core. If it can be a plugin, make it a plugin.

---

## 🤝 Need Help?

Join the **Discord** (Link in README). We hang out in `#dev-core`. If you're stuck on a borrow checker error, we've all been there. We'll help you out.

**Now go build something cool.**

*\- Mahavishnu-K & The ORE Maintainers*
