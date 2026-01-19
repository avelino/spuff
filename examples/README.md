# Examples

This directory contains example configurations and templates for spuff.

## Configuration Examples

| File | Use Case |
|------|----------|
| [configs/minimal.yaml](configs/minimal.yaml) | Bare minimum configuration |
| [configs/developer.yaml](configs/developer.yaml) | Standard developer setup |
| [configs/power-user.yaml](configs/power-user.yaml) | Full-featured setup with Tailscale |
| [configs/ci-cd.yaml](configs/ci-cd.yaml) | CI/CD pipeline configuration |
| [configs/ml-gpu.yaml](configs/ml-gpu.yaml) | Machine learning with GPU (future) |

## Usage

Copy an example to your config directory:

```bash
cp examples/configs/developer.yaml ~/.config/spuff/config.yaml
```

Or use as a reference when running `spuff init`.

## Cloud-Init Examples

| File | Description |
|------|-------------|
| [cloud-init/custom-packages.yaml](cloud-init/custom-packages.yaml) | Adding custom packages |
| [cloud-init/custom-tools.sh](cloud-init/custom-tools.sh) | Custom tool installation script |

## Dotfiles Example

The [dotfiles/](dotfiles/) directory shows an example dotfiles repository structure that spuff can clone and apply.

## Environment Variables

Example shell configuration:

```bash
# ~/.bashrc or ~/.zshrc

# Required
export DIGITALOCEAN_TOKEN="dop_v1_xxxxxxxxxx"

# Optional
export SPUFF_AGENT_TOKEN="your-custom-token"
export TS_AUTHKEY="tskey-auth-xxxxxxxxxx"

# Logging
export RUST_LOG="spuff=debug"
```
