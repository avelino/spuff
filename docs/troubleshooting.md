# Troubleshooting Guide

This guide helps diagnose and resolve common issues with spuff.

## Quick Diagnostics

```bash
# Check spuff version
spuff --version

# Check current status
spuff status

# Check with debug logging
RUST_LOG=debug spuff status

# Check local state
sqlite3 ~/.config/spuff/state.db "SELECT * FROM instances;"
```

---

## SSH Issues

### "Permission denied (publickey)"

**Symptoms:**
```
Error: SSH connection failed: Permission denied (publickey)
```

**Causes & Solutions:**

1. **SSH key not in agent**
   ```bash
   # Check if key is loaded
   ssh-add -l

   # Add key to agent
   eval "$(ssh-agent -s)"
   ssh-add ~/.ssh/id_ed25519
   ```

2. **Wrong key configured**
   ```bash
   # Check which key spuff uses
   spuff config show

   # Update if needed
   spuff config set ssh_key_path ~/.ssh/correct_key
   ```

3. **Key not uploaded to provider**
   - Go to DigitalOcean dashboard
   - Settings > Security > SSH Keys
   - Add your public key: `cat ~/.ssh/id_ed25519.pub`

4. **Key permissions wrong**
   ```bash
   chmod 600 ~/.ssh/id_ed25519
   chmod 644 ~/.ssh/id_ed25519.pub
   ```

### "Connection refused" on port 22

**Symptoms:**
```
Error: SSH connection failed: Connection refused
```

**Causes & Solutions:**

1. **VM still booting**
   - Wait a few more seconds
   - SSH service starts after cloud-init user creation

2. **Firewall blocking**
   - Check provider firewall/security groups
   - Ensure port 22 is open

3. **Instance not running**
   ```bash
   # Check instance status
   spuff status --detailed
   ```

### "Host key verification failed"

**Symptoms:**
```
Host key verification failed.
```

**Cause:** Previous VM had same IP, different host key.

**Solution:**
```bash
# Remove old host key
ssh-keygen -R <vm-ip>

# Or clear all known hosts
> ~/.ssh/known_hosts
```

### SSH key requires passphrase

**Symptoms:**
```
Enter passphrase for key '/home/user/.ssh/id_ed25519':
```

**Solution:**
```bash
# Add key to agent with passphrase
ssh-add ~/.ssh/id_ed25519
# Enter passphrase once

# Verify
ssh-add -l
```

---

## VM Creation Issues

### "API token not configured"

**Symptoms:**
```
Error: API token not configured. Set DIGITALOCEAN_TOKEN environment variable.
```

**Solution:**
```bash
# Set token
export DIGITALOCEAN_TOKEN="dop_v1_xxxxxxxxxxxx"

# Or add to shell profile
echo 'export DIGITALOCEAN_TOKEN="dop_v1_xxxx"' >> ~/.bashrc
source ~/.bashrc
```

### "Invalid region"

**Symptoms:**
```
Error: Region 'xxx' not found
```

**Solution:**
```bash
# Use valid region
spuff config set region nyc1

# Valid regions: nyc1, nyc3, sfo3, ams3, sgp1, lon1, fra1, tor1, blr1
```

### "Quota exceeded"

**Symptoms:**
```
Error: You have reached the droplet limit for your account
```

**Solutions:**
1. Destroy existing instances: `spuff down --force`
2. Request quota increase from provider
3. Check for orphaned instances in provider dashboard

### "SSH key not found"

**Symptoms:**
```
Error: SSH key not found in your account
```

**Cause:** Public key not registered with cloud provider.

**Solution:**
1. Copy public key: `cat ~/.ssh/id_ed25519.pub`
2. Add to provider:
   - DigitalOcean: Settings > Security > SSH Keys > Add SSH Key

---

## Cloud-Init Issues

### Bootstrap never completes

**Symptoms:**
- `spuff agent status` shows `bootstrap_status: running` forever
- Can SSH in but tools not installed

**Diagnosis:**
```bash
# SSH to VM
spuff ssh

# Check cloud-init status
cloud-init status --format=json

# Check for errors
sudo grep -i error /var/log/cloud-init-output.log

# Check async bootstrap
cat /opt/spuff/bootstrap.status
sudo cat /var/log/spuff-bootstrap.log
```

**Common causes:**
1. Network timeout downloading packages
2. Package repository issues
3. Script syntax error

### Docker not installed

**Symptoms:**
```
docker: command not found
```

**Diagnosis:**
```bash
# Check if install ran
sudo grep -i docker /var/log/cloud-init-output.log

# Try manual install
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker $USER
```

### Shell aliases not working

**Symptoms:**
```
ll: command not found
```

**Cause:** `.bashrc` or `.profile` not properly configured.

**Solution:**
```bash
# Check .profile exists (needed for login shells)
cat ~/.profile

# Check .bashrc has aliases
grep "alias ll" ~/.bashrc

# Source manually
source ~/.bashrc
```

