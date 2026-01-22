# Project Configuration (`spuff.yaml`)

spuff supports per-project configuration via a `spuff.yaml` file in your project root. This enables **environment as code** - defining your development environment declaratively alongside your source code.

> **See also:** [Spuff Specification](./spec.md) for the formal specification with validation rules and conformance requirements.

## Overview

When you run `spuff up` in a directory containing a `spuff.yaml` file, spuff will:

1. Read the project configuration
2. Apply any resource overrides (size, region)
3. Provision the VM with the project config embedded
4. Install bundles, packages, and services automatically
5. Clone repositories and run setup scripts

## File Location

Place `spuff.yaml` in your project root (same directory as your git repository). spuff will search up the directory tree to find it.

```
my-project/
├── spuff.yaml          # Project configuration
├── spuff.secrets.yaml  # Secrets (add to .gitignore!)
├── docker-compose.yaml # Services (optional)
└── src/
```

## Complete Example

```yaml
# spuff.yaml - Project environment configuration
version: "1"

# Project name (default: directory name)
name: my-awesome-project

# Override VM resources
resources:
  size: s-4vcpu-8gb
  region: nyc1

# Language bundles - pre-configured toolchains
bundles:
  - rust       # rustup, cargo, rust-analyzer, clippy
  - node       # nodejs, npm, typescript, eslint
  - python     # python3, pip, uv, ruff, pyright

# Additional system packages
packages:
  - postgresql-client
  - redis-tools
  - protobuf-compiler

# Docker services (uses docker-compose.yaml)
services:
  enabled: true
  compose_file: docker-compose.yaml
  profiles: [dev]

# Repositories to clone
repositories:
  - owner/repo                           # Short format (GitHub)
  - url: git@github.com:org/backend.git  # Full format
    path: ~/projects/backend
    branch: develop

# Environment variables
env:
  DATABASE_URL: postgres://dev:dev@localhost:5432/mydb
  REDIS_URL: redis://localhost:6379
  RUST_LOG: debug

# Setup scripts (run in order)
setup:
  - cargo build
  - npm install
  - ./scripts/init-db.sh

# Ports for SSH tunneling
ports:
  - 3000  # Frontend
  - 8080  # Backend API
  - 5432  # Postgres

# Volume mounts (SSHFS-based bidirectional sync)
volumes:
  - source: ./data          # Local directory to sync
    target: ~/data          # Remote directory on VM
  - source: ./src
    target: ~/project/src
    mount_point: ./src      # Mount remote back to local (bidirectional)

# Lifecycle hooks
hooks:
  post_up: |
    echo "Environment ready!"
    make dev-setup
  pre_down: |
    make db-backup
```

---

## Configuration Reference

### `version`

**Type:** `string`
**Default:** `"1"`

Spec version for future compatibility.

```yaml
version: "1"
```

---

### `name`

**Type:** `string` (optional)
**Default:** Directory name

Custom name for the environment.

```yaml
name: my-project
```

---

### `resources`

Override global VM configuration. CLI flags take precedence over project config.

```yaml
resources:
  size: s-4vcpu-8gb    # VM size
  region: fra1         # Region
```

**Precedence:** CLI flags > spuff.yaml > ~/.spuff/config.yaml

---

### `bundles`

Pre-configured language toolchains. Each bundle installs the compiler/runtime plus essential development tools (LSPs, linters, formatters).

```yaml
bundles:
  - rust
  - go
  - python
```

**Available bundles:**

| Bundle   | Includes                                                    |
|----------|-------------------------------------------------------------|
| `rust`   | rustup, cargo, rust-analyzer, clippy, rustfmt, mold         |
| `go`     | go, gopls, delve, golangci-lint, air                        |
| `python` | python3.12, pip, venv, uv, ruff, pyright, ipython           |
| `node`   | node 22 LTS, npm, pnpm, typescript, eslint, prettier        |
| `elixir` | erlang/OTP, elixir, mix, elixir-ls, phoenix                 |
| `java`   | openjdk 21, maven, gradle, jdtls                            |
| `zig`    | zig, zls                                                    |
| `cpp`    | gcc, clang, cmake, ninja, clangd, gdb, lldb                 |
| `ruby`   | ruby, bundler, solargraph, rubocop                          |

---

### `packages`

Additional system packages to install via apt.

```yaml
packages:
  - postgresql-client
  - redis-tools
  - libssl-dev
  - protobuf-compiler
```

---

### `services`

Docker services configuration. Uses your project's `docker-compose.yaml`.

```yaml
services:
  enabled: true                     # Default: true
  compose_file: docker-compose.yaml # Default: docker-compose.yaml
  profiles: [dev, debug]            # Optional compose profiles
```

