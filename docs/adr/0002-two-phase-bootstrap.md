# ADR-0002: Two-Phase Bootstrap (Sync + Async)

## Status

Accepted

## Date

2025-01

## Context

VM bootstrapping involves installing many tools and dependencies:

**Essential (needed immediately):**

- Docker
- Basic shell tools (git, curl)
- User setup
- SSH access

**Nice-to-have (can wait):**

- devbox/nix
- Node.js
- Claude Code CLI
- Dotfiles
- spuff-agent download

If we install everything synchronously, the user waits several minutes before SSH access is available. This creates a poor first impression.

### Requirements

- SSH should be available as fast as possible
- Essential tools must be ready before user connects
- Long-running installations shouldn't block access
- User should see progress of background tasks

## Decision

We will split bootstrap into **two phases**:

### Phase 1: Synchronous (bootstrap-sync.sh)

Runs during cloud-init, blocks until complete:

- Docker installation
- Basic shell tools (fzf, bat, eza, starship)
- Directory structure creation
- Essential configuration

### Phase 2: Asynchronous (bootstrap-async.sh)

Runs in background via `nohup`, doesn't block:

- devbox/nix installation
- Node.js and npm
- Claude Code CLI
- Dotfiles cloning
- spuff-agent download (if not using --dev)

### Progress Tracking

The async script writes status to `/opt/spuff/bootstrap.status`:

- `running` - Bootstrap in progress
- `ready` - All done
- `failed` - Error occurred

The spuff-agent reads this file and exposes it via the `/status` endpoint.

### Implementation

In cloud-init:

```yaml
runcmd:
  # Phase 1: Sync (blocks)
  - ["/opt/spuff/bootstrap-sync.sh"]

  # Start agent
  - ["systemctl", "start", "spuff-agent"]

  # Phase 2: Async (background)
  - ["nohup", "/opt/spuff/bootstrap-async.sh", "&"]
```

## Consequences

### Positive

- **Fast SSH access**: User can connect in ~2-3 minutes instead of 5-7
- **Better UX**: User sees progress, not just waiting
- **Parallel work**: User can work while background tasks complete
- **Flexibility**: Easy to move items between phases

### Negative

- **Complexity**: Two scripts instead of one
- **State management**: Need to track async progress
- **Potential confusion**: User might try to use tools before they're ready
- **Error handling**: Async errors are less visible

### Neutral

- spuff-agent starts before full bootstrap completes
- TUI shows bootstrap progress to user

## Alternatives Considered

### Alternative 1: Everything Synchronous

Install all tools in a single synchronous script.

**Pros:**

- Simpler implementation
- Guaranteed everything ready when SSH available

**Cons:**

- Long wait time (5-7 minutes)
- Poor user experience
- Can't work while waiting

**Why rejected:** User experience is a priority. Waiting 5+ minutes is unacceptable.

### Alternative 2: Pre-built Images

Use pre-built images with everything installed.

**Pros:**

- Instant readiness
- Consistent environment

**Cons:**

- Image maintenance burden
- Storage costs
- Less flexibility for customization

**Why rejected:** May implement later, but cloud-init provides flexibility during development.

### Alternative 3: On-Demand Installation

Only install tools when user first uses them.

**Pros:**

- Fastest initial boot
- Only install what's needed

**Cons:**

- Delayed experience when using tools
- Complex detection of "first use"
- Confusing errors

**Why rejected:** Unexpected delays are worse than known upfront wait.

## References

- [ADR-0001: cloud-init Bootstrap](0001-cloud-init-bootstrap.md)
- [nohup Documentation](https://man7.org/linux/man-pages/man1/nohup.1.html)