---

## Agent Issues

### "Agent not responding"

**Symptoms:**
```
Error: Failed to connect to agent at 127.0.0.1:7575
```

**Diagnosis:**
```bash
# SSH to VM
spuff ssh

# Check agent status
sudo systemctl status spuff-agent

# Check agent logs
sudo journalctl -u spuff-agent -n 50

# Test agent locally
curl http://127.0.0.1:7575/health
```

**Solutions:**

1. **Agent not running**
   ```bash
   sudo systemctl start spuff-agent
   ```

2. **Agent crashed**
   ```bash
   sudo journalctl -u spuff-agent --since "5 minutes ago"
   # Check for crash reason
   ```

3. **Binary not found**
   ```bash
   ls -la /opt/spuff/spuff-agent
   # If missing, download or re-create VM
   ```

### "Unauthorized" from agent

**Symptoms:**
```
Error: Agent returned 401 Unauthorized
```

**Cause:** Token mismatch between CLI and agent.

**Solution:**
```bash
# Check token on VM
ssh dev@<ip> 'echo $SPUFF_AGENT_TOKEN'

# Or check systemd environment
ssh dev@<ip> 'sudo systemctl show spuff-agent --property=Environment'
```

---

## State Issues

### "No active instance"

**Symptoms:**
```
Error: No active instance found
```

**Cause:** Local state doesn't know about running instance.

**Diagnosis:**
```bash
# Check local state
sqlite3 ~/.config/spuff/state.db "SELECT * FROM instances;"

# Check provider for spuff instances
curl -H "Authorization: Bearer $DIGITALOCEAN_TOKEN" \
  "https://api.digitalocean.com/v2/droplets?tag_name=spuff" | jq
```

**Solutions:**

1. **Instance exists but not in state**
   - Manually add to state, or
   - Destroy via provider dashboard and recreate

2. **Instance was deleted externally**
   ```bash
   # Clear local state
   rm ~/.config/spuff/state.db
   ```

### State out of sync

**Symptoms:**
- `spuff status` shows instance that doesn't exist
- `spuff down` fails with "not found"

**Solution:**
```bash
# Reset local state
rm ~/.config/spuff/state.db

# Recreate
spuff up
```

---

## TUI Issues

### "Device not configured" error

**Symptoms:**
```
Error: Device not configured (os error 6)
```

**Cause:** Terminal not properly initialized after subprocess.

**Solutions:**

1. **Reset terminal**
   ```bash
   reset
   ```

2. **Run with text output**
   ```bash
   spuff up 2>&1 | cat
   ```

3. **Check TTY**
   ```bash
   tty  # Should show /dev/ttys000 or similar
   ```

### TUI garbled display

**Symptoms:**
- Random characters
- Broken layout

**Solutions:**
```bash
# Reset terminal
reset

# Or use text mode
TERM=dumb spuff up
```

---

## Network Issues

### Timeout waiting for instance

**Symptoms:**
```
Error: Timeout waiting for instance to become ready
```

**Causes:**
1. Provider having issues
2. Region overloaded
3. Network issues

**Solutions:**
1. Try different region: `spuff up --region fra1`
2. Check provider status page
3. Retry after a few minutes

### Can't reach provider API

**Symptoms:**
```
Error: Request failed: connection refused
```

**Solutions:**
1. Check internet connection
2. Check if provider API is up
3. Check firewall/proxy settings

---

## Configuration Issues

### "Config file not found"

**Symptoms:**
```
Error: Config file not found at ~/.config/spuff/config.yaml
```

**Solution:**
```bash
spuff init
```

### "Invalid config"

**Symptoms:**
```
Error: Invalid config: missing field 'region'
```

**Solution:**
```bash
# View current config
cat ~/.config/spuff/config.yaml

# Recreate
rm ~/.config/spuff/config.yaml
spuff init
```

---

## Getting Help

If this guide doesn't solve your issue:

1. **Enable debug logging**
   ```bash
   RUST_LOG=debug spuff <command>
   ```

2. **Collect information**
   - spuff version
   - OS and version
   - Full error message
   - Debug logs

3. **Open an issue**
   - https://github.com/avelino/spuff/issues
   - Include collected information
   - Redact any tokens/secrets!

---

## Common Commands Reference

```bash
# Debug logging
RUST_LOG=debug spuff <command>

# Check status
spuff status --detailed

# View config
spuff config show

# Reset state
rm ~/.config/spuff/state.db

# Reset terminal
reset

# Check SSH agent
ssh-add -l

# Add SSH key
ssh-add ~/.ssh/id_ed25519

# Test SSH
ssh -v dev@<ip> echo ok

# Check cloud-init (on VM)
sudo cat /var/log/cloud-init-output.log

# Check agent (on VM)
sudo systemctl status spuff-agent
sudo journalctl -u spuff-agent -f
```
