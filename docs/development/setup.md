# Development Setup

This guide covers setting up your development environment for spuff.

## Prerequisites

### Required

- **Rust 1.75+** - Install via [rustup](https://rustup.rs/)
- **Git** - For version control
- **SSH client** - For testing VM connections

### Recommended

- **cargo-watch** - Auto-rebuild on changes
- **cargo-zigbuild** - Cross-compilation to Linux
- **sqlite3** - For inspecting local state

## Installation

### 1. Install Rust

```bash
# Install rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify installation
rustc --version
cargo --version
```

### 2. Clone the Repository

```bash
git clone https://github.com/avelino/spuff.git
cd spuff
```

### 3. Build

```bash
# Debug build (faster compilation)
cargo build

# Release build (optimized)
cargo build --release
```

### 4. Verify

```bash
# Run tests
cargo test --all

# Check help
cargo run -- --help
```

## Optional Tools

### cargo-watch

Auto-rebuild on file changes:

```bash
cargo install cargo-watch

# Watch and rebuild
cargo watch -x build

# Watch and run tests
cargo watch -x test
```

### cargo-zigbuild

Cross-compile the agent for Linux (required if developing on macOS):

```bash
# Install zig
brew install zig  # macOS
# or download from https://ziglang.org/download/

# Install cargo-zigbuild
cargo install cargo-zigbuild

# Cross-compile
cargo zigbuild --release --target x86_64-unknown-linux-gnu --bin spuff-agent
```

### SQLite CLI

Inspect local state database:

```bash
# macOS
brew install sqlite

# View state
sqlite3 ~/.config/spuff/state.db "SELECT * FROM instances;"
```

## IDE Setup

### VS Code

Recommended extensions:

- **rust-analyzer** - Rust language support
- **Even Better TOML** - Cargo.toml syntax
- **crates** - Dependency version info
- **CodeLLDB** - Debugging support

Settings (`.vscode/settings.json`):

```json
{
  "rust-analyzer.checkOnSave.command": "clippy",
  "rust-analyzer.cargo.features": "all",
  "[rust]": {
    "editor.formatOnSave": true
  }
}
```

### JetBrains (RustRover/IntelliJ)

1. Install Rust plugin
2. Open project directory
3. Configure toolchain in Settings > Languages > Rust

## Cloud Provider Setup

### DigitalOcean

1. Create account at [digitalocean.com](https://www.digitalocean.com/)
2. Generate API token: API > Generate New Token
3. Set environment variable:

```bash
export DIGITALOCEAN_TOKEN="dop_v1_xxxxxxxxx"
```

1. Upload SSH key: Settings > Security > SSH Keys

### Hetzner (for development)

1. Create account at [hetzner.com](https://www.hetzner.com/cloud)
2. Generate API token: Security > API Tokens
3. Set environment variable:

```bash
export HETZNER_TOKEN="xxxxxxxxx"
```

## SSH Key Setup

Ensure you have an SSH key:

```bash
# Check for existing keys
ls -la ~/.ssh/

# Generate if needed
ssh-keygen -t ed25519 -C "your@email.com"

# Add to SSH agent
eval "$(ssh-agent -s)"
ssh-add ~/.ssh/id_ed25519
```

## Configuration

Create a test configuration:

```bash
# Initialize config
cargo run -- init

# Or create manually
mkdir -p ~/.config/spuff
cat > ~/.config/spuff/config.yaml << EOF
provider: digitalocean
region: nyc1
size: s-1vcpu-1gb  # Small for testing
idle_timeout: 30m
environment: devbox
ssh_key_path: ~/.ssh/id_ed25519
ssh_user: dev
EOF
```

## Running Locally

### CLI Commands

```bash
# With cargo run
cargo run -- status
cargo run -- up
cargo run -- down

# With compiled binary
./target/debug/spuff status
./target/release/spuff up
```

### Agent (Local Testing)

The agent typically runs on VMs, but you can test locally:

```bash
# Build agent
cargo build --bin spuff-agent

# Run agent
SPUFF_AGENT_TOKEN=test-token ./target/debug/spuff-agent

# Test endpoints
curl -H "X-Spuff-Token: test-token" http://127.0.0.1:7575/health
curl -H "X-Spuff-Token: test-token" http://127.0.0.1:7575/metrics
```

## Development Workflow

### Feature Development

```bash
# 1. Create branch
git checkout -b feature/my-feature

# 2. Make changes
# ...

# 3. Format and lint
cargo fmt
cargo clippy

# 4. Test
cargo test --all

# 5. Commit
git add .
git commit -m "feat: add my feature"

# 6. Push and create PR
git push -u origin feature/my-feature
```

### Testing Changes on Real VMs

```bash
# 1. Build agent for Linux (if on macOS)
cargo zigbuild --release --target x86_64-unknown-linux-gnu --bin spuff-agent

# 2. Create VM with --dev flag (uploads local agent)
cargo run -- up --dev

# 3. Test changes on VM
# ...

# 4. Destroy VM
cargo run -- down --force
```

## Troubleshooting

### Build Errors

```bash
# Clear build cache
cargo clean

# Update dependencies
cargo update

# Check for issues
cargo check
```

### Permission Denied on SSH Key

```bash
# Fix permissions
chmod 600 ~/.ssh/id_ed25519
chmod 644 ~/.ssh/id_ed25519.pub
```

### Agent Not Starting

Check systemd logs on the VM:

```bash
sudo systemctl status spuff-agent
sudo journalctl -u spuff-agent -f
```

### State Database Issues

Reset local state:

```bash
rm ~/.config/spuff/state.db
```

## Next Steps

- [Testing](testing.md) - Learn about testing strategies
- [Debugging](debugging.md) - Debugging tips
- [Cross-Compilation](cross-compilation.md) - Building for Linux