**Note:** spuff doesn't duplicate docker-compose configuration - it uses your existing compose file.

---

### `repositories`

Clone additional repositories into the environment.

```yaml
repositories:
  # Short format (GitHub)
  - owner/repo

  # Full format
  - url: git@github.com:org/backend.git
    path: ~/projects/backend    # Default: ~/projects/<repo-name>
    branch: develop             # Optional

  # HTTPS format
  - url: https://github.com/org/shared-libs.git
```

**SSH Agent Forwarding:** spuff uses SSH agent forwarding, so your local SSH keys work for cloning private repos.

---

### `env`

Environment variables set on the VM.

```yaml
env:
  DATABASE_URL: postgres://localhost:5432/mydb
  REDIS_URL: redis://localhost:6379
  DEBUG: "true"
```

**Variable resolution:** References to `$VAR`, `${VAR}`, or `${VAR:-default}` are resolved from your local environment before being sent to the VM.

```yaml
env:
  # Resolved from local $DATABASE_PASSWORD
  DATABASE_PASSWORD: $DATABASE_PASSWORD

  # With default value
  LOG_LEVEL: ${LOG_LEVEL:-info}
```

---

### `setup`

Shell commands executed after bundles and packages are installed.

```yaml
setup:
  - cargo build --release
  - npm install
  - ./scripts/init-db.sh
```

Scripts are executed in order. If any script fails, subsequent scripts are skipped.

---

### `ports`

Ports for automatic SSH tunneling. When you run `spuff ssh`, these ports are forwarded from your local machine to the VM.

```yaml
ports:
  - 3000  # localhost:3000 -> vm:3000
  - 8080  # localhost:8080 -> vm:8080
  - 5432  # localhost:5432 -> vm:5432
```

This allows you to work "locally" (browser, IDE) while connected to the remote VM.

---

### `volumes`

Mount remote VM directories locally via SSHFS for bidirectional file editing.

```yaml
volumes:
  # Basic: sync local to remote
  - source: ./data
    target: ~/data

  # Bidirectional: mount remote over local for real-time editing
  - source: ./src
    target: ~/project/src
    mount_point: ./src    # Optional: mount remote back here

  # With explicit mount point
  - source: ./config
    target: /etc/myapp
    mount_point: ~/.local/share/spuff/mounts/myapp-config
```

**Fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `source` | Yes | Local directory path (relative to spuff.yaml or absolute) |
| `target` | Yes | Remote directory path on the VM |
| `mount_point` | No | Where to mount remote directory locally |

**Mount Point Resolution:**

1. If `mount_point` is specified, use it
2. If only `source` is specified, mount over `source` for bidirectional editing
3. Otherwise, auto-generate under `~/.local/share/spuff/mounts/<instance>/<path>`

**Behavior during `spuff up`:**

1. Remote directory is created on the VM
2. Local `source` is synced to remote `target` via rsync
3. Remote `target` is mounted locally via SSHFS

**Behavior during `spuff down`:**

1. All mounted volumes are force-unmounted before VM destruction
2. This prevents SSHFS from hanging when the remote server disappears

**Requirements:**

