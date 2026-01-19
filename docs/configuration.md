# Configuration Reference

This document describes all configuration options available in spuff's `config.yaml` file.

## File Location

The configuration file is located at:

```
~/.config/spuff/config.yaml
```

To create or edit the configuration:

```bash
spuff init          # Interactive setup (creates config.yaml)
spuff config show   # Display current configuration
spuff config edit   # Open in $EDITOR
spuff config set <key> <value>  # Set individual values
```

## Complete Example

```yaml
# Cloud provider configuration
provider: digitalocean
region: nyc1
size: s-2vcpu-4gb

# VM lifecycle
idle_timeout: 2h
environment: devbox

# SSH configuration
ssh_key_path: ~/.ssh/id_ed25519
ssh_user: dev

# Optional: Dotfiles repository
dotfiles: https://github.com/yourusername/dotfiles

# Optional: Tailscale VPN
tailscale_enabled: false
tailscale_authkey: tskey-auth-xxxxx

# Optional: Agent authentication
agent_token: your-secret-token
```

---

## Configuration Options

### `provider`

**Type:** `string`
**Required:** Yes
**Default:** `digitalocean`

The cloud provider to use for creating VMs.

```yaml
provider: digitalocean
```

**Supported values:**

| Provider | Status | Description |
|----------|--------|-------------|
| `digitalocean` | Stable | DigitalOcean Droplets |
| `hetzner` | Planned | Hetzner Cloud |
| `aws` | Planned | Amazon EC2 |

---

### `region`

**Type:** `string`
**Required:** Yes
**Default:** `nyc1`

The geographic region where your VM will be created. Lower latency = faster connection.

```yaml
region: nyc1
```

**DigitalOcean regions:**

| Region | Location |
|--------|----------|
| `nyc1`, `nyc3` | New York, USA |
| `sfo3` | San Francisco, USA |
| `ams3` | Amsterdam, Netherlands |
| `sgp1` | Singapore |
| `lon1` | London, UK |
| `fra1` | Frankfurt, Germany |
| `tor1` | Toronto, Canada |
| `blr1` | Bangalore, India |
| `syd1` | Sydney, Australia |

**Tip:** Choose the region closest to you for best performance.

**Override at runtime:**

```bash
spuff up --region fra1
```

---

### `size`

**Type:** `string`
**Required:** Yes
**Default:** `s-2vcpu-4gb`

The VM size/type determining CPU, memory, and cost.

```yaml
size: s-2vcpu-4gb
```

**DigitalOcean sizes:**

| Size | vCPUs | Memory | Disk | Price/hour |
|------|-------|--------|------|------------|
| `s-1vcpu-1gb` | 1 | 1 GB | 25 GB | $0.009 |
| `s-1vcpu-2gb` | 1 | 2 GB | 50 GB | $0.018 |
| `s-2vcpu-2gb` | 2 | 2 GB | 60 GB | $0.027 |
| `s-2vcpu-4gb` | 2 | 4 GB | 80 GB | $0.036 |
| `s-4vcpu-8gb` | 4 | 8 GB | 160 GB | $0.071 |
| `s-8vcpu-16gb` | 8 | 16 GB | 320 GB | $0.143 |

**Recommended:**

- Light development: `s-2vcpu-4gb` (default)
- Heavy builds/Claude Code: `s-4vcpu-8gb`
- CI/testing: `s-1vcpu-2gb`

**Override at runtime:**

```bash
spuff up --size s-4vcpu-8gb
```

---

### `idle_timeout`

**Type:** `string` (duration)
**Required:** Yes
**Default:** `2h`

Time of inactivity after which the VM will be automatically destroyed. This prevents forgotten instances and surprise bills.

```yaml
idle_timeout: 2h
```

**Duration formats:**

| Format | Example | Description |
|--------|---------|-------------|
| `Nh` | `2h` | N hours |
| `Nm` | `30m` | N minutes |
| `Ns` | `3600s` | N seconds |
| `N` | `7200` | N seconds (raw) |

**Examples:**

```yaml
idle_timeout: 30m    # 30 minutes
idle_timeout: 2h     # 2 hours (recommended)
idle_timeout: 4h     # 4 hours
idle_timeout: 24h    # 24 hours (use with caution)
```

**How it works:**

1. The `spuff-agent` running on the VM monitors activity
2. Activity includes: SSH sessions, CPU usage, network traffic
3. When no activity for the configured duration, the VM self-destructs
4. Snapshots can be created automatically before destruction

