# spuff

Ephemeral dev environments in the cloud. Spin up when needed, auto-destroy when forgotten

```bash
spuff up      # Create VM, configure environment, connect
spuff down    # Destroy and stop paying
```

## The Problem

Running Claude Code, OpenCode, or heavy builds locally turns your laptop into a space heater. Cloud alternatives exist (~~Gitpod, Codespaces~~) but they're expensive, vendor-locked, or overkill for what you need.

## The Solution

A single CLI that provisions a cloud VM with your exact dev environment, connects you via SSH, and **auto-destroys after idle timeout**. No surprise bills. No forgotten instances.

**Key features:**

- **Multi-cloud** — DigitalOcean, Hetzner, AWS, Fly.io
- **Reproducible environments** — Nix/Devbox, same setup local and remote
- **Smart snapshots** — Save state before destroy, restore in seconds
- **Zero config after setup** — Dotfiles sync via git

## Who is this for

- Devs using AI coding agents (Claude Code, OpenCode, Aider)
- Anyone offloading heavy builds
- Remote workers with unstable connections
- People tired of paying for idle Codespaces

## Stack

Rust CLI, Nix/Devbox, cloud-init, Tailscale (optional)

---

**Built with open source, for open source.**
