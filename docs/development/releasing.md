# Release Process

This document describes how to create releases for spuff.

## Versioning

We follow [Semantic Versioning](https://semver.org/):

- **MAJOR** (x.0.0): Breaking changes
- **MINOR** (0.x.0): New features, backwards compatible
- **PATCH** (0.0.x): Bug fixes, backwards compatible

During alpha (0.x.x), minor versions may include breaking changes.

## Release Checklist

### 1. Prepare Release

```bash
# Ensure you're on main
git checkout main
git pull origin main

# Create release branch
git checkout -b release/v0.2.0
```

### 2. Update Version

Edit `Cargo.toml`:

```toml
[package]
name = "spuff"
version = "0.2.0"  # Update this
```

### 3. Update Documentation

- [ ] README.md - version badges, new features
- [ ] CLAUDE.md - roadmap updates
- [ ] docs/configuration.md - new config options

### 4. Run Tests

```bash
# All tests
cargo test --all

# Clippy
cargo clippy -- -D warnings

# Format check
cargo fmt --check

# Build release
cargo build --release
```

### 5. Create PR

```bash
git add -A
git commit -m "chore: prepare release v0.2.0"
git push -u origin release/v0.2.0
```

Create PR: "Release v0.2.0"

### 6. Merge and Tag

After PR approval and merge:

```bash
git checkout main
git pull origin main

# Create tag
git tag -a v0.2.0 -m "Release v0.2.0"

# Push tag
git push origin v0.2.0
```

### 7. Create GitHub Release

1. Go to [Releases](https://github.com/avelino/spuff/releases)
2. Click "Draft a new release"
3. Select tag: `v0.2.0`
4. Title: `v0.2.0`
5. Generate release notes or write manually
6. Publish release

## Release Notes Template

```markdown
## What's Changed

### New Features
- Feature 1 (#PR)
- Feature 2 (#PR)

### Bug Fixes
- Fix issue (#PR)

### Documentation
- Doc improvement (#PR)

### Breaking Changes
- Breaking change description

## Upgrade Guide

Steps to upgrade from previous version.

## Contributors

Thanks to @contributor1, @contributor2
```

## Automated Releases (Future)

### GitHub Actions

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: spuff-linux-x64
          - os: macos-latest
            target: x86_64-apple-darwin
            artifact: spuff-macos-x64
          - os: macos-latest
            target: aarch64-apple-darwin
            artifact: spuff-macos-arm64

    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Build
        run: cargo build --release --target ${{ matrix.target }}

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: target/${{ matrix.target }}/release/spuff

  release:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - name: Download artifacts
        uses: actions/download-artifact@v4

      - name: Create release
        uses: softprops/action-gh-release@v1
        with:
          files: |
            spuff-linux-x64/spuff
            spuff-macos-x64/spuff
            spuff-macos-arm64/spuff
```

## Binary Distribution

### Current (Alpha)

Build from source:

```bash
git clone https://github.com/avelino/spuff.git
cd spuff
cargo build --release
cp target/release/spuff ~/.local/bin/
```

### Future Plans

- [ ] Homebrew tap
- [ ] apt/yum repositories
- [ ] Pre-built binaries on GitHub Releases
- [ ] cargo install support

### Homebrew Formula (Draft)

```ruby
class Spuff < Formula
  desc "Ephemeral dev environments in the cloud"
  homepage "https://github.com/avelino/spuff"
  url "https://github.com/avelino/spuff/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "..."
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "build", "--release"
    bin.install "target/release/spuff"
  end

  test do
    assert_match "spuff #{version}", shell_output("#{bin}/spuff --version")
  end
end
```

## Version Bumping Script

```bash
#!/bin/bash
# scripts/bump-version.sh

VERSION=$1

if [ -z "$VERSION" ]; then
  echo "Usage: $0 <version>"
  exit 1
fi

# Update Cargo.toml
sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml

# Verify
grep "^version" Cargo.toml

echo "Version bumped to $VERSION"
echo "Don't forget to commit and tag!"
```

## Hotfix Process

For urgent fixes:

```bash
# Create hotfix branch from tag
git checkout -b hotfix/v0.2.1 v0.2.0

# Make fix
# ...

# Bump patch version
./scripts/bump-version.sh 0.2.1

# Commit
git commit -am "fix: critical bug"

# Create PR to main
# After merge, tag
git tag -a v0.2.1 -m "Hotfix v0.2.1"
git push origin v0.2.1
```

## Post-Release

After release:

1. Announce on social media/Discord
2. Update documentation if needed
3. Monitor for issues
4. Bump to next dev version if desired
