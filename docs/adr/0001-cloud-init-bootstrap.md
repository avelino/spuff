# ADR-0001: Use cloud-init for VM Bootstrap

## Status

Accepted

## Date

2025-01

## Context

When provisioning cloud VMs, we need a way to:

1. Create a non-root user with SSH access
2. Install required packages and tools
3. Configure the environment (shell, aliases, etc.)
4. Start the spuff-agent daemon
5. Do all of this automatically without manual intervention

All major cloud providers support some form of "user data" that runs on first boot. We need to choose how to format and deliver this bootstrap configuration.

### Requirements

- Works across multiple cloud providers
- Supports complex multi-step installation
- Can create users, install packages, write files
- Runs automatically on first boot
- Has good debugging/logging capabilities

## Decision

We will use **cloud-init** with YAML configuration format for VM bootstrapping.

Cloud-init is:

- The de facto standard for cloud instance initialization
- Supported by all major cloud providers (DigitalOcean, AWS, GCP, Azure, Hetzner)
- Well-documented with extensive module support
- Logs to `/var/log/cloud-init-output.log` for debugging

### Implementation

1. Generate cloud-init YAML using Tera templates (`src/environment/cloud_init.rs`)
2. Base64-encode the YAML for provider API compatibility
3. Pass as `user_data` in instance creation request
4. Cloud-init executes on first boot

### cloud-init Structure

```yaml
#cloud-config
users:
  - name: dev
    groups: [sudo, docker]
    shell: /bin/bash
    ssh_authorized_keys: [...]

package_update: true
packages: [git, curl, vim, ...]

write_files:
  - path: /opt/spuff/bootstrap.sh
    content: |
      #!/bin/bash
      # Installation script

runcmd:
  - ["/opt/spuff/bootstrap.sh"]
```

## Consequences

### Positive

- **Universal compatibility**: Works with every major cloud provider
- **Declarative configuration**: YAML is readable and maintainable
- **Built-in features**: User creation, package installation, file writing
- **Good logging**: `/var/log/cloud-init-output.log` aids debugging
- **No SSH required**: Runs before network access is available
- **Idempotent**: Can be re-run safely

### Negative

- **YAML complexity**: Complex scripts in YAML can be hard to read
- **Debugging difficulty**: Errors only visible in logs after boot
- **Provider variations**: Some providers have quirks with user-data handling
- **Size limits**: Some providers limit user-data size (~64KB typical)

### Neutral

- Requires understanding of cloud-init modules and syntax
- Templates add a layer of indirection

## Alternatives Considered

### Alternative 1: Packer Images

Pre-build VM images with all tools installed using Packer.

**Pros:**

- Faster boot times (no installation during boot)
- Consistent, tested images

**Cons:**

- Need to maintain images per provider/region
- Image updates require rebuild and redistribute
- Storage costs for images

**Why rejected:** Too much operational overhead for the initial version. cloud-init provides flexibility during rapid development. May revisit for production optimization.

### Alternative 2: SSH + Bash Scripts

SSH into the VM after boot and run setup scripts directly.

**Pros:**

- Simpler debugging (interactive SSH)
- More control over execution order

**Cons:**

- Requires SSH to be ready first
- Adds latency to provisioning
- Network-dependent

**Why rejected:** Adds complexity and latency. cloud-init runs before we need SSH access.

### Alternative 3: Ansible/Configuration Management

Use Ansible or similar tools for configuration.

**Pros:**

- Powerful configuration management
- Declarative, idempotent

**Cons:**

- Additional dependency
- Overkill for our use case
- Requires SSH access

**Why rejected:** Overkill for bootstrapping ephemeral VMs. cloud-init is sufficient.

## References

- [cloud-init Documentation](https://cloudinit.readthedocs.io/)
- [DigitalOcean cloud-init Support](https://docs.digitalocean.com/products/droplets/how-to/automate-setup-with-cloud-init/)
- [Tera Templates](https://keats.github.io/tera/)