---

### `environment`

**Type:** `string`
**Required:** Yes
**Default:** `devbox`

The base environment type that determines what tools are pre-installed.

```yaml
environment: devbox
```

**Supported values:**

| Environment | Description |
|-------------|-------------|
| `devbox` | Modern shell (zsh), Docker, Git, development tools |
| `nix` | Nix package manager environment (planned) |
| `minimal` | Bare minimum (planned) |

---

### `ssh_key_path`

**Type:** `string` (file path)
**Required:** Yes
**Default:** `~/.ssh/id_ed25519`

Path to your SSH private key file. This key is used to:

1. Authenticate with the cloud provider (public key registered)
2. Connect to the VM via SSH
3. Forward your SSH agent for git operations

```yaml
ssh_key_path: ~/.ssh/id_ed25519
```

**Supported key types:**

- `id_ed25519` (recommended)
- `id_rsa`
- `id_ecdsa`

**Notes:**

- The path supports `~` expansion
- The corresponding `.pub` file must exist
- The public key is registered with your cloud provider

**Examples:**

```yaml
ssh_key_path: ~/.ssh/id_ed25519           # Default
ssh_key_path: ~/.ssh/spuff_key            # Custom key
ssh_key_path: /home/user/.ssh/id_rsa      # Absolute path
```

---

### `ssh_user`

**Type:** `string`
**Required:** No
**Default:** `dev`

The SSH username for connecting to the VM. This user is created with passwordless sudo access.

```yaml
ssh_user: dev
```

**Notes:**

- Default is `dev` (non-root for security)
- User is created with `sudo` and `docker` groups
- Root SSH login is disabled by default
- Password authentication is disabled (SSH key only)

---

### `dotfiles`

**Type:** `string` (URL) or `null`
**Required:** No
**Default:** `null` (not set)

URL to a git repository containing your dotfiles. When set, spuff will clone and apply your dotfiles on VM creation.

```yaml
dotfiles: https://github.com/yourusername/dotfiles
```

**Supported formats:**

```yaml
# HTTPS (recommended for public repos)
dotfiles: https://github.com/user/dotfiles

# SSH (requires SSH agent forwarding)
dotfiles: git@github.com:user/dotfiles.git
```

**How it works:**

1. Repository is cloned to `~/dotfiles`
2. If `install.sh` exists, it's executed
3. If `Makefile` exists with `install` target, `make install` is run
4. Otherwise, symlinks are created for common dotfiles

**Tip:** Keep your dotfiles repo lightweight for faster VM startup.

---

### `tailscale_enabled`

**Type:** `boolean`
**Required:** No
**Default:** `false`

Enable Tailscale VPN integration. When enabled, your VM joins your Tailscale network for secure private access.

```yaml
tailscale_enabled: true
```

**Benefits:**

- Access VM via private Tailscale IP (no public exposure)
- Persistent hostname across VM recreations
- Secure mesh networking
- Access from anywhere without port forwarding

---

### `tailscale_authkey`

**Type:** `string` or `null`
**Required:** No (required if `tailscale_enabled: true`)
**Default:** `null` (not set)

Your Tailscale authentication key for automatic VM enrollment.

```yaml
tailscale_authkey: tskey-auth-xxxxxxxxxxxxx
```

**Getting an auth key:**

