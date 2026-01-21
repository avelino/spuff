# Spuff Specification

## Status of this document

This document specifies the Spuff project configuration file format used to define ephemeral cloud development environments. Distribution of this document is unlimited.

The canonical version of this specification can be found at [docs/spec.md](./spec.md).

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in [RFC 2119](https://www.ietf.org/rfc/rfc2119.txt).

## Version

This document specifies **Spuff Configuration Format version 1**.

## Requirements and Optional Attributes

The Spuff configuration format aims to provide a developer-friendly syntax for defining reproducible development environments. Implementation of some attributes and features MAY vary across cloud providers and SHOULD be documented by the implementation.

The following terms are used to define attribute requirements:

- **Required**: Attributes that MUST be present for the configuration to be valid
- **Optional**: Attributes that MAY be omitted; implementations MUST use defined default values
- **Platform-dependent**: Behavior or availability depends on the underlying cloud provider

## The Spuff File

The configuration file MUST be named `spuff.yaml` or `spuff.yml`. The file MUST be valid [YAML 1.2](https://yaml.org/spec/1.2/spec.html) encoded in UTF-8.

A **Spuff configuration** consists of:

- `version` - Specification version (OPTIONAL)
- `name` - Project name (OPTIONAL)
- `resources` - VM resource configuration (OPTIONAL)
- `bundles` - Language toolchain definitions (OPTIONAL)
- `packages` - System packages (OPTIONAL)
- `services` - Docker Compose services (OPTIONAL)
- `repositories` - Additional repositories to clone (OPTIONAL)
- `env` - Environment variables (OPTIONAL)
- `setup` - Setup scripts (OPTIONAL)
- `ports` - SSH tunnel port mappings (OPTIONAL)
- `hooks` - Lifecycle hooks (OPTIONAL)

The following diagram shows how the configuration elements interact:

```mermaid
flowchart TB
    subgraph spuffyaml["spuff.yaml"]
        version["version: '1'<br/>name: my-project"]

        subgraph config["Configuration"]
            resources["resources<br/>(VM sizing)"]
            bundles["bundles<br/>(toolchains)"]
            packages["packages<br/>(apt install)"]
        end

        cloudinit["Cloud-Init Bootstrap"]

        subgraph agent["spuff-agent"]
            services["services<br/>(docker)"]
            repos["repos<br/>(git)"]
            setup["setup<br/>(scripts)"]
            hooks["hooks<br/>(post_up)"]
        end

        ports["ports: [3000, 8080] → SSH Tunnel (spuff ssh)"]
    end

    resources --> cloudinit
    bundles --> cloudinit
    packages --> cloudinit
    cloudinit --> agent
```

### File Discovery

Spuff implementations MUST search for configuration files using the following algorithm:

1. Start from the current working directory
2. Look for `spuff.yaml` or `spuff.yml` (in that order)
3. If not found, move to the parent directory
4. Repeat until a configuration file is found or the filesystem root is reached

The directory containing the discovered configuration file is considered the **project root**.

### Secrets File

A separate `spuff.secrets.yaml` file MAY be placed alongside the main configuration file. This file:

- MUST contain only an `env` section
- MUST NOT be committed to version control
- Values MUST override corresponding values in the main configuration
- SHOULD be added to `.gitignore`

```yaml
# spuff.secrets.yaml
env:
  DATABASE_PASSWORD: super-secret
  API_KEY: sk-xxx
```

---

## Version top-level element

```yaml
version: "1"
```

**Type:** `string`
**Default:** `"1"`
**Required:** No

The `version` attribute defines the specification version. This attribute is OPTIONAL and defaults to `"1"`.

Implementations MUST reject configurations with unsupported version numbers. Implementations SHOULD provide clear error messages when encountering unknown versions.

---

## Name top-level element

```yaml
name: my-project
```

**Type:** `string`
**Default:** Directory name of project root
**Required:** No
**Constraints:** SHOULD be a valid identifier (alphanumeric, hyphens, underscores)

The `name` attribute defines a human-readable name for the project environment.

If omitted, implementations MUST use the name of the directory containing the configuration file.

The name is used for:
- VM instance naming (with unique suffix)
- State tracking and identification
- Logging and status display

---

## Resources top-level element

```yaml
resources:
  size: s-4vcpu-8gb
  region: nyc1
```

The `resources` element defines VM resource configuration. All sub-attributes are OPTIONAL.

### Attribute Precedence

Configuration values follow this precedence order (highest to lowest):

1. CLI flags (`--size`, `--region`)
2. Project configuration (`spuff.yaml`)
3. Global configuration (`~/.config/spuff/config.yaml`)
4. Provider defaults

### size

**Type:** `string`
**Default:** Provider-dependent
**Required:** No

Specifies the VM instance size. Valid values are provider-dependent.

**DigitalOcean examples:**

| Size | vCPUs | Memory | Disk |
|------|-------|--------|------|
| `s-1vcpu-1gb` | 1 | 1 GB | 25 GB |
| `s-2vcpu-4gb` | 2 | 4 GB | 80 GB |
| `s-4vcpu-8gb` | 4 | 8 GB | 160 GB |
| `s-8vcpu-16gb` | 8 | 16 GB | 320 GB |

### region

**Type:** `string`
**Default:** Provider-dependent
**Required:** No

Specifies the datacenter region. Valid values are provider-dependent.

**DigitalOcean examples:** `nyc1`, `nyc3`, `sfo3`, `ams3`, `fra1`, `lon1`, `sgp1`, `blr1`

---

## Bundles top-level element

```yaml
bundles:
  - rust
  - go
  - python
```

**Type:** `array<string>`
**Default:** `[]` (empty array)
**Required:** No

The `bundles` element defines pre-configured language toolchains to install. Each bundle includes the language runtime/compiler plus essential development tools (LSPs, linters, formatters, debuggers).

### Valid Bundle Identifiers

Implementations MUST support the following bundle identifiers:

| Identifier | Required Tools | Optional Tools |
|------------|----------------|----------------|
| `rust` | rustup, cargo | rust-analyzer, clippy, rustfmt, mold, cargo-watch |
| `go` | go (1.23+) | gopls, delve, golangci-lint, air |
| `python` | python3.12, pip | uv, ruff, pyright, ipython |
| `node` | node (22 LTS), npm | pnpm, typescript, eslint, prettier |
| `elixir` | erlang/OTP, elixir, mix | elixir-ls, phoenix |
| `java` | openjdk (21), maven | gradle, jdtls |
| `zig` | zig (0.13+) | zls |
| `cpp` | gcc, clang, cmake | ninja, clangd, gdb, lldb |
| `ruby` | ruby, bundler | solargraph, rubocop |

Implementations MUST return an error for unknown bundle identifiers.

### Installation Behavior

- Required tools: Installation failure MUST cause the bundle to be marked as failed
- Optional tools: Installation failure SHOULD be logged but MUST NOT cause bundle failure
- Bundles SHOULD be installed in parallel when possible
- Installation progress MUST be trackable via the agent API

---

## Packages top-level element

```yaml
packages:
  - postgresql-client
  - redis-tools
  - protobuf-compiler
  - libssl-dev
```

**Type:** `array<string>`
**Default:** `[]` (empty array)
**Required:** No

The `packages` element defines additional system packages to install via the system package manager (apt on Ubuntu/Debian).

Package names MUST be valid package identifiers for the target system. Implementations SHOULD NOT validate package names before provisioning (validation occurs at install time).

### Installation Behavior

- Packages MUST be installed after the base system is ready
- Package installation failure SHOULD be logged with the specific package name
- All packages are installed in a single transaction when possible

---

## Services top-level element

```yaml
services:
  enabled: true
  compose_file: docker-compose.yaml
  profiles:
    - dev
    - debug
```

The `services` element configures Docker Compose services. This element does NOT duplicate Docker Compose configuration; it references an existing compose file.

### enabled

**Type:** `boolean`
**Default:** `true` if compose file exists, `false` otherwise
**Required:** No

Controls whether Docker Compose services should be started.

### compose_file

**Type:** `string`
**Default:** `"docker-compose.yaml"`
**Required:** No

Path to the Docker Compose file, relative to the project root.

Implementations MUST support both `docker-compose.yaml` and `docker-compose.yml` filenames when using default discovery.

### profiles

**Type:** `array<string>`
**Default:** `[]` (empty array, all services without profile)
**Required:** No

Docker Compose profiles to activate. Corresponds to `docker compose --profile` flag.

---

## Repositories top-level element

```yaml
repositories:
  - owner/repo
  - url: git@github.com:org/backend.git
    path: ~/projects/backend
    branch: develop
```

**Type:** `array<Repository>`
**Default:** `[]` (empty array)
**Required:** No

The `repositories` element defines additional Git repositories to clone into the environment.

### Repository Formats

Repositories can be specified in two formats:

#### Short Syntax

```yaml
repositories:
  - owner/repo
```

Short syntax assumes GitHub and expands to:
- URL: `https://github.com/owner/repo.git`
- Path: `~/projects/repo`
- Branch: HEAD (default branch)

#### Long Syntax

```yaml
repositories:
  - url: git@github.com:org/backend.git
    path: ~/projects/backend
    branch: develop
```

| Attribute | Type | Default | Required | Description |
|-----------|------|---------|----------|-------------|
| `url` | string | - | Yes | Git repository URL (HTTPS or SSH) |
| `path` | string | `~/projects/<repo-name>` | No | Clone destination path |
| `branch` | string | `null` (HEAD) | No | Branch, tag, or commit to checkout |

### SSH Agent Forwarding

Implementations MUST use SSH agent forwarding for cloning operations. This allows users' local SSH keys to authenticate with private repositories without exposing keys on the VM.

---

## Env top-level element

```yaml
env:
  DATABASE_URL: postgres://localhost:5432/mydb
  RUST_LOG: debug
  LOG_LEVEL: ${LOG_LEVEL:-info}
```

**Type:** `object<string, string>`
**Default:** `{}` (empty object)
**Required:** No

The `env` element defines environment variables to be set on the VM.

### Variable Resolution

Implementations MUST support variable references in values. References are resolved from the **local environment** (where `spuff up` is executed) before being sent to the VM.

| Format | Example | Behavior |
|--------|---------|----------|
| `$VAR` | `$HOME` | Simple reference, empty string if not set |
| `${VAR}` | `${USER}` | Braced reference, empty string if not set |
| `${VAR:-default}` | `${PORT:-8080}` | With default, uses default if not set |

### Resolution Rules

1. Variable names MUST match the pattern `[a-zA-Z_][a-zA-Z0-9_]*`
2. Unset variables MUST resolve to empty string (without default) or the default value
3. Resolution MUST occur before configuration is sent to the VM
4. Literal `$` can be escaped as `$$`

### Secrets Merging

When `spuff.secrets.yaml` exists, its `env` values MUST be merged after the main configuration, overriding any duplicate keys.

---

## Setup top-level element

```yaml
setup:
  - cargo build --release
  - npm install
  - ./scripts/init-db.sh
```

**Type:** `array<string>`
**Default:** `[]` (empty array)
**Required:** No

The `setup` element defines shell commands to execute after bundles and packages are installed.

### Execution Rules

1. Commands MUST be executed in order (sequential, not parallel)
2. Commands MUST be executed in the user's home directory by default
3. If a command returns a non-zero exit code, subsequent commands MUST be skipped
4. Exit codes and output MUST be logged to `/var/log/spuff/scripts/NNN.log`
5. Commands MUST be executed as the unprivileged user, not root

### Logging

Each script MUST have its output captured in a numbered log file:

```
/var/log/spuff/scripts/
├── 001.log  # First setup command
├── 002.log  # Second setup command
└── 003.log  # Third setup command
```

---

## Ports top-level element

```yaml
ports:
  - 3000
  - 8080
  - 5432
```

**Type:** `array<integer>`
**Default:** `[]` (empty array)
**Required:** No
**Constraints:** Each value MUST be a valid port number (1-65535)

The `ports` element defines ports for automatic SSH tunneling when connecting via `spuff ssh`.

### Tunnel Behavior

For each port `N` in the array:
- Local `localhost:N` is forwarded to VM `localhost:N`
- Tunnels are established when `spuff ssh` is invoked
- Tunnels remain active for the duration of the SSH session

This allows local development tools (browsers, IDEs) to connect to services running on the remote VM.

---

## Hooks top-level element

```yaml
hooks:
  post_up: |
    echo "Environment ready!"
    make dev-setup
  pre_down: |
    make db-backup
```

The `hooks` element defines lifecycle scripts for custom automation.

### post_up

**Type:** `string`
**Default:** `null`
**Required:** No

Script executed after the environment is fully ready (all bundles, packages, services, repositories, and setup scripts complete).

### pre_down

**Type:** `string`
**Default:** `null`
**Required:** No

Script executed before VM destruction. This allows for cleanup, backups, or graceful shutdown procedures.

### Execution Rules

1. Hooks MUST be executed as shell scripts (bash)
2. Hook failures SHOULD be logged but MUST NOT prevent the operation from completing
3. Multi-line scripts SHOULD use YAML literal block syntax (`|`)

---

## Complete Configuration Example

```yaml
# spuff.yaml - Complete example
version: "1"

name: api-backend

resources:
  size: s-4vcpu-8gb
  region: fra1

bundles:
  - rust
  - python
  - node

packages:
  - postgresql-client
  - redis-tools
  - protobuf-compiler

services:
  enabled: true
  compose_file: docker-compose.yaml
  profiles:
    - dev

repositories:
  - owner/frontend
  - url: git@github.com:org/shared-libs.git
    path: ~/projects/libs
    branch: main

env:
  DATABASE_URL: postgres://dev:dev@localhost:5432/mydb
  REDIS_URL: redis://localhost:6379
  RUST_LOG: debug
  API_KEY: ${API_KEY}
  LOG_LEVEL: ${LOG_LEVEL:-info}

setup:
  - cargo build --release
  - npm install
  - pip install -r requirements.txt
  - ./scripts/init-db.sh

ports:
  - 3000
  - 8080
  - 5432
  - 6379

hooks:
  post_up: |
    echo "Environment ready!"
    make dev-setup
  pre_down: |
    make db-backup > /tmp/backup.sql
```

---

## Agent API Reference

The spuff-agent running on provisioned VMs exposes a REST API for managing project setup. All endpoints require authentication via `X-Spuff-Token` header when `SPUFF_AGENT_TOKEN` is set.

### GET /project/config

Returns the current project configuration.

**Response:** `200 OK` with JSON body containing the parsed configuration.

### GET /project/status

Returns detailed setup progress.

**Response:** `200 OK` with JSON body:

```json
{
  "started": true,
  "completed": false,
  "bundles": [
    {"name": "rust", "status": "done", "version": "1.78.0"},
    {"name": "python", "status": "in_progress", "version": null}
  ],
  "packages": {
    "status": "done",
    "installed": ["postgresql-client", "redis-tools"],
    "failed": []
  },
  "services": {
    "status": "done",
    "containers": [
      {"name": "postgres", "status": "running", "ports": ["5432"]},
      {"name": "redis", "status": "running", "ports": ["6379"]}
    ]
  },
  "repositories": [
    {"url": "git@github.com:org/frontend.git", "path": "~/projects/frontend", "status": "done"},
    {"url": "git@github.com:org/backend.git", "path": "~/projects/backend", "status": "pending"}
  ],
  "scripts": [
    {"command": "cargo build", "status": "done", "exit_code": 0},
    {"command": "npm install", "status": "pending", "exit_code": null}
  ]
}
```

### POST /project/setup

Triggers project setup. This endpoint is idempotent; calling it multiple times has no effect if setup is already in progress or complete.

**Request Body:** Project configuration JSON (optional, uses embedded config if omitted)

**Response:** `202 Accepted` if setup started, `200 OK` if already running/complete.

### Setup Status Values

| Status | Description |
|--------|-------------|
| `pending` | Not yet started |
| `in_progress` | Currently executing |
| `done` | Successfully completed |
| `failed` | Execution failed (with error message) |
| `skipped` | Intentionally skipped |

---

## Validation Rules

### Strict Validation

Implementations MUST validate:

- YAML syntax is valid
- File encoding is UTF-8
- Bundle identifiers are in the supported list
- Version string is supported

### Lenient Validation

Implementations SHOULD NOT validate:

- Package names (provider/repository dependent)
- Region codes (provider dependent)
- VM sizes (provider dependent)
- Repository URLs (format flexibility)
- Port numbers beyond basic range check

Invalid values in lenient categories will cause runtime errors during provisioning.

---

## File Format Notes

- **YAML Version:** 1.2 (via serde_yaml)
- **Encoding:** UTF-8 (MUST)
- **Indentation:** 2-space or 4-space (standard YAML)
- **Comments:** Supported with `#`
- **Multi-line Strings:** Use `|` for literal blocks, `>` for folded blocks
- **Empty Sections:** All sections are optional; omit unused sections

---

## Changelog

### Version 1 (Initial)

- Initial specification release
- Support for bundles: rust, go, python, node, elixir, java, zig, cpp, ruby
- Environment variable resolution with defaults
- Docker Compose integration
- SSH port tunneling
- Lifecycle hooks (post_up, pre_down)
- Secrets management via spuff.secrets.yaml
