# Contributing to Ore

So, you want to hack on the kernel for AI? Welcome aboard.

We're building something like POSIX for local intelligence: a stable interface between applications and inference hardware. This isn't just another wrapper. We're building the scheduler, the sandbox, the memory manager, and the firewall that will stop our GPUs from melting when 10 agents try to run at once.

If you're reading this, you're probably annoyed by how fragile local AI is right now. Good. Let's fix it.

This guide explains how to set up a development environment, what we expect from a change, and how to get it merged. If anything here is unclear or out of date, that's a bug too. Please tell us.

## Contents

- [Code of Conduct](#code-of-conduct)
- [The "It Works on My Machine" Disclaimer](#the-it-works-on-my-machine-disclaimer)
- [Ways to Contribute](#ways-to-contribute)
- [Finding Something to Work On](#finding-something-to-work-on)
- [Development Setup](#development-setup)
- [The Rules of the Road](#the-rules-of-the-road)
- [Testing](#testing)
- [Commit Messages](#commit-messages)
- [Pull Request Process](#pull-request-process)
- [Philosophy](#philosophy)
- [Licensing](#licensing)
- [Need Help?](#need-help)

---

## Code of Conduct

Everyone taking part in this project is expected to follow our [Code of Conduct](CODE_OF_CONDUCT.md). Be kind, assume good intent, and critique code, not people.

---

## The "It Works on My Machine" Disclaimer

Look, Ore is young. It was born in a caffeine-fuelled sprint.

- Some code is **beautiful Rust**. (RAII GPU leases that release VRAM the moment they go out of scope? *Chef's kiss.*)
- Some code **sucks**. (We might, nah... we *do* have `.unwrap()` in places we shouldn't. The error handling can be a bit dramatic.)

If you see bad code, don't just judge it. **Fix it :)**

Refactors aren't just welcome, they're **celebrated** 🎉. Help us make this kernel bulletproof.

---

## Ways to Contribute

You don't have to write kernel code to help.

| Contribution | Where to start |
|---|---|
| **Report a bug** | Open an issue with the [Bug Report](.github/ISSUE_TEMPLATE/bug_report.yml) template. Include your OS, GPU, engine (`native` or `ollama`), the model, and the kernel log. |
| **Request a feature** | Open an issue with the [Feature Request](.github/ISSUE_TEMPLATE/feature_request.yml) template. Describe the problem before the solution. |
| **Report a security issue** | **Do not open a public issue.** Follow [SECURITY.md](SECURITY.md) instead. |
| **Improve the docs** | Everything in [`docs/`](docs/) is fair game. If you had to figure something out the hard way, write it down for the next person. |
| **Add benchmarks** | Reproducible numbers on hardware we don't own are extremely valuable. Include the model, quantization, context length, and exact hardware. |
| **Review pull requests** | A careful second pair of eyes on an open PR helps as much as writing one. |
| **Write code** | Read on. |

---

## Finding Something to Work On

### 1. The "Good First Issue" Hunt

If you're new here, or just want to dip your toes in:

- Check the **Issues** tab.
- Look for the `good first issue` label. These are usually things like:
  - Adding CLI colors (because plain text is boring).
  - Adding or tuning a regex rule in the firewall.
  - Fixing a typo in the docs (we can't spell).
  - Replacing an `.unwrap()` with proper error handling.

### 2. The Hardcore Engineering

If you want to **architect subsystems**, the [Roadmap](ROADMAP.md) lists what's open. Phase 4 (Hardening) is where help matters most right now. Some highlights:

- **The Scheduler:** priority-aware admission (`gpu_priority` is parsed but not yet used), and real VRAM readings on AMD and Intel GPUs.
- **The Sandbox:** adversarial tests for path escapes, fuel exhaustion, and egress-rule bypasses.
- **The Firewall:** smarter prompt-injection heuristics, with measured false-positive rates.
- **IPC:** Unix domain sockets or streaming transport instead of HTTP polling.
- **The Native Engine:** loaders for Mistral, Gemma, Phi, and DeepSeek.

> **Please open an issue first** before rewriting the entire scheduler. We don't want you to waste a weekend building something we're already halfway through refactoring.

As a rule of thumb: bug fixes, docs, and small, self-contained improvements can go straight to a PR. New features, new dependencies, API or manifest changes, and anything touching security boundaries should start with an issue.

---

## Development Setup

### Prerequisites

- **Rust.** Install [rustup](https://rustup.rs/). The exact toolchain (currently 1.93.0, with `rustfmt` and `clippy`) is pinned in [`rust-toolchain.toml`](rust-toolchain.toml), and `rustup` installs it automatically the first time you build.
- **Optional: [Ollama](https://ollama.com/)**, if you're working on the Ollama driver.
- **Optional: a CUDA toolkit or Apple Metal**, if you're working on GPU paths in the native engine. Everything builds and runs on CPU without them.
- **Optional: language toolchains for `ore mktool`** (for example Go, Zig, or a WASI-capable C compiler), if you're working on the sandbox or the integration tests in [`tests/`](tests/).

### Build and Run

1. **Fork it.**
2. Clone your fork and add the upstream remote:

   ```bash
   git clone https://github.com/<your-username>/ore-kernel.git
   cd ore-kernel
   git remote add upstream https://github.com/Mahavishnu-K/ore-kernel.git
   ```

3. Build the workspace (go grab a coffee, compiling Candle and Wasmtime takes a minute):

   ```bash
   cargo build --workspace
   ```

4. Run the tests (if this fails on a clean checkout, yell at us in Discord):

   ```bash
   cargo test --workspace
   ```

5. Run the kernel and point the CLI at it:

   ```bash
   cargo run --release -p ore-server   # kernel on http://127.0.0.1:6767
   cargo run -p ore-cli -- status      # in a second terminal
   ```

   Use `--release` when you're testing inference. Debug builds of the native engine are 5 to 10 times slower.

### Make Targets

The [`Makefile`](Makefile) wraps the common commands:

| Command | Runs |
|---|---|
| `make` | `format`, `check`, `lint`, `build`, `test`, in that order |
| `make format` | `cargo fmt --all` |
| `make lint` | `cargo clippy --workspace -- -D warnings` |
| `make test` | `cargo test --workspace` |
| `make release` | `cargo build --release --workspace` |

### Where Things Live

| Crate | What it contains |
|---|---|
| [`ore-core`](ore-core/) | Kernel logic: scheduler, memory manager, sandbox, `ore-ld` linker, IPC, firewall, inference drivers |
| [`ore-server`](ore-server/) | The Axum daemon: routes, handlers, execution modes, runtime shims |
| [`ore-cli`](ore-cli/) | The `ore` command, including the `mktool` compiler and the C/Zig SDK headers |
| [`ore-common`](ore-common/) | Types shared by every crate: protocol, manifest schema, config, paths, constants |
| [`ore-sys`](ore-sys/) | Rust SDK for writing `ore-ld` hosts and plugins |

For how the pieces fit together, read [Architecture](docs/architecture.md) and the [Kernel Internals](docs/kernel-internals/) docs before changing a subsystem.

---

## The Rules of the Road

CI enforces the first three on every push and pull request. Run them locally before you push.

- **Format your code.** Run `cargo fmt --all`. If CI fails because of whitespace, a kitten cries somewhere.
- **Clippy is god.** Run `cargo clippy --workspace -- -D warnings`. Warnings are errors here. If Clippy says your code is inefficient, listen to the crab.
- **Tests pass.** Run `cargo test --workspace`.
- **No `unwrap()` in production paths.** The kernel cannot crash. If a file is missing, return a `Result` with a useful error. If you panic, the user's agents die. Don't kill the agents. (`unwrap()` and `expect()` are fine in tests, and `expect()` is acceptable for true invariants, with a message that explains why it can't fail.)
- **Justify every `unsafe`.** Each `unsafe` block needs a `// SAFETY:` comment explaining why it's sound.
- **Log through the kernel.** Use `kprintln!` rather than `println!` in `ore-core` and `ore-server`, so output stays consistent.
- **Talk before adding dependencies.** Every new crate adds to build time and attack surface. Mention it in the issue or PR and explain why.
- **Keep shared types in `ore-common`.** If more than one crate needs a type, path, or constant, it belongs there, not copied around.
- **Update the docs with the code.** If you change behavior, a route, a manifest field, or a CLI flag, update the matching page in `docs/` in the same PR.

---

## Testing

| Kind | Where | How to run |
|---|---|---|
| **Unit tests** | `#[cfg(test)]` modules next to the code | `cargo test --workspace` |
| **Sandbox and linker tests** | [`tests/`](tests/): sandbox boundaries, `ore-ld`, Memory Fusion, project compilation | Compile with `ore mktool` and run against a live kernel. See [`tests/README.md`](tests/README.md). |
| **End-to-end examples** | [`examples/`](examples/): Python agents for paging, scheduling, IPC, and the sandbox | Run against a live kernel. See [`examples/README.md`](examples/README.md). |

What we expect:

- **Bug fixes** come with a test that fails before the fix and passes after it.
- **New features** come with tests for the main path and the obvious failure cases.
- **Changes to a security boundary** (sandbox, VFS, egress proxy, manifests, auth, firewall) come with **adversarial tests**: show that the thing you're preventing is actually prevented.

If something is genuinely hard to test automatically, say so in the PR and describe how you tested it by hand.

---

## Commit Messages

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<optional scope>): <short summary in the imperative mood>

<optional body: what changed and why, wrapped at 72 characters>
```

| Type | Use it for |
|---|---|
| `feat` | A new feature |
| `fix` | A bug fix |
| `docs` | Documentation only |
| `refactor` | A code change that neither fixes a bug nor adds a feature |
| `perf` | A performance improvement |
| `test` | Adding or fixing tests |
| `chore` | Build, CI, dependencies, tooling |

Scopes are optional and usually name a crate or subsystem: `scheduler`, `sandbox`, `linker`, `ipc`, `firewall`, `cli`, `common`.

- Good: `feat(firewall): add IPv4 redaction rule`
- Good: `fix(scheduler): release KV reservation when a lease is dropped early`
- Bad: `fixed stuff`
- Bad: `wip`

Mark breaking changes with `!` after the type (`feat(manifest)!: rename max_cpu_instructions`) and explain the migration in the body.

---

## Pull Request Process

1. **Branch from `dev`.** `dev` is the integration branch; `main` holds released code. Sync first:

   ```bash
   git fetch upstream
   git checkout -b feat/my-change upstream/dev
   ```

2. **Keep it focused.** One logical change per PR. A 200-line PR gets reviewed this week. A 3,000-line PR gets reviewed eventually.
3. **Run the checks** locally: `make` (or `cargo fmt --all`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`).
4. **Update [`CHANGELOG.md`](CHANGELOG.md)** under `[Unreleased]` for any user-facing change.
5. **Open the PR against `dev`** and fill in the [pull request template](.github/PULL_REQUEST_TEMPLATE.md). Link the issue it closes (`Fixes #123`).
6. **Respond to review.** Push follow-up commits rather than force-pushing during review, so reviewers can see what changed. We may squash on merge.

A PR is ready to merge when CI is green, at least one maintainer has approved it, and all review conversations are resolved. Draft PRs are welcome if you want early feedback on a direction.

---

## Philosophy

When trade-offs conflict, this is the order we resolve them in:

1. **Security first.** We assume users will create agents that are dangerously stupid. Ore protects the user from their own creations.
2. **Performance second.** We are a kernel. We must be lighter than the apps we run.
3. **Features third.** Don't bloat the core. If it can be a plugin, make it a plugin.

---

### Licensing & Contributor Terms

The Ore Kernel (`ore-core`, `ore-server`, `ore-sys` and `ore-cli`) is licensed under the **GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later)**. 

To allow Ore to thrive as both an open-source project and a sustainable enterprise platform, we use a streamlined contributor grant instead of a restrictive corporate CLA:

**By submitting a pull request, patch, or contribution to Ore, you agree to the following terms:**
1. Your contribution is licensed under the repository's open-source license (AGPL-3.0-or-later for the kernel).
2. You grant the project maintainer (**Mahavishnu-K**), including any future legal successors, entities, or assigns, a perpetual, worldwide, non-exclusive, royalty-free, transferable, and sublicensable right to use, modify, distribute, license, and commercialize your contribution under alternative commercial or proprietary terms without restriction.
3. You certify that you are the original author of the contribution or have the legal right to submit it under these terms, free of any conflicting third-party intellectual property or patent claims.

---

## Need Help?

Join the [Discord](https://discord.com/channels/1477053099494342755/1477053558879686737). We hang out in `#dev-core`. If you're stuck on a borrow checker error, we've all been there. We'll help you out.

You can also open a draft PR or an issue with your question. There are no stupid questions, only undocumented subsystems.

**Now go build something cool.**

*\- Mahavishnu-K & The Ore Maintainers*
