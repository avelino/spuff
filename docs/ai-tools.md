# AI Coding Tools

spuff installs AI coding tools on provisioned VMs so you can use them directly in your cloud dev environment. All tools are installed via npm and available globally.

## Available Tools

| Tool | Package | Binary | Auth |
|------|---------|--------|------|
| `claude-code` | `@anthropic-ai/claude-code` | `claude` | `ANTHROPIC_API_KEY` |
| `codex` | `@openai/codex` | `codex` | `OPENAI_API_KEY` |
| `opencode` | `opencode-ai` | `opencode` | Multiple providers |
| `copilot` | `@github/copilot` | `copilot` | GitHub subscription + `GH_TOKEN` |

## Configuration

### Project config (`spuff.yaml`)

```yaml
# Install all tools (default)
ai_tools: all

# Disable all AI tools
ai_tools: none

# Install specific tools only
ai_tools:
  - claude-code
  - copilot
```

### Global config (`~/.spuff/config.yaml`)

```yaml
ai_tools: all
```

### CLI flag

```bash
spuff up --ai-tools claude-code,copilot
spuff up --ai-tools none
spuff up --ai-tools all
```

### Precedence

1. CLI `--ai-tools` flag (highest)
2. Project config (`spuff.yaml`)
3. Global config (`~/.spuff/config.yaml`)
4. Default: `all`

## CLI Commands

```bash
spuff ai list              # Show available tools and which are enabled
spuff ai status            # Check installation status on remote VM
spuff ai install <tool>    # Install a specific tool on running instance
spuff ai info <tool>       # Show tool details and auth requirements
```

### `spuff ai list`

Shows all available tools with their current enabled/disabled state based on your config:

```
Available AI Coding Tools

  claude-code - [enabled]
    Anthropic's Claude Code CLI
    Install: npm install -g @anthropic-ai/claude-code

  codex - [enabled]
    OpenAI Codex CLI
    Install: npm install -g @openai/codex

  opencode - [enabled]
    Open-source AI coding assistant
    Install: npm i -g opencode-ai

  copilot - [enabled]
    GitHub Copilot CLI
    Install: npm install -g @github/copilot
```

### `spuff ai status`

Queries the remote agent to show real-time installation status:

```
AI Tools Status

  claude-code     installed (1.0.0)
  codex           installed (0.5.0)
  opencode        installing
  copilot         pending
```

### `spuff ai install <tool>`

Installs a specific tool on a running instance without reprovisioning:

```bash
spuff ai install copilot
```

## Authentication

Each tool requires its own authentication. Pass credentials via environment variables in your `spuff.yaml`:

```yaml
env:
  ANTHROPIC_API_KEY: $ANTHROPIC_API_KEY
  OPENAI_API_KEY: $OPENAI_API_KEY
  GH_TOKEN: $GH_TOKEN
```

Or use `spuff.secrets.yaml` (not committed to git):

```yaml
# spuff.secrets.yaml
env:
  ANTHROPIC_API_KEY: sk-ant-xxx
  OPENAI_API_KEY: sk-xxx
  GH_TOKEN: ghp_xxx
```

### Claude Code

Requires `ANTHROPIC_API_KEY` environment variable.

```bash
# On the remote VM
claude
```

Documentation: https://docs.anthropic.com/claude-code

### Codex CLI

Requires `OPENAI_API_KEY` environment variable.

```bash
# On the remote VM
codex
```

Documentation: https://github.com/openai/codex-cli

### OpenCode

Supports multiple AI providers. Configure via its own config file or environment variables.

```bash
# On the remote VM
opencode
```

Documentation: https://opencode.ai

### GitHub Copilot CLI

Requires an active GitHub Copilot subscription. Authenticate via:

1. **Environment variable:** Set `GH_TOKEN` or `GITHUB_TOKEN` with a fine-grained PAT that has "Copilot Requests" permission
2. **Interactive login:** Run `copilot` then use `/login`

```bash
# On the remote VM
copilot
```

Documentation: https://github.com/github/copilot-cli

## Installation Flow

1. During `spuff up`, the AI tools config is embedded in the cloud-init template
2. After the VM boots, the spuff-agent reads the config from `/opt/spuff/devtools.json`
3. Node.js is installed first (prerequisite for all AI tools)
4. Each enabled AI tool is installed via `npm install -g <package>`
5. Installation happens asynchronously — SSH is available before tools finish installing
6. Use `spuff ai status` to track progress

## Disabling AI Tools

If you don't need AI tools and want faster provisioning:

```yaml
# spuff.yaml
ai_tools: none
```

Or via CLI:

```bash
spuff up --ai-tools none
```
