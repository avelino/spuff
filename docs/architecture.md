# Spuff Technical Architecture

This document provides a deep dive into spuff's architecture, protocols, data flows, and internal workings. It is intended for engineers who want to understand or contribute to the project.

## Table of Contents

- [System Overview](#system-overview)
- [Components](#components)
- [Protocol Stack](#protocol-stack)
- [Data Flow](#data-flow)
- [Cloud Provider Integration](#cloud-provider-integration)
- [SSH/SCP Communication](#sshscp-communication)
- [Agent HTTP API](#agent-http-api)
- [Cloud-Init Provisioning](#cloud-init-provisioning)
- [State Management](#state-management)
- [Security Model](#security-model)

---

## System Overview

Spuff is a CLI tool that orchestrates ephemeral development VMs across cloud providers. The system consists of three main runtime components:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              User's Machine                                  │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │                           spuff CLI                                   │   │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────┐  ┌────────────────┐  │   │
│  │  │  Commands  │  │  Provider  │  │    SSH     │  │     State      │  │   │
│  │  │  (up/down) │  │  Adapter   │  │  Connector │  │   (SQLite)     │  │   │
│  │  └─────┬──────┘  └──────┬─────┘  └──────┬─────┘  └───────┬────────┘  │   │
│  └────────│────────────────│───────────────│────────────────│───────────┘   │
└───────────│────────────────│───────────────│────────────────│───────────────┘
            │                │               │                │
            │                │               │                │
            │    ┌───────────▼───────────┐   │                │
            │    │   HTTPS (REST API)    │   │    ┌───────────▼───────────┐
            │    │   api.digitalocean.com│   │    │  ~/.config/spuff/     │
            │    └───────────┬───────────┘   │    │  └── state.db         │
            │                │               │    └───────────────────────┘
            │                │               │
┌───────────│────────────────│───────────────│────────────────────────────────┐
│           │     Cloud Provider             │                                 │
│           │                │               │                                 │
│           │    ┌───────────▼───────────┐   │                                 │
│           │    │   Droplet/Instance    │   │                                 │
│           │    │   (Ubuntu 24.04)      │   │                                 │
│           │    │                       │◄──┘                                 │
│           │    │   ┌───────────────┐   │    SSH (TCP :22)                   │
│           │    │   │  cloud-init   │   │                                     │
│           │    │   └───────┬───────┘   │                                     │
│           │    │           │           │                                     │
│           │    │   ┌───────▼───────┐   │                                     │
│           │    │   │ spuff-agent   │   │                                     │
│           │    │   │ (HTTP :7575)  │   │                                     │
│           │    │   └───────────────┘   │                                     │
│           │    └───────────────────────┘                                     │
└───────────│──────────────────────────────────────────────────────────────────┘
            │
            └─────► stdout (TUI progress display)
```

---

## Components

### CLI (`spuff`)

The main binary that users interact with. Built with:

- **clap** for argument parsing
- **tokio** for async runtime
- **ratatui** for terminal UI
- **reqwest** for HTTP client (provider APIs)
- **rusqlite** for local state

Key modules:

- `src/cli/commands/` - Command implementations (up, down, ssh, status, etc.)
- `src/provider/` - Cloud provider abstractions and implementations
- `src/connector/ssh.rs` - SSH/SCP operations
- `src/environment/cloud_init.rs` - Cloud-init template generation
- `src/state.rs` - SQLite state management
- `src/tui/` - Terminal UI components

### Agent (`spuff-agent`)

A lightweight daemon running on the VM that provides:

- System metrics collection (CPU, memory, disk)
- Idle time tracking for auto-destruction
- Bootstrap status reporting
- Remote command execution (experimental)

Built with:

- **axum** for HTTP server
- **sysinfo** for system metrics
- **tokio** for async runtime

Key modules:

- `src/agent/main.rs` - Entry point and server setup
- `src/agent/routes.rs` - HTTP API endpoints
- `src/agent/metrics.rs` - System metrics collection

### Cloud-Init

YAML configuration that bootstraps the VM:

- User creation and SSH key injection
- Package installation
- Tool installation (Docker, devbox, etc.)
- Agent installation and startup

Generated from Tera templates in `src/environment/cloud_init.rs`.

---

## Protocol Stack

Spuff uses three distinct communication protocols:

| Protocol | Use Case | Port | Encryption |
|----------|----------|------|------------|
| HTTPS | Cloud Provider API | 443 | TLS 1.2+ |
| SSH | Remote shell & SCP | 22 | SSH protocol |
| HTTP | Agent API | 7575 | None (localhost) |

### Protocol Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            spuff up --dev                                    │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 1. HTTPS POST api.digitalocean.com/v2/droplets                              │
│    Headers: Authorization: Bearer <token>                                    │
│    Body: { name, region, size, image, user_data, ssh_keys, tags }           │
│    Response: { droplet: { id: 123456, status: "new" } }                     │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 2. HTTPS GET api.digitalocean.com/v2/droplets/123456  (polling)             │
│    Response: { droplet: { status: "active", networks: { v4: [ip] } } }      │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 3. TCP connect to <ip>:22 (wait for SSH port)                               │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 4. SSH login test: ssh -o BatchMode=yes dev@<ip> echo ok                    │
│    (retry until cloud-init creates user)                                    │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 5. [--dev only] SCP upload: spuff-agent → /opt/spuff/spuff-agent            │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 6. SSH commands to monitor cloud-init progress                              │
│    - tail -200 /var/log/cloud-init-output.log                               │
│    - cloud-init status --format=json                                        │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ 7. SSH interactive session: ssh -A dev@<ip>                                 │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Data Flow

### Instance Creation (`spuff up`)

```rust
// Simplified flow from src/cli/commands/up.rs

async fn provision_instance(config: &AppConfig, ...) -> Result<()> {
    // 1. Generate cloud-init YAML from template
    let user_data = generate_cloud_init(config)?;  // Base64-encoded YAML

    // 2. Call provider API to create instance
    let instance = provider.create_instance(&InstanceConfig {
        name: generate_instance_name(),  // "spuff-<uuid>"
        region: config.region,
        size: config.size,
        image: "ubuntu-24-04-x64",
        user_data: Some(user_data),
        ssh_keys: ssh_key_ids,  // fetched from provider account
        tags: vec!["spuff"],
    }).await?;

    // 3. Poll until instance has public IP
    let instance = provider.wait_ready(&instance.id).await?;

    // 4. Save to local state
    db.save_instance(&Instance { id, name, ip, ... })?;

    // 5. Wait for SSH port
    wait_for_ssh(&instance.ip, 22, timeout).await?;

    // 6. Wait for user to exist (cloud-init creates it)
    wait_for_ssh_login(&instance.ip, config, timeout).await?;

    // 7. [dev mode] Upload local agent binary
    scp_upload(&instance.ip, config, "target/release/spuff-agent", "/tmp/spuff-agent").await?;
    run_command(&instance.ip, config, "sudo mv /tmp/spuff-agent /opt/spuff/").await?;

    // 8. Monitor cloud-init progress via SSH
    wait_for_cloud_init_with_progress(&instance.ip, config, &tx).await?;

    // 9. Start interactive SSH session
    connect(&instance.ip, config).await?;
}
```

### Instance Destruction (`spuff down`)

```rust
async fn execute(config: &AppConfig) -> Result<()> {
    let db = StateDb::open()?;

    // 1. Get active instance from local state
    let instance = db.get_active_instance()?;

    // 2. Call provider API to destroy
    provider.destroy_instance(&instance.id).await?;

    // 3. Remove from local state
    db.delete_instance(&instance.id)?;
}
```

---

## Cloud Provider Integration

### DigitalOcean API

Located in `src/provider/digitalocean.rs`.

**Base URL:** `https://api.digitalocean.com/v2`

**Authentication:**

```http
Authorization: Bearer <DIGITALOCEAN_TOKEN>
```

**Endpoints Used:**

| Method | Endpoint | Purpose |
|--------|----------|---------|
| POST | `/droplets` | Create instance |
| GET | `/droplets/{id}` | Get instance status |
| DELETE | `/droplets/{id}` | Destroy instance |
| GET | `/droplets?tag_name=spuff` | List spuff instances |
| GET | `/account/keys` | Get SSH key IDs |
| POST | `/droplets/{id}/actions` | Create snapshot |
| GET | `/snapshots?resource_type=droplet` | List snapshots |
| DELETE | `/snapshots/{id}` | Delete snapshot |
| GET | `/actions/{id}` | Poll action status |

**Create Droplet Request:**

```json
{
  "name": "spuff-a1b2c3d4",
  "region": "nyc1",
  "size": "s-2vcpu-4gb",
  "image": "ubuntu-24-04-x64",
  "ssh_keys": ["12345", "67890"],
  "user_data": "<base64-encoded cloud-init>",
  "tags": ["spuff"],
  "monitoring": true
}
```

**Create Droplet Response:**

```json
{
  "droplet": {
    "id": 123456789,
    "status": "new",
    "created_at": "2024-01-01T00:00:00Z",
    "networks": { "v4": [] }
  }
}
```

**Instance Status Polling:**
The CLI polls `GET /droplets/{id}` every 5 seconds until:

- `status` changes from `"new"` to `"active"`
- `networks.v4` contains a public IP address

---

## SSH/SCP Communication

Located in `src/connector/ssh.rs`.

All SSH operations use the system's `ssh` and `scp` binaries with consistent options:

```bash
# Common options for all SSH/SCP commands
-o StrictHostKeyChecking=accept-new   # Auto-accept new host keys
-o UserKnownHostsFile=/dev/null       # Don't persist host keys
-o LogLevel=ERROR                      # Suppress warnings
-o BatchMode=yes                       # Non-interactive mode
-i ~/.ssh/id_ed25519                  # Private key path
```

### SSH Functions

**wait_for_ssh(host, port, timeout)**

```rust
// TCP connection test (no SSH handshake)
// Used to detect when SSH port opens
loop {
    match TcpStream::connect(&addr).await {
        Ok(_) => break,  // Port is open
        Err(_) => sleep(2s).await,
    }
}
```

**wait_for_ssh_login(host, config, timeout)**

```rust
// Full SSH login test (waits for user to exist)
loop {
    let result = Command::new("ssh")
        .args(["-o", "BatchMode=yes", ...])
        .arg(format!("{}@{}", config.ssh_user, host))
        .arg("echo ok")
        .output().await;

    if result.status.success() {
        return Ok(());
    }
    sleep(3s).await;
}
```

**run_command(host, config, command)**

```rust
// Execute remote command and capture output
Command::new("ssh")
    .args([...common_options...])
    .arg(format!("{}@{}", config.ssh_user, host))
    .arg(command)
    .output().await
```

**scp_upload(host, config, local_path, remote_path)**

```rust
// Upload file via SCP
Command::new("scp")
    .args([...common_options...])
    .arg(local_path)
    .arg(format!("{}@{}:{}", config.ssh_user, host, remote_path))
    .output().await
```

**connect(host, config)**

```rust
// Interactive SSH session with agent forwarding
Command::new("ssh")
    .arg("-A")  // Forward SSH agent for git
    .args([...common_options...])
    .arg(format!("{}@{}", config.ssh_user, host))
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit())
    .status().await
```

### SSH Agent Forwarding

The `-A` flag enables SSH agent forwarding, allowing:

- Git operations with SSH URLs on the VM
- Access to private repositories without copying keys
- Chain SSH connections through the VM

---

## Agent HTTP API

Located in `src/agent/routes.rs`.

**Server:** Axum on `127.0.0.1:7575` (localhost only)

**Authentication:**

```http
X-Spuff-Token: <SPUFF_AGENT_TOKEN>
```

If `SPUFF_AGENT_TOKEN` env var is not set, authentication is disabled.

### Endpoints

#### GET /health (public)

```json
{
  "status": "ok",
  "service": "spuff-agent",
  "version": "0.1.0"
}
```

#### GET /status (authenticated)

```json
{
  "uptime_seconds": 3600,
  "idle_seconds": 120,
  "hostname": "spuff-a1b2c3d4",
  "cloud_init_done": true,
  "bootstrap_status": "ready",
  "bootstrap_ready": true,
  "agent_version": "0.1.0"
}
```

Bootstrap status values:

- `"unknown"` - Status file doesn't exist
- `"running"` - Bootstrap in progress
- `"ready"` - Bootstrap complete
- `"failed"` - Bootstrap encountered errors

#### GET /metrics (authenticated)

```json
{
  "cpu_usage_percent": 25.5,
  "memory_used_bytes": 1073741824,
  "memory_total_bytes": 4294967296,
  "disk_used_bytes": 5368709120,
  "disk_total_bytes": 85899345920,
  "load_average": [0.5, 0.3, 0.2],
  "timestamp": "2024-01-01T00:00:00Z"
}
```

#### GET /processes (authenticated)

Returns top 10 processes by CPU usage.

#### POST /exec (authenticated, experimental)

```json
// Request
{
  "command": "ls -la /home",
  "timeout_secs": 30
}

// Response
{
  "exit_code": 0,
  "stdout": "...",
  "stderr": "",
  "duration_ms": 15
}
```

#### POST /heartbeat (authenticated)

Resets idle timer. Returns current timestamp.

#### GET /logs?file=/var/log/syslog&lines=100 (authenticated)

Returns last N lines from log files in `/var/log/`.

#### GET /cloud-init (authenticated)

```json
{
  "status": "done",
  "done": true,
  "errors": [],
  "boot_finished": "2024-01-01T00:05:00Z"
}
```

---

## Cloud-Init Provisioning

### Template Structure

Cloud-init YAML is generated from Tera templates in `src/environment/cloud_init.rs`.

```yaml
#cloud-config

# User creation
users:
  - name: {{ username }}
    groups: [sudo, docker]
    shell: /bin/bash
    sudo: ["ALL=(ALL) NOPASSWD:ALL"]
    lock_passwd: true
    ssh_authorized_keys:
      - {{ ssh_public_key }}

# Disable root login
disable_root: true
ssh_pwauth: false

# Package management
package_update: true
package_upgrade: false
packages:
  - git
  - curl
  - vim
  - htop
  - unzip
  - build-essential

# File creation
write_files:
  # Sync bootstrap script (runs during cloud-init)
  - path: /opt/spuff/bootstrap-sync.sh
    permissions: "0755"
    content: |
      #!/bin/bash
      # Docker, basic tools, etc.

  # Async bootstrap script (runs in background)
  - path: /opt/spuff/bootstrap-async.sh
    permissions: "0755"
    content: |
      #!/bin/bash
      # devbox, node.js, claude-code, etc.

  # systemd service for agent
  - path: /etc/systemd/system/spuff-agent.service
    content: |
      [Unit]
      Description=Spuff Agent
      After=network.target

      [Service]
      Type=simple
      ExecStart=/opt/spuff/spuff-agent
      Environment=SPUFF_AGENT_TOKEN={{ agent_token }}
      Restart=always

      [Install]
      WantedBy=multi-user.target

  # Shell configuration (.bashrc)
  - path: {{ home_dir }}/.bashrc
    content: |
      # Aliases
      alias ll='eza -la'
      alias g='git'
      # ...

# Command execution
runcmd:
  - ["/opt/spuff/bootstrap-sync.sh"]
  - ["systemctl", "daemon-reload"]
  - ["systemctl", "enable", "spuff-agent"]
  - ["nohup", "/opt/spuff/bootstrap-async.sh", "&"]
```

### Two-Phase Bootstrap

To minimize time to first SSH login, bootstrap is split into two phases:

**Phase 1: Synchronous (bootstrap-sync.sh)**

- Runs during cloud-init
- Installs critical components:
  - Docker
  - Basic shell tools (fzf, bat, eza)
  - Creates directory structure
- SSH login is blocked until this completes

**Phase 2: Asynchronous (bootstrap-async.sh)**

- Runs in background via `nohup`
- Installs heavier components:
  - devbox/nix
  - Node.js
  - Claude Code CLI
  - spuff-agent download
  - Dotfiles clone
- Progress tracked via `/opt/spuff/bootstrap.status`

### Status File

The async bootstrap writes its status to `/opt/spuff/bootstrap.status`:

```bash
# During bootstrap
echo "running" > /opt/spuff/bootstrap.status

# On completion
echo "ready" > /opt/spuff/bootstrap.status

# On error
echo "failed" > /opt/spuff/bootstrap.status
```

This file is read by the agent's `/status` endpoint.

---

## State Management

Located in `src/state.rs`.

**Database:** SQLite at `~/.config/spuff/state.db`

### Schema

```sql
CREATE TABLE IF NOT EXISTS instances (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    ip TEXT NOT NULL,
    provider TEXT NOT NULL,
    region TEXT NOT NULL,
    size TEXT NOT NULL,
    created_at TEXT NOT NULL
);
```

### Operations

```rust
impl StateDb {
    pub fn open() -> Result<Self>;

    pub fn save_instance(&self, instance: &Instance) -> Result<()>;
    pub fn get_active_instance(&self) -> Result<Option<Instance>>;
    pub fn delete_instance(&self, id: &str) -> Result<()>;
    pub fn list_instances(&self) -> Result<Vec<Instance>>;
}
```

### Instance Lifecycle

```
                 ┌──────────────┐
                 │   (empty)    │
                 └──────┬───────┘
                        │
            spuff up    │
                        ▼
                 ┌──────────────┐
                 │   Instance   │  saved to state.db
                 │   created    │
                 └──────┬───────┘
                        │
          spuff down    │
          (or timeout)  │
                        ▼
                 ┌──────────────┐
                 │   Instance   │  removed from state.db
                 │   destroyed  │
                 └──────────────┘
```

---

## Security Model

### Authentication Layers

1. **Cloud Provider API**
   - Bearer token authentication
   - Token stored in env var or config file
   - Config file permissions: `0600`

2. **SSH**
   - Ed25519 key pair (or RSA)
   - Public key registered with provider
   - Private key protected by filesystem permissions
   - Optional passphrase (requires ssh-agent)

3. **Agent API**
   - Token-based authentication via `X-Spuff-Token` header
   - Server binds to localhost only (127.0.0.1)
   - Token passed via env var to agent service

### Network Security

```
Internet                    VM
   │                        │
   │    ┌───────────────────┤
   │    │ Port 22 (SSH)     │  ← Only authenticated access
   │    └───────────────────┤
   │    ┌───────────────────┤
   │    │ Port 7575 (Agent) │  ← localhost only, not exposed
   │    └───────────────────┤
   │                        │
```

### VM Security Hardening

From cloud-init:

- Root SSH login disabled (`disable_root: true`)
- Password authentication disabled (`ssh_pwauth: false`)
- User password locked (`lock_passwd: true`)
- Non-root user with sudo access
- Only SSH key authentication allowed

### Sensitive Data Handling

| Data | Storage | Protection |
|------|---------|------------|
| API Token | env var or config.yaml | File permissions (0600) |
| SSH Private Key | ~/.ssh/id_* | File permissions (0600) |
| Agent Token | env var | Process environment |
| State DB | ~/.config/spuff/state.db | File permissions |

---

## Error Handling

### SSH Errors

The system provides clear error messages for common SSH issues:

```rust
// Key requires passphrase but ssh-agent not running
if stderr.contains("Permission denied") || stderr.contains("passphrase") {
    return Err(SpuffError::Ssh(
        "SSH key requires passphrase. Run 'ssh-add' first."
    ));
}
```

### Provider API Errors

All provider API calls check HTTP status codes and parse error responses:

```rust
if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    return Err(SpuffError::Provider(format!(
        "Failed to create droplet: {} - {}",
        status, body
    )));
}
```

### Timeout Handling

Operations have configurable timeouts:

| Operation | Default Timeout |
|-----------|-----------------|
| Provider API calls | 30s |
| SSH port wait | 300s (5 min) |
| SSH login wait | 120s (2 min) |
| Cloud-init wait | 600s (10 min) |
| Agent exec command | 30s |

---

## Extending Spuff

### Adding a New Provider

1. Create `src/provider/<name>.rs`
2. Implement the `Provider` trait:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    async fn create_instance(&self, config: &InstanceConfig) -> Result<Instance>;
    async fn destroy_instance(&self, id: &str) -> Result<()>;
    async fn get_instance(&self, id: &str) -> Result<Option<Instance>>;
    async fn list_instances(&self) -> Result<Vec<Instance>>;
    async fn wait_ready(&self, id: &str) -> Result<Instance>;
    async fn create_snapshot(&self, instance_id: &str, name: &str) -> Result<Snapshot>;
    async fn list_snapshots(&self) -> Result<Vec<Snapshot>>;
    async fn delete_snapshot(&self, id: &str) -> Result<()>;
}
```

1. Add to `src/provider/mod.rs`:

```rust
pub fn create_provider(config: &AppConfig) -> Result<Box<dyn Provider>> {
    match config.provider.as_str() {
        "digitalocean" => Ok(Box::new(DigitalOceanProvider::new(&config.api_token)?)),
        "hetzner" => Ok(Box::new(HetznerProvider::new(&config.api_token)?)),
        _ => Err(SpuffError::Provider("Unknown provider")),
    }
}
```

### Adding Agent Endpoints

1. Add route in `src/agent/routes.rs`:

```rust
pub fn create_routes() -> Router<Arc<AppState>> {
    Router::new()
        // ... existing routes ...
        .route("/custom", get(custom_endpoint))
}

async fn custom_endpoint(
    AuthenticatedState(state): AuthenticatedState,
) -> impl IntoResponse {
    state.update_activity().await;
    Json(serde_json::json!({ "custom": "data" }))
}
```

---

## Debugging

### Enable Debug Logging

```bash
RUST_LOG=debug spuff up
```

### Inspect Cloud-Init

```bash
# On the VM
sudo cat /var/log/cloud-init-output.log
sudo cloud-init status --format=json
cat /opt/spuff/bootstrap.status
```

### Agent Status

```bash
# On the VM
sudo systemctl status spuff-agent
sudo journalctl -u spuff-agent -f
curl -H "X-Spuff-Token: $TOKEN" http://127.0.0.1:7575/status
```

### Local State

```bash
sqlite3 ~/.config/spuff/state.db "SELECT * FROM instances;"
```