1. Go to [Tailscale Admin Console](https://login.tailscale.com/admin/settings/keys)
2. Click "Generate auth key"
3. Settings:
   - Reusable: Yes (for multiple VMs)
   - Ephemeral: Yes (auto-removes when VM destroyed)
   - Pre-authorized: Optional
4. Copy the key (starts with `tskey-auth-`)

**Alternative:** Use environment variable

```bash
export TS_AUTHKEY="tskey-auth-xxxxxxxxxxxxx"
```

---

### `agent_token`

**Type:** `string` or `null`
**Required:** No
**Default:** `null` (auto-generated)

Authentication token for the spuff-agent API running on the VM. Protects the agent's HTTP endpoints from unauthorized access.

```yaml
agent_token: your-secret-token-here
```

**How it works:**

1. If set, all agent API requests require `X-Spuff-Token` header
2. The CLI automatically includes this token in requests
3. If not set, a random token is generated during VM creation

**Alternative:** Use environment variable

```bash
export SPUFF_AGENT_TOKEN="your-secret-token"
```

**Security note:** The agent token protects endpoints that expose system metrics and can execute commands. Always use a strong, unique token.

---

## Environment Variables

API tokens and secrets can be provided via environment variables instead of (or in addition to) the config file:

| Variable | Description | Priority |
|----------|-------------|----------|
| `SPUFF_API_TOKEN` | Cloud provider API token | Highest |
| `DIGITALOCEAN_TOKEN` | DigitalOcean API token | Provider-specific |
| `HETZNER_TOKEN` | Hetzner API token | Provider-specific |
| `AWS_ACCESS_KEY_ID` | AWS access key | Provider-specific |
| `SPUFF_AGENT_TOKEN` | Agent authentication token | Override config |
| `TS_AUTHKEY` | Tailscale auth key | Override config |

**Priority:** Environment variables take precedence over config file values.

**Example setup:**

```bash
# Add to ~/.bashrc or ~/.zshrc
export DIGITALOCEAN_TOKEN="dop_v1_xxxxxxxxxxxxxxxx"
export SPUFF_AGENT_TOKEN="my-secret-agent-token"
```

---

## CLI Commands

### View Configuration

```bash
spuff config show
```

Output:

```
Current Configuration

  Provider:     digitalocean
  Region:       nyc1
  Size:         s-2vcpu-4gb
  Idle timeout: 2h
  Environment:  devbox
  Dotfiles:     https://github.com/user/dotfiles
  SSH key:      ~/.ssh/id_ed25519
  Tailscale:    enabled

Config file: /home/user/.config/spuff/config.yaml
```

### Set Individual Values

```bash
spuff config set region fra1
spuff config set size s-4vcpu-8gb
spuff config set idle_timeout 4h
spuff config set dotfiles https://github.com/user/dotfiles
spuff config set tailscale true
```

**Available keys:**

- `provider`
- `region`
- `size`
- `idle_timeout` (or `idle-timeout`)
- `environment`
- `dotfiles`
- `ssh_key` (or `ssh-key`)
- `ssh_user` (or `ssh-user`)
- `tailscale`

### Edit in Editor

```bash
spuff config edit
```

Opens the config file in your `$EDITOR` (defaults to `vim`).

---

## Runtime Overrides

Many configuration values can be overridden at runtime:

```bash
# Override size and region for this run only
spuff up --size s-4vcpu-8gb --region fra1

# Use a specific snapshot
spuff up --snapshot snap-123456

# Create without connecting
spuff up --no-connect

# Development mode (upload local agent)
spuff up --dev
```

---

## File Permissions

The config file is created with restricted permissions (`0600`) to protect sensitive data like tokens. If you edit the file manually, ensure proper permissions:

```bash
chmod 600 ~/.config/spuff/config.yaml
```

---

## Example Configurations

### Minimal (default)

```yaml
provider: digitalocean
region: nyc1
size: s-2vcpu-4gb
idle_timeout: 2h
environment: devbox
ssh_key_path: ~/.ssh/id_ed25519
ssh_user: dev
```

### Power User

```yaml
provider: digitalocean
region: fra1
size: s-4vcpu-8gb
idle_timeout: 4h
environment: devbox
ssh_key_path: ~/.ssh/id_ed25519
ssh_user: dev
dotfiles: https://github.com/myuser/dotfiles
tailscale_enabled: true
tailscale_authkey: tskey-auth-xxxxxxxxxxxxx
agent_token: super-secret-token-12345
```

### CI/Testing

```yaml
provider: digitalocean
region: nyc1
size: s-1vcpu-2gb
idle_timeout: 30m
environment: devbox
ssh_key_path: ~/.ssh/ci_key
ssh_user: ci
```

---

## Troubleshooting

### Config not found

```
Error: Config file not found: ~/.config/spuff/config.yaml. Run 'spuff init' first.
```

**Solution:** Run `spuff init` to create the configuration file.

### Invalid config

```
Error: Invalid config: missing field `region`
```

**Solution:** Add the missing field or run `spuff init` to regenerate.

### Token not found

```
Error: API token not configured
```

**Solution:** Either:

1. Add `api_token` to config.yaml (not recommended)
2. Set environment variable: `export DIGITALOCEAN_TOKEN="..."`
3. Set generic variable: `export SPUFF_API_TOKEN="..."`

### SSH key not found

```
Error: SSH key not found at ~/.ssh/id_ed25519
```

**Solution:**

1. Generate a key: `ssh-keygen -t ed25519`
2. Or update `ssh_key_path` to point to existing key
