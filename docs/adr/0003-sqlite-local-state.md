# ADR-0003: SQLite for Local State Management

## Status

Superseded — Migrated to ChronDB (Git-backed document store) in `~/.spuff/chrondb/`

## Date

2025-01

## Context

Spuff needs to track information about active instances locally:

- Instance ID, name, IP address
- Cloud provider and region
- Creation timestamp
- Current status

This state is needed for:

- `spuff status` - Show current instance info
- `spuff ssh` - Connect to instance by name
- `spuff down` - Know what to destroy
- Orphan detection - Find forgotten instances

### Requirements

- Persistent across CLI invocations
- Fast reads and writes
- No external dependencies (database servers)
- Works offline
- Easy to backup/migrate
- Queryable (list, filter, search)

## Decision

We will use **SQLite** for local state management, stored at `~/.spuff/state.db`.

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

### Implementation

Using `rusqlite` crate with bundled SQLite:

```rust
pub struct StateDb {
    conn: Connection,
}

impl StateDb {
    pub fn open() -> Result<Self> {
        let path = config_dir()?.join("state.db");
        let conn = Connection::open(&path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn save_instance(&self, instance: &Instance) -> Result<()>;
    pub fn get_active_instance(&self) -> Result<Option<Instance>>;
    pub fn delete_instance(&self, id: &str) -> Result<()>;
    pub fn list_instances(&self) -> Result<Vec<Instance>>;
}
```

### Why SQLite?

1. **Zero configuration**: No server to install or manage
2. **Single file**: Easy to backup, move, or delete
3. **ACID compliant**: Reliable even on crashes
4. **Fast**: Perfect for local, single-user access
5. **Familiar**: SQL is well-understood
6. **Bundled**: `rusqlite` bundles SQLite, no system dependency

## Consequences

### Positive

- **Simple**: No external dependencies or services
- **Reliable**: SQLite is battle-tested
- **Queryable**: SQL allows flexible queries
- **Portable**: Single file, works on all platforms
- **Debuggable**: Can inspect with `sqlite3` CLI

### Negative

- **Binary file**: Not human-readable (vs JSON/YAML)
- **Schema migrations**: Need to handle schema changes
- **Concurrency**: SQLite has write locks (not an issue for CLI)
- **Additional dependency**: `rusqlite` adds to binary size

### Neutral

- Learning curve for SQL if unfamiliar
- Need to decide on migration strategy for future schema changes

## Alternatives Considered

### Alternative 1: JSON File

Store state in a JSON file at `~/.spuff/state.json`.

**Pros:**

- Human-readable
- No additional dependencies
- Simple to implement

**Cons:**

- No atomic updates (corruption risk on crash)
- Must load entire file for any operation
- No query capabilities
- Manual locking needed

**Why rejected:** Risk of corruption and lack of query capability.

### Alternative 2: YAML File

Similar to JSON but with YAML format.

**Pros:**

- Human-readable and editable
- Familiar format

**Cons:**

- Same issues as JSON
- YAML parsing is slower

**Why rejected:** Same reasons as JSON.

### Alternative 3: sled (Embedded KV Store)

Use sled, an embedded key-value database in Rust.

**Pros:**

- Pure Rust, no C dependencies
- Good performance

**Cons:**

- Less mature than SQLite
- No SQL queries
- Larger binary size

**Why rejected:** SQLite is more mature and SQL provides flexibility.

### Alternative 4: Cloud Storage (Provider State)

Rely on cloud provider tags to track instances.

**Pros:**

- No local state needed
- Accessible from anywhere

**Cons:**

- Requires API calls for every operation
- Doesn't work offline
- Provider-specific implementation

**Why rejected:** Adds latency and requires network access.

## References

- [SQLite Documentation](https://www.sqlite.org/docs.html)
- [rusqlite Crate](https://github.com/rusqlite/rusqlite)
- [SQLite for Application File Format](https://www.sqlite.org/appfileformat.html)
