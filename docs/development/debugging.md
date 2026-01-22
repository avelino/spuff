# Debugging Guide

This guide covers debugging techniques for spuff development.

## Logging

### Enable Debug Logging

```bash
# General debug logging
RUST_LOG=debug cargo run -- up

# Spuff-specific logging
RUST_LOG=spuff=debug cargo run -- status

# Trace level (very verbose)
RUST_LOG=spuff=trace cargo run -- up

# Multiple modules
RUST_LOG=spuff=debug,reqwest=debug cargo run -- up
```

### Log Levels

| Level | Use Case |
|-------|----------|
| `error` | Errors that stop execution |
| `warn` | Potential issues |
| `info` | Normal operation events |
| `debug` | Detailed flow information |
| `trace` | Very detailed, including data |

### Adding Logs to Code

```rust
use tracing::{debug, info, warn, error, trace};

async fn create_instance(&self, config: &InstanceConfig) -> Result<Instance> {
    info!("Creating instance: {}", config.name);
    debug!(?config, "Instance configuration");

    match self.api_call().await {
        Ok(response) => {
            trace!(?response, "API response");
            Ok(response)
        }
        Err(e) => {
            error!(%e, "Failed to create instance");
            Err(e)
        }
    }
}
```

## Debugging VM Bootstrap

### Cloud-Init Logs

SSH into the VM and check:

```bash
# Cloud-init output log
sudo cat /var/log/cloud-init-output.log

# Cloud-init status
cloud-init status --format=json

# Follow cloud-init in real-time
sudo tail -f /var/log/cloud-init-output.log
```

### Bootstrap Status

```bash
# Check async bootstrap status
cat /opt/spuff/bootstrap.status

# Check bootstrap script output
cat /var/log/spuff-bootstrap.log
```

### Agent Logs

```bash
# Agent service status
sudo systemctl status spuff-agent

# Agent logs
sudo journalctl -u spuff-agent -f

# Recent agent logs
sudo journalctl -u spuff-agent --since "5 minutes ago"
```

## Debugging SSH Issues

### Verbose SSH

```bash
# Very verbose
ssh -vvv dev@<ip>

# Check authentication
ssh -v dev@<ip> 2>&1 | grep -i auth
```

### Common SSH Issues

**Permission denied:**

```bash
# Check key permissions
ls -la ~/.ssh/

# Fix permissions
chmod 600 ~/.ssh/id_ed25519
chmod 644 ~/.ssh/id_ed25519.pub
```

**Key requires passphrase:**

```bash
# Add key to agent
eval "$(ssh-agent -s)"
ssh-add ~/.ssh/id_ed25519
```

**Host key verification:**

```bash
# Remove old host key
ssh-keygen -R <ip>
```

## Debugging Provider API

### Log API Requests

```rust
// In provider code
debug!("API request: {} {}", method, url);
trace!(?body, "Request body");

let response = client.request(method, &url).send().await?;

debug!("API response: {}", response.status());
trace!(?response_body, "Response body");
```

### Inspect with curl

```bash
# DigitalOcean API
curl -X GET \
  -H "Authorization: Bearer $DIGITALOCEAN_TOKEN" \
  "https://api.digitalocean.com/v2/droplets"

# List droplets with spuff tag
curl -X GET \
  -H "Authorization: Bearer $DIGITALOCEAN_TOKEN" \
  "https://api.digitalocean.com/v2/droplets?tag_name=spuff"
```

## Debugging Local State

### SQLite Inspection

```bash
# Open database
sqlite3 ~/.spuff/state.db

# List tables
.tables

# Show schema
.schema instances

# List instances
SELECT * FROM instances;

# Pretty print
.mode column
.headers on
SELECT * FROM instances;
```

### Reset State

```bash
# Delete state database
rm ~/.spuff/state.db

# Spuff will recreate on next run
```

## Debugging Agent

### Local Agent Testing

