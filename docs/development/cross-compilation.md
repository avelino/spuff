# Cross-Compilation Guide

The spuff-agent runs on Linux cloud VMs, but you might develop on macOS or Windows. This guide covers cross-compiling the agent.

## Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                      Development Machine (macOS)                             │
│                                                                              │
│   ┌─────────────────┐        ┌─────────────────────────────────────────┐   │
│   │   Rust Code     │───────►│  cargo zigbuild                         │   │
│   │   (src/agent/)  │        │  --target x86_64-unknown-linux-gnu      │   │
│   └─────────────────┘        └─────────────────────────────────────────┘   │
│                                              │                               │
│                                              ▼                               │
│                              ┌─────────────────────────────────────────┐   │
│                              │  target/x86_64-unknown-linux-gnu/       │   │
│                              │  release/spuff-agent                    │   │
│                              │  (Linux ELF binary)                     │   │
│                              └─────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
                                               │
                                               │ SCP/cloud-init
                                               ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Cloud VM (Linux)                                   │
│                                                                              │
│   /opt/spuff/spuff-agent                                                    │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Using cargo-zigbuild (Recommended)

### Install Zig

**macOS:**

```bash
brew install zig
```

**Linux:**

```bash
# Download from https://ziglang.org/download/
# Or use package manager
sudo apt install zig  # Debian/Ubuntu
```

**Verify:**

```bash
zig version
```

### Install cargo-zigbuild

```bash
cargo install cargo-zigbuild
```

### Cross-Compile

```bash
# Add Linux target
rustup target add x86_64-unknown-linux-gnu

# Build agent for Linux
cargo zigbuild --release --target x86_64-unknown-linux-gnu --bin spuff-agent
```

The binary will be at:

```
target/x86_64-unknown-linux-gnu/release/spuff-agent
```

### Why zigbuild?

- Uses Zig as a C compiler/linker
- No need for cross-compilation toolchain
- Works out of the box on macOS
- Handles glibc linking properly

## Using Docker (Alternative)

If you prefer Docker:

```bash
# Build in Linux container
docker run --rm -v $(pwd):/app -w /app rust:latest \
  cargo build --release --bin spuff-agent

# Binary at target/release/spuff-agent
```

Or with a Dockerfile:

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin spuff-agent

FROM scratch
COPY --from=builder /app/target/release/spuff-agent /spuff-agent
```

## Using cross (Alternative)

```bash
# Install cross
cargo install cross

# Build
cross build --release --target x86_64-unknown-linux-gnu --bin spuff-agent
```

Note: `cross` requires Docker.

## Testing the Binary

### Check Binary Type

```bash
# On macOS
file target/x86_64-unknown-linux-gnu/release/spuff-agent
# Should output: ELF 64-bit LSB pie executable, x86-64, ...

# Check it's not macOS Mach-O
file target/release/spuff-agent  # This would be Mach-O
```

### Test on VM

Use `spuff up --dev` to upload and test your local agent:

```bash
# Build agent for Linux
cargo zigbuild --release --target x86_64-unknown-linux-gnu --bin spuff-agent

# Create VM with local agent
cargo run -- up --dev
```

The `--dev` flag:

1. Creates VM normally
2. Uploads local agent binary via SCP
3. Restarts agent service

### Manual Upload

```bash
# Build
cargo zigbuild --release --target x86_64-unknown-linux-gnu --bin spuff-agent

# Get VM IP
IP=$(cargo run -- status --json | jq -r '.ip')

# Upload
scp -o StrictHostKeyChecking=no \
  target/x86_64-unknown-linux-gnu/release/spuff-agent \
  dev@$IP:/tmp/spuff-agent

# SSH and install
ssh dev@$IP 'sudo mv /tmp/spuff-agent /opt/spuff/ && sudo systemctl restart spuff-agent'
```

## Build Script

Create a build script for convenience:

```bash
#!/bin/bash
# scripts/build-agent.sh

set -e

TARGET="x86_64-unknown-linux-gnu"
BINARY="spuff-agent"

echo "Building $BINARY for $TARGET..."

cargo zigbuild --release --target $TARGET --bin $BINARY

OUTPUT="target/$TARGET/release/$BINARY"

echo "Built: $OUTPUT"
file $OUTPUT
ls -lh $OUTPUT
```

## Troubleshooting

### Missing Target

```bash
error: target 'x86_64-unknown-linux-gnu' not found
```

**Solution:**

```bash
rustup target add x86_64-unknown-linux-gnu
```

### Zig Not Found

```bash
error: linker `zig` not found
```

**Solution:**
Install Zig (see above) and ensure it's in PATH.

### glibc Version Mismatch

```bash
/lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.XX' not found
```

**Solution:**
The VM has an older glibc. Options:

1. Target an older glibc: `cargo zigbuild --target x86_64-unknown-linux-gnu.2.17`
2. Use Ubuntu 24.04 images (has newer glibc)

### Linking Errors

```bash
error: linking with `cc` failed
```

**Solution:**
Ensure you're using `cargo zigbuild` not `cargo build` for cross-compilation.

## CI/CD Integration

### GitHub Actions

```yaml
jobs:
  build-agent:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-unknown-linux-gnu

      - name: Build agent
        run: cargo build --release --target x86_64-unknown-linux-gnu --bin spuff-agent

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: spuff-agent-linux
          path: target/x86_64-unknown-linux-gnu/release/spuff-agent
```

## Supported Targets

| Target | Architecture | Notes |
|--------|--------------|-------|
| `x86_64-unknown-linux-gnu` | x86-64 Linux | Primary target |
| `aarch64-unknown-linux-gnu` | ARM64 Linux | For ARM VMs |
| `x86_64-unknown-linux-musl` | x86-64 Linux (static) | No glibc dependency |

### Building for ARM64

```bash
rustup target add aarch64-unknown-linux-gnu
cargo zigbuild --release --target aarch64-unknown-linux-gnu --bin spuff-agent
```

### Static Linking (musl)

```bash
rustup target add x86_64-unknown-linux-musl
cargo zigbuild --release --target x86_64-unknown-linux-musl --bin spuff-agent
```

Static binaries work on any Linux distribution but may be slightly slower.
