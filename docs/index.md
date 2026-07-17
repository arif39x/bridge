# Bridge

Bridge is a cross-language ORM with a Python expression layer and a Rust execution engine. It combines the ergonomics of Python with the performance of native Rust code via PyO3 FFI.

## Architecture

Bridge splits the ORM into two layers:

- **Expression Layer (Python)**: Thin API for model definition, query building, session management, hooks, and eager loading orchestration.
- **Execution Engine (Rust)**: Built on `sqlx` and `tokio`. Handles connection pooling, SQL construction, row hydration, circuit breaking, and telemetry.

Data crosses the language boundary through native memory bindings (PyO3), not HTTP or JSON pipes.

## Why Bridge

- **Security first**: No string interpolation in queries. Identifiers validated by regex. FFI wrapped in `catch_unwind`. Circuit breaker prevents cascading failures.
- **Async throughout**: Python `asyncio` driving Rust `tokio` — no blocking calls.
- **12 database dialects**: PostgreSQL, SQLite, MySQL, MariaDB, Oracle, MS SQL Server, CockroachDB, PlanetScale, Neon, YugabyteDB, Cloudflare D1, Dolt.
- **Identity Map + Dirty Tracking**: Unit of Work pattern with snapshot-based diff and automatic flush on commit.
- **Eager and lazy loading**: Batch SELECT IN for to-many relations, JOIN-based for to-one, lazy proxies for deferred resolution.
- **Schema migration engine**: Snapshot-based diffing generates UP/DOWN SQL.
- **Optional extras**: Admin panel (FastAPI), REST API generator, Arrow IPC, OpenTelemetry, pytest fixtures, factory_boy support.

## Feature flags

Bridge uses Cargo feature flags to keep the core lightweight:

| Feature | Description |
|---------|-------------|
| `default` | No default features |
| `allow-raw-sql` | Enables `execute_raw()` and `Raw` SQL expressions |
| `data-science` | Enables Arrow IPC via `fetch_arrow()` |
| `java-interop` | Enables JNI bindings for Java interop |

## Quick links

- [Quickstart](getting-started/quickstart.md) — install and first query in 5 minutes
- [Defining models](getting-started/defining-models.md)
- [Basic queries](getting-started/basic-queries.md)
- [Relationships](getting-started/relationships.md)
- [Query builder reference](manual/queries/query-builder.md)
- [Transactions and sessions](manual/sessions/transactions.md)
- [Migrations](manual/migrations/creating.md)
- [Python API reference](api/python/bridge.md)
