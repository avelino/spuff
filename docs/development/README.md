# Development Guide

This directory contains documentation for developing spuff.

## Contents

- [Setup](setup.md) - Development environment setup
- [Testing](testing.md) - Testing strategies and commands
- [Debugging](debugging.md) - Debugging tips and tools
- [Cross-Compilation](cross-compilation.md) - Building the agent for Linux
- [Releasing](releasing.md) - Release process

## Quick Start

```bash
# Clone
git clone https://github.com/avelino/spuff.git
cd spuff

# Build
cargo build

# Test
cargo test --all

# Run
cargo run -- status
```

## Project Structure

```
spuff/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── cli/                 # Command implementations
│   │   ├── mod.rs
│   │   └── commands/
│   │       ├── up.rs        # spuff up
│   │       ├── down.rs      # spuff down
│   │       ├── ssh.rs       # spuff ssh
│   │       └── ...
│   ├── provider/            # Cloud provider abstraction
│   │   ├── mod.rs           # Provider trait
│   │   └── digitalocean.rs  # DigitalOcean implementation
│   ├── connector/           # SSH/network operations
│   │   └── ssh.rs
│   ├── environment/         # Cloud-init and templates
│   │   └── cloud_init.rs
│   ├── agent/               # Remote agent (separate binary)
│   │   ├── main.rs
│   │   ├── routes.rs
│   │   └── metrics.rs
│   ├── tui/                 # Terminal UI
│   │   ├── mod.rs
│   │   ├── progress.rs
│   │   └── widgets.rs
│   ├── config.rs            # Configuration loading
│   ├── state.rs             # SQLite state management
│   ├── error.rs             # Error types
│   └── utils.rs             # Utilities
├── docs/                    # Documentation
├── examples/                # Example configurations
├── tests/                   # Integration tests
├── Cargo.toml
├── CLAUDE.md                # LLM instructions
├── CONTRIBUTING.md          # Contribution guidelines
└── README.md
```

## Key Concepts

### Two Binaries

Spuff produces two binaries:

1. **spuff** - CLI tool that runs on user's machine
2. **spuff-agent** - Daemon that runs on cloud VMs

### Provider Abstraction

Cloud providers implement the `Provider` trait. See [docs/providers/](../providers/) for details.

### Cloud-Init Templates

VM bootstrapping uses Tera templates. See [docs/adr/0001-cloud-init-bootstrap.md](../adr/0001-cloud-init-bootstrap.md).

### Local State

Instance tracking uses SQLite. See [docs/adr/0003-sqlite-local-state.md](../adr/0003-sqlite-local-state.md).

## Useful Commands

```bash
# Fast compile check (no build)
cargo check

# Format code
cargo fmt

# Lint check
cargo clippy

# Build release
cargo build --release

# Run specific binary
cargo run --bin spuff -- up
cargo run --bin spuff-agent

# Run with logging
RUST_LOG=debug cargo run -- up
RUST_LOG=spuff=trace cargo run -- status
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `DIGITALOCEAN_TOKEN` | DigitalOcean API token |
| `RUST_LOG` | Logging level (debug, trace, etc.) |
| `SPUFF_AGENT_TOKEN` | Agent authentication token |
| `TS_AUTHKEY` | Tailscale auth key |
