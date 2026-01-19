# ADR-0006: Project Configuration Specification (`spuff.yaml`)

## Status

Accepted

## Date

2025-01-19

## Context

spuff currently uses a global configuration file (`~/.config/spuff/config.yaml`) for all environments. While this works for basic usage, teams and projects often need:

1. **Reproducible environments** - Every developer should get the same setup
2. **Project-specific tooling** - Different projects need different language stacks
3. **Infrastructure as code** - Environment configuration should be versioned with the codebase
4. **Automatic dependency installation** - No manual setup steps after `spuff up`

Without project-level configuration:

- Developers must manually install tools after VM creation
- Environments drift between team members
- Onboarding requires documentation and manual steps
- Environment setup is not reproducible or auditable

## Decision

We will implement a **project-level configuration specification** via `spuff.yaml` that:

### 1. Configuration File

- Single file: `spuff.yaml` in project root
- Optional secrets file: `spuff.secrets.yaml` (gitignored)
- YAML format for readability and compatibility
- Discovery: search upward from CWD to find config

### 2. Key Features

**Language Bundles**: Pre-configured toolchains that include compiler/runtime + LSPs + linters + formatters:

- rust, go, python, node, elixir, java, zig, cpp, ruby
- Each bundle is self-contained and tested

**Resource Overrides**: Project can specify VM size/region (CLI args take precedence)

**Docker Services Integration**: Uses existing `docker-compose.yaml` instead of duplicating configuration

**Repository Cloning**: Automatically clone related repositories with SSH agent forwarding

**Environment Variables**: Support for `$VAR`, `${VAR}`, and `${VAR:-default}` resolution from host

**Setup Scripts**: Ordered list of commands executed after packages/bundles install

**Port Tunneling**: Declare ports for automatic SSH tunnel setup via `spuff ssh`

### 3. Implementation Architecture

```
CLI (spuff up)
    │
    ├─ Load spuff.yaml from CWD
    ├─ Merge with global config
    ├─ Embed as project.json in cloud-init
    │
    └─ VM boots → agent starts
         │
         └─ Agent reads /opt/spuff/project.json
              │
              ├─ Install bundles (async)
              ├─ Install packages
              ├─ Clone repositories
              ├─ Start services (docker-compose)
              ├─ Run setup scripts
              └─ Execute hooks
```

### 4. What We Will NOT Do

- **No config inheritance/extends** - Simplicity first; can add later
- **No IDE extension management** - Environment is terminal-focused
- **No lock file** - Consider for future version
- **No secrets management service** - Use local files + env vars

## Consequences

### Positive

- **Reproducible environments** - `git clone` + `spuff up` = working environment
- **Self-documenting** - Configuration shows what the project needs
- **Version controlled** - Changes are tracked and auditable
- **Team alignment** - Everyone uses the same tooling
- **Faster onboarding** - No manual setup steps
- **Composable** - Can share configs between similar projects

### Negative

- **Initial setup cost** - Projects need to create spuff.yaml
- **Increased complexity** - More configuration to understand
- **Potential conflicts** - Project config vs global config can confuse users
- **Bundle maintenance** - Need to keep bundle scripts updated

### Neutral

- Moves setup responsibility from developers to the configuration file
- Requires agent to handle async installation tasks

## Alternatives Considered

### Alternative 1: Nix Flakes

Use Nix for environment definition (like devenv.sh).

**Pros:**

- Extremely reproducible
- Large package ecosystem
- Declarative and functional

**Cons:**

- Steep learning curve
- Nix-specific syntax
- Slow initial builds
- Not familiar to most developers

**Why rejected:** Nix is powerful but adds significant complexity. Our target users want simple YAML, not a new language.

### Alternative 2: Devcontainers

Use VS Code's devcontainer.json specification.

**Pros:**

- Industry standard
- VS Code integration
- Container-based isolation

**Cons:**

- VS Code-centric
- Docker dependency
- Doesn't fit our VM-based model

**Why rejected:** We provision VMs, not containers. Different paradigm.

### Alternative 3: Terraform/Pulumi

Full IaC tools for environment definition.

**Pros:**

- Extremely powerful
- Multi-cloud support
- State management

**Cons:**

- Overkill for dev environments
- Slow iteration
- Complex for simple use cases

**Why rejected:** These tools are designed for production infrastructure, not ephemeral dev environments.

### Alternative 4: Shell Scripts Only

Use a `setup.sh` script in each project.

**Pros:**

- No new concepts
- Full flexibility
- Easy to understand

**Cons:**

- Not declarative
- Hard to track progress
- No structure or validation
- Can't show nice status output

**Why rejected:** We want declarative configuration with structured output and progress tracking.

## References

- [docs/project-config.md](../project-config.md) - User documentation
- [src/project_config.rs](../../src/project_config.rs) - CLI parsing implementation
- [src/agent/project_setup.rs](../../src/agent/project_setup.rs) - Agent setup handler
- [Devcontainers specification](https://containers.dev/) - Inspiration (but different approach)
- [devenv.sh](https://devenv.sh/) - Nix-based alternative
