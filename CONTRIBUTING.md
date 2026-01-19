# Contributing to Spuff

First off, thanks for taking the time to contribute! This document provides guidelines and information about contributing to spuff.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [How to Contribute](#how-to-contribute)
- [Development Setup](#development-setup)
- [Pull Request Process](#pull-request-process)
- [Coding Standards](#coding-standards)
- [Commit Messages](#commit-messages)
- [Release Process](#release-process)

---

## Code of Conduct

This project follows the [Contributor Covenant](https://www.contributor-covenant.org/) code of conduct. By participating, you are expected to uphold this code. Please report unacceptable behavior to the maintainers.

**In short:**
- Be respectful and inclusive
- Welcome newcomers and help them learn
- Focus on what's best for the community
- Accept constructive criticism gracefully

---

## Getting Started

### Prerequisites

- Rust 1.75+ (`rustup` recommended)
- Git
- A cloud provider account for testing (DigitalOcean recommended)
- SSH key pair

### Quick Setup

```bash
# Clone the repository
git clone https://github.com/avelino/spuff.git
cd spuff

# Build
cargo build

# Run tests
cargo test --all

# Run with debug logging
RUST_LOG=spuff=debug cargo run -- status
```

See [docs/development/setup.md](docs/development/setup.md) for detailed setup instructions.

---

## How to Contribute

### Reporting Bugs

Before creating a bug report:
1. Check [existing issues](https://github.com/avelino/spuff/issues) to avoid duplicates
2. Collect relevant information:
   - Spuff version (`spuff --version`)
   - Operating system and version
   - Cloud provider being used
   - Full error message and stack trace
   - Steps to reproduce

**Create an issue** with:
- Clear, descriptive title
- Step-by-step reproduction instructions
- Expected vs actual behavior
- Relevant logs (with secrets redacted!)

### Suggesting Features

We love feature ideas! Before suggesting:
1. Check the [roadmap](CLAUDE.md#roadmap) - it might already be planned
2. Search [existing issues](https://github.com/avelino/spuff/issues) for similar suggestions
3. Consider if it fits spuff's philosophy: simple, ephemeral, cloud-agnostic

**Create an issue** with:
- Use case: What problem does this solve?
- Proposed solution: How should it work?
- Alternatives considered: What else did you think of?

### Contributing Code

1. **Find an issue** to work on, or create one for discussion
2. **Comment** on the issue to claim it
3. **Fork** the repository
4. **Create a branch** from `main`
5. **Make your changes** following our coding standards
6. **Test** your changes thoroughly
7. **Submit a PR** following the template

### Documentation

Documentation improvements are always welcome:
- Fix typos and clarify confusing sections
- Add examples and use cases
- Translate documentation
- Improve API documentation

---

## Development Setup

### Building

```bash
# Debug build (faster compilation)
cargo build

# Release build (optimized)
cargo build --release

# Build only the CLI
cargo build --bin spuff

# Build only the agent
cargo build --bin spuff-agent
```

### Testing

```bash
# Run all tests
cargo test --all

# Run specific test
cargo test test_name

# Run with output
cargo test -- --nocapture

# Run integration tests (requires cloud credentials)
cargo test --features integration
```

### Linting

```bash
# Check formatting
cargo fmt --check

# Run clippy
cargo clippy -- -D warnings

# Fix formatting
cargo fmt
```

### Cross-compilation (Agent)

The agent runs on Linux VMs but you might develop on macOS:

```bash
# Install cargo-zigbuild
cargo install cargo-zigbuild

# Build for Linux
cargo zigbuild --release --target x86_64-unknown-linux-gnu --bin spuff-agent
```

See [docs/development/cross-compilation.md](docs/development/cross-compilation.md) for details.

---

## Pull Request Process

### Before Submitting

- [ ] Code compiles without warnings (`cargo build`)
- [ ] All tests pass (`cargo test --all`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] Clippy is happy (`cargo clippy`)
- [ ] Documentation is updated if needed
- [ ] Commit messages follow our convention

### PR Template

When creating a PR, include:

```markdown
## Summary
Brief description of changes

## Motivation
Why is this change needed?

## Changes
- Change 1
- Change 2

## Testing
How did you test this?

## Checklist
- [ ] Tests added/updated
- [ ] Documentation updated
- [ ] Breaking changes documented
```

### Review Process

1. **Automated checks** run (CI, linting, tests)
2. **Maintainer review** within 3-5 business days
3. **Address feedback** by pushing new commits
4. **Approval and merge** by maintainer

### After Merge

- Delete your feature branch
- Update your fork's main branch
- Celebrate!

---

## Coding Standards

### Rust Style

We follow standard Rust conventions:

```rust
// Use descriptive names
fn create_instance(config: &InstanceConfig) -> Result<Instance>

// Document public APIs
/// Creates a new cloud instance with the given configuration.
///
/// # Arguments
/// * `config` - Instance configuration including size, region, etc.
///
/// # Errors
/// Returns an error if the provider API call fails.
pub async fn create_instance(&self, config: &InstanceConfig) -> Result<Instance>

// Handle errors explicitly, don't panic
match provider.create_instance(&config).await {
    Ok(instance) => instance,
    Err(e) => return Err(SpuffError::Provider(format!("Failed: {}", e))),
}

// Prefer early returns
if config.region.is_empty() {
    return Err(SpuffError::Config("Region is required"));
}
```

### Project Structure

```
src/
├── cli/           # Command implementations
├── provider/      # Cloud provider abstractions
├── connector/     # SSH/networking
├── environment/   # Cloud-init, templates
├── agent/         # Remote agent binary
├── tui/           # Terminal UI components
├── config.rs      # Configuration loading
├── state.rs       # Local state management
├── error.rs       # Error types
└── main.rs        # CLI entry point
```

### Error Handling

- Use `thiserror` for error types
- Provide context in error messages
- Don't swallow errors silently
- Use `anyhow::Result` in binary, custom errors in library code

### Testing

- Test behavior, not implementation
- Include happy path + edge cases + error cases
- Use descriptive test names
- Mock external services (cloud APIs, SSH)

---

## Commit Messages

We use conventional commit style:

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

### Types

| Type | Description |
|------|-------------|
| `feat` | New feature |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `style` | Formatting, no code change |
| `refactor` | Code change that neither fixes nor adds |
| `perf` | Performance improvement |
| `test` | Adding or fixing tests |
| `chore` | Maintenance tasks |

### Examples

```
feat(provider): add Hetzner Cloud support

Implements the Provider trait for Hetzner Cloud API.
Supports instance creation, destruction, and snapshots.

Closes #42
```

```
fix(ssh): handle passphrase-protected keys

Detect when SSH key requires passphrase and provide
helpful error message directing user to ssh-add.

Fixes #37
```

```
docs: add provider development guide

New documentation for implementing custom cloud providers
including API reference and testing guide.
```

---

## Release Process

Releases are managed by maintainers:

1. **Version bump** in `Cargo.toml`
2. **Update documentation** if needed
3. **Create release PR**
4. **Tag after merge** (`git tag v0.1.0`)
5. **GitHub Release** with release notes
6. **Binary artifacts** built by CI

### Versioning

We follow [Semantic Versioning](https://semver.org/):

- **MAJOR**: Breaking changes
- **MINOR**: New features, backwards compatible
- **PATCH**: Bug fixes, backwards compatible

During alpha (0.x.x), minor versions may include breaking changes.

---

## Getting Help

- **Questions**: Open a [Discussion](https://github.com/avelino/spuff/discussions)
- **Bugs**: Open an [Issue](https://github.com/avelino/spuff/issues)
- **Chat**: Join our Discord (coming soon)

---

## Recognition

Contributors are recognized in:
- Release notes
- README contributors section
- GitHub contributor graph

Thank you for contributing to spuff!
