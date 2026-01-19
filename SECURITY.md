# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report them via email to: **security@avelino.run**

Include as much of the following information as possible:

- Type of vulnerability (e.g., command injection, authentication bypass, etc.)
- Full paths of source file(s) related to the vulnerability
- Location of the affected source code (tag/branch/commit or direct URL)
- Step-by-step instructions to reproduce the issue
- Proof-of-concept or exploit code (if possible)
- Impact of the issue, including how an attacker might exploit it

You should receive a response within 48 hours. If the issue is confirmed, we will:

1. Acknowledge the report within 48 hours
2. Provide an estimated timeline for a fix
3. Notify you when the issue is fixed
4. Credit you in the security advisory (unless you prefer to remain anonymous)

## Security Model

Spuff handles sensitive data including:

- Cloud provider API tokens
- SSH private keys (references only)
- Agent authentication tokens

### Design Principles

1. **Minimal exposure**: Agent API binds to localhost only
2. **Token-based auth**: All sensitive endpoints require authentication
3. **No key storage**: SSH keys are referenced, not copied to VMs
4. **Ephemeral by design**: VMs are temporary, reducing attack surface

### Known Security Considerations

| Area | Risk | Mitigation |
|------|------|------------|
| Cloud API tokens | Token exposure | Stored in env vars, not committed |
| SSH agent forwarding | Key exposure if VM compromised | User's choice to enable |
| Agent API | Unauthenticated access | Token auth, localhost binding |
| Cloud-init | Secrets in user-data | Base64 encoded, VM-only access |

See [docs/security.md](docs/security.md) for the complete security model.

## Best Practices for Users

1. **Never commit tokens** to version control
2. **Use environment variables** for sensitive configuration
3. **Rotate API tokens** periodically
4. **Use SSH key passphrases** and ssh-agent
5. **Set idle timeouts** to minimize exposure window
6. **Review cloud-init logs** for any anomalies

## Security Updates

Security updates are released as patch versions and announced via:

- GitHub Security Advisories
- Release notes
- README badge (if critical)

We recommend always running the latest version.
