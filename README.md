# Oh My Works (OMW)

> Linux-first, ultra-lightweight AI workspace, terminal multiplexer orchestrator, and anti-OOM server manager. Built with Rust, Go, and Zig.

[![GitHub Repo](https://img.shields.io/badge/GitHub-IvanoWisnuBudiLaras%2FOh--My--Works-blue)](https://github.com/IvanoWisnuBudiLaras/Oh-My-Works)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-green)](#license)

---

## Monorepo Structure

```text
Oh-My-Works/
├── Cargo.toml            # Workspace root
├── crates/
│   ├── omw-core/         # Cross-platform abstractions (Linux PSI kernel sensors & Win32 memory API fallback)
│   ├── omw-terminal/     # Multi-terminal multiplexer state & session manager
│   ├── omw-guardian/     # Anti-OOM evaluator & system health watchdog
│   ├── omw-harness/      # Meta-AI harness orchestrator & agent task dispatcher
│   ├── omw-memory/       # Hybrid Tri-Layer Graph Memory (episodic, semantic, procedural)
│   └── omw-cli/          # Fast CLI entrypoint for telemetry and daemon control
├── apps/
│   ├── desktop/          # Desktop UI application & interactive workspace
│   └── server-daemon/    # Zero-dependency, ultra-lightweight telemetry daemon (<1MB, stdlib HTTP)
└── docs/
    ├── prd_oh_my_works.md                                              # Complete PRD & 8 Pillars Architecture
    ├── oh_my_works_omw_linux_ai_workspace_meta_harness_orchestrator.html  # Interactive UI Prototype
    └── Ide Aplikasi.txt                                                # Original project genesis & notes
```

## Quick Start

### Build & Run Unit Tests

```bash
cargo test
```

### Run CLI (Status, Multiplexer, Harness, Memory)

```bash
cargo run -p omw-cli -- status
cargo run -p omw-cli -- terminal
cargo run -p omw-cli -- exec git status
cargo run -p omw-cli -- harness
cargo run -p omw-cli -- memory
```

### Run Server Telemetry Daemon

```bash
cargo run -p omw-server-daemon
# curl http://127.0.0.1:9180
```

### Run Desktop UI (Web Preview)

```bash
cd apps/desktop && npm start
# Open http://localhost:3000
```

---

## 8 Architecture Pillars

1. **Desktop Workspace & Multi-Terminal Orchestrator:** Low-footprint desktop shell with programmatic PTY multiplexing.
2. **Meta-AI Harness & Auto-Skill Router:** Coordinator commanding subagent harnesses.
3. **Hybrid Tri-Layer Graph Memory:** Fast associative knowledge retrieval.
4. **Dual-Engine Browser Use:** Headless automation via Zig/Lightpanda and full browser control.
5. **App Cloner & Pentester:** Reverse engineering and vulnerability assessment hooks.
6. **Containerized Android Emulator:** Linux Waydroid/container automated runtime.
7. **Anti-OOM Guardian & Server Panel:** Kernel PSI stall telemetry and proactive process safety.
8. **Executive Synthesis & Git Backup:** Automated syncing and session preservation.

---

## License

Dual-licensed under MIT or Apache-2.0.