```bash
# Run agent locally
RUST_LOG=debug SPUFF_AGENT_TOKEN=test ./target/debug/spuff-agent

# Test endpoints
curl -H "X-Spuff-Token: test" http://127.0.0.1:7575/health
curl -H "X-Spuff-Token: test" http://127.0.0.1:7575/status
curl -H "X-Spuff-Token: test" http://127.0.0.1:7575/metrics
```

### Agent on VM

```bash
# SSH to VM
spuff ssh

# Check agent
sudo systemctl status spuff-agent
sudo journalctl -u spuff-agent -f

# Test agent locally on VM
curl -H "X-Spuff-Token: $SPUFF_AGENT_TOKEN" http://127.0.0.1:7575/status
```

## Debugging TUI

### Disable TUI

For debugging, you can disable the TUI:

```bash
# Run without TTY
cargo run -- up 2>&1 | cat

# Or set non-interactive
echo "" | cargo run -- up
```

### TUI Fallback

When TUI fails, spuff falls back to text output. Check stderr for TUI errors.

## IDE Debugging

### VS Code (CodeLLDB)

1. Install CodeLLDB extension
2. Create launch configuration:

```json
// .vscode/launch.json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "Debug spuff",
      "cargo": {
        "args": ["build", "--bin=spuff"],
        "filter": {
          "name": "spuff",
          "kind": "bin"
        }
      },
      "args": ["status"],
      "cwd": "${workspaceFolder}",
      "env": {
        "RUST_LOG": "debug",
        "DIGITALOCEAN_TOKEN": "${env:DIGITALOCEAN_TOKEN}"
      }
    }
  ]
}
```

1. Set breakpoints and press F5

### RustRover/IntelliJ

1. Create Run Configuration
2. Set binary: `spuff`
3. Set arguments: `status`
4. Set environment variables
5. Click Debug

## Common Issues

### "Device not configured" (TUI Error)

The terminal is not properly initialized:

```bash
# Reset terminal
reset

# Or run with text fallback
cargo run -- up 2>&1 | cat
```

### "Permission denied" on SSH

1. Check key is added to agent: `ssh-add -l`
2. Check key is uploaded to provider
3. Check key permissions: `chmod 600 ~/.ssh/id_*`

### Instance Not Found

State may be out of sync:

```bash
# Check local state
sqlite3 ~/.spuff/state.db "SELECT * FROM instances;"

# Check provider
curl -H "Authorization: Bearer $DIGITALOCEAN_TOKEN" \
  "https://api.digitalocean.com/v2/droplets?tag_name=spuff"
```

### Cloud-Init Never Completes

Check for errors:

```bash
# SSH in with root
ssh root@<ip>

# Check cloud-init status
cloud-init status --format=json

# Check for errors
grep -i error /var/log/cloud-init-output.log
```

## Profiling

### CPU Profiling

```bash
# Install flamegraph
cargo install flamegraph

# Profile
cargo flamegraph --bin spuff -- up

# Open flamegraph.svg
```

### Memory Profiling

```bash
# Install heaptrack (Linux)
sudo apt install heaptrack

# Profile
heaptrack ./target/release/spuff up

# Analyze
heaptrack_gui heaptrack.spuff.*.gz
```

## Useful Commands Cheatsheet

```bash
# Reset everything
rm -rf ~/.spuff/

# Clear state, keep config
rm ~/.spuff/state.db

# Debug logging
RUST_LOG=spuff=debug cargo run -- <command>

# Check VM cloud-init
ssh dev@<ip> 'sudo cat /var/log/cloud-init-output.log'

# Check agent on VM
ssh dev@<ip> 'sudo journalctl -u spuff-agent'

# API check
curl -s -H "Authorization: Bearer $DIGITALOCEAN_TOKEN" \
  "https://api.digitalocean.com/v2/droplets?tag_name=spuff" | jq
```