- macOS: [macFUSE](https://osxfuse.github.io/) and `sshfs` (`brew install macfuse sshfs`)
- Linux: `fuse` and `sshfs` packages

**CLI Commands:**

```bash
spuff volume mount              # Mount all configured volumes
spuff volume unmount            # Unmount all volumes
spuff volume ls                 # List volume status
```

---

### `hooks`

Lifecycle scripts for custom automation.

```yaml
hooks:
  # Runs after environment is fully ready
  post_up: |
    echo "Environment ready!"
    make dev-setup

  # Runs before VM destruction
  pre_down: |
    make db-backup > /tmp/backup.sql
```

---

## Secrets Management

### spuff.secrets.yaml

Store sensitive values in a separate file that's **not committed to git**:

```yaml
# spuff.secrets.yaml
env:
  DATABASE_PASSWORD: super-secret
  API_KEY: sk-xxx
  AWS_SECRET_ACCESS_KEY: xxxxx
```

Add to `.gitignore`:

```gitignore
spuff.secrets.yaml
```

**Merge behavior:** `spuff.secrets.yaml` is merged with `spuff.yaml`, with secrets taking precedence.

### Environment Variable Resolution

Reference local environment variables in your config:

```yaml
env:
  # Simple reference
  API_KEY: $API_KEY

  # With braces
  SECRET: ${DATABASE_SECRET}

  # With default value
  LOG_LEVEL: ${LOG_LEVEL:-debug}
```

---

## CLI Integration

### `spuff up`

When `spuff.yaml` is present:

```
$ spuff up

  Creating instance: my-project-abc123
  Provider: digitalocean    Region: nyc1
  Size: s-4vcpu-8gb (from spuff.yaml)

  [1/5] Creating instance................ ✓
  [2/5] Waiting for IP................... ✓ 167.99.123.45
  [3/5] Waiting for SSH.................. ✓
  [4/5] Running bootstrap................ ✓
  [5/5] Agent ready...................... ✓

  Project Setup (spuff.yaml)
  The following will be installed by spuff-agent:

  Bundles: rust, node, python
  Packages: postgresql-client, redis-tools
  Services: docker-compose.yaml (2 services)
  Repositories: 3 repos to clone
  Ports: 3000, 8080, 5432 (tunnel via `spuff ssh`)
  Setup scripts: 3 commands

  Run `spuff status --detailed` to track progress

  ✓ Instance ready!
```

### `spuff status --detailed`

Shows project setup progress:

```
$ spuff status --detailed

  ● my-project-abc123 (167.99.123.45)

  Provider      digitalocean
  Region        nyc1
  Size          s-4vcpu-8gb
  Uptime        2h 15m
  Bootstrap     ready

  ╭────────────────────────────────────────────────────────╮
  │  Project Setup (spuff.yaml)                            │
  ├────────────────────────────────────────────────────────┤
  │  Bundles                                               │
  │    [✓] rust (1.78.0)                                   │
  │    [✓] node (22.0.0)                                   │
  │    [>] python (installing...)                          │
  │  Packages                                              │
  │    [✓] 2 installed                                     │
  │  Services (docker-compose.yaml)                        │
  │    [✓] postgres (5432) - running                       │
  │    [✓] redis (6379) - running                          │
  │  Repositories                                          │
  │    [✓] backend → ~/projects/backend                    │
  │    [>] frontend (cloning...)                           │
  │  Setup Scripts                                         │
  │    [ ] #1 cargo build --release                        │
  │    [ ] #2 npm install                                  │
  ╰────────────────────────────────────────────────────────╯
```

### `spuff logs`

View project setup logs:

```bash
spuff logs                    # General setup log
spuff logs --bundle rust      # Rust bundle installation
spuff logs --packages         # Package installation
spuff logs --repos            # Repository cloning
spuff logs --services         # Docker services
spuff logs --script 1         # Setup script #1
spuff logs -f                 # Follow mode (tail -f)
```

### `spuff ssh`

Connects with automatic port tunneling:

```
$ spuff ssh

  ╭──────────────────────────────────────────────────────────╮
  │  SSH Tunnels (from spuff.yaml)                           │
  │  localhost:3000 → vm:3000                                │
  │  localhost:8080 → vm:8080                                │
  │  localhost:5432 → vm:5432                                │
  ╰──────────────────────────────────────────────────────────╯

  Connecting to my-project-abc123 (167.99.123.45)...

dev@my-project-abc123:~$
```

---

## Logging

All project setup activities are logged to `/var/log/spuff/` on the VM:

```
/var/log/spuff/
├── setup.log           # General setup log
├── bundles/
│   ├── rust.log        # Rust bundle installation
│   ├── node.log        # Node bundle installation
│   └── python.log      # Python bundle installation
├── packages.log        # apt package installation
├── repositories.log    # Git clone operations
├── services.log        # Docker compose logs
└── scripts/
    ├── 001.log         # First setup script
    ├── 002.log         # Second setup script
    └── 003.log         # Third setup script
```

---

## Agent API Endpoints

The spuff-agent exposes these endpoints for project management:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/project/config` | GET | Current project configuration |
| `/project/status` | GET | Detailed setup progress |
| `/project/setup` | POST | Start project setup |

These are used internally by the CLI but can be accessed directly via SSH tunnel.

---

## Best Practices

1. **Keep spuff.yaml in version control** - The environment becomes reproducible
2. **Use spuff.secrets.yaml for secrets** - Never commit credentials
3. **Prefer bundles over individual packages** - They include LSPs and dev tools
4. **Use `docker-compose.yaml` for services** - Don't duplicate config
5. **Test your setup scripts locally first** - Saves provisioning time
6. **Use specific versions in setup scripts** - Avoid "works on my machine" issues

---

## Troubleshooting

### Project config not detected

```
No spuff.yaml found in current directory or parents
```

**Solution:** Ensure `spuff.yaml` is in your project root and you're running `spuff up` from within the project directory.

### Bundle installation failed

```bash
spuff logs --bundle rust
```

Check the bundle-specific log for errors. Common issues:

- Network connectivity
- Disk space
- Package conflicts

### Services not starting

```bash
spuff logs --services
```

Ensure your `docker-compose.yaml` is valid and doesn't require manual configuration.

### Setup script failed

```bash
spuff logs --script 1
```

View the specific script's output. The script runs in the user's home directory by default.
