# Architecture Decision Records

This directory contains Architecture Decision Records (ADRs) for the spuff project.

## What is an ADR?

An ADR is a document that captures an important architectural decision made along with its context and consequences. ADRs help:

- **Document** the reasoning behind decisions
- **Communicate** decisions to the team
- **Onboard** new contributors by explaining "why"
- **Revisit** decisions when context changes

## ADR Index

| ID | Title | Status | Date |
|----|-------|--------|------|
| [0001](0001-cloud-init-bootstrap.md) | Use cloud-init for VM bootstrap | Accepted | 2025-01 |
| [0002](0002-two-phase-bootstrap.md) | Two-phase bootstrap (sync + async) | Accepted | 2025-01 |
| [0003](0003-sqlite-local-state.md) | SQLite for local state management | Accepted | 2025-01 |
| [0004](0004-ssh-agent-forwarding.md) | SSH agent forwarding for git access | Accepted | 2025-01 |
| [0005](0005-provider-trait-abstraction.md) | Provider trait for cloud abstraction | Accepted | 2025-01 |
| [0006](0006-project-config-spec.md) | Project configuration (spuff.yaml) | Accepted | 2025-01 |

## Status Values

- **Proposed** - Under discussion
- **Accepted** - Decision made, implementing
- **Deprecated** - Superseded by another ADR
- **Superseded** - Replaced by a newer ADR

## Creating a New ADR

1. Copy the template:

   ```bash
   cp docs/adr/template.md docs/adr/NNNN-title.md
   ```

2. Fill in the template with:
   - Context: What is the situation?
   - Decision: What did we decide?
   - Consequences: What are the results?

3. Submit a PR for review

4. Update this README with the new ADR

## Template

See [template.md](template.md) for the ADR template.

## References

- [ADR GitHub Organization](https://adr.github.io/)
- [Michael Nygard's Article](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
