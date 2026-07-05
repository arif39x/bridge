<div align="center">

<img src="assets/bridge.png" alt="Bridge Logo" width="250" style="border-radius: 50%;"/>

# Bridge

[![Performance: Native FFI](https://img.shields.io/badge/Performance-Native_FFI-red.svg)](#architecture)
[![Async: Tokio/Asyncio](https://img.shields.io/badge/Async-Tokio%2FAsyncio-blue.svg)](#architecture)
[![Security: SQL_Injection_Proof](https://img.shields.io/badge/Security-Injection_Proof-success.svg)](#security-mandate)
[![Reliability: Circuit_Breaker](https://img.shields.io/badge/Reliability-Circuit_Breaker-gold.svg)](#security-mandate)
[![Observability: OpenTelemetry](https://img.shields.io/badge/Observability-OpenTelemetry-blueviolet.svg)](#architecture)
[![Python 3.10+](https://img.shields.io/badge/Python-3.10%2B-blue?logo=python)](#)
[![Rust 1.70+](https://img.shields.io/badge/Rust-1.70%2B-orange?logo=rust)](#)
[![sqlx 0.7](https://img.shields.io/badge/sqlx-0.7-red)](#)

**Bridge** is a cross-language ORM (Rust+Python). It is lightweight, secure by default.

</div>

---

## Architecture

Bridge uses the **Performance Bridge** principle by splitting the ORM into two distinct parts to maximize both **Speed** and **Developer Ergonomics**:

1. **Expression Layer (Python)**: A thin, expressive API for intuitive queries and models. Handles high-level logic, task-local identity mapping, dirty tracking, hooks, and eager loading orchestration.
2. **Execution Engine (Rust)**: An ultra-fast core built on `sqlx` and `tokio`. Handles connection pooling, SQL construction, row hydration, circuit breaking, and cross-language telemetry.

Instead of slow HTTP or JSON-over-pipe communication, Bridge utilizes **Native Memory Bindings (FFI via PyO3)**, allowing data to flow between Python and Rust with near-zero latency.

---

## Quick Start

```python
from bridge import BaseModel, HasMany, BelongsToMany, transaction, connect

class User(BaseModel):
    table = "users"
    _fields = ["id", "username", "email"]

    id: str
    username: str
    email: str

async def main():
    pool = await connect("postgres://localhost/mydb")
    async with transaction() as tx:
        user = await User.create(tx, username="alice", email="alice@example.com")
        found = await User.find_one(tx, username="alice")
        print(found.to_dict())
```

---

## Key Features

| Feature | Description |
|---------|-------------|
| **Declarative Models** | Define models with Python type hints; auto-registration with Rust metadata |
| **Fluent Query Builder** | `.filter()`, `.limit()`, `.select()`, `.prefetch_related()`, `.first()` |
| **CRUD Operations** | Single and bulk insert, update, delete with automatic type coercion |
| **Unit of Work / Session** | Identity map, dirty tracking with snapshot-based diff, automatic flush on commit |
| **Eager Loading** | Batch SELECT IN for `HasMany`, `BelongsToMany`, and `SelfReferential` relations |
| **Lazy Loading** | Per-relation lazy proxies that resolve on first access |
| **Lazy Streaming** | `fetch_lazy()` returns an async iterator for large result sets |
| **Arrow IPC** | `fetch_arrow()` uses Apache Arrow for zero-copy data transfer (feature `data-science`) |
| **Schema Migration Engine** | Snapshot-based diffing generates UP/DOWN SQL for evolving schemas |
| **CLI Tools** | `bridge reflect`, `bridge makemigrations`, `bridge migrate` |
| **Schema Introspection** | Reflect existing database tables into Python model classes |
| **Admin Panel** | Auto-generated FastAPI admin with JWT auth, CSRF, rate limiting (optional) |
| **REST API Generator** | Auto-generate FastAPI routers from model classes |
| **Lifecycle Hooks** | `before_create`, `after_create`, `before_delete`, `after_delete` decorators |
| **Optimistic Concurrency** | Version-guarded updates via `_bridge_row_version` column |
| **OpenTelemetry** | Distributed tracing with OTLP export, slow query logging |
| **Pytest Integration** | `db_session` fixture with transactional rollback |
| **factory_boy** | `BridgeFactory` base class for async test data creation |

---

## Supported SQL Databases

Bridge provides native and protocol-compatible support for a wide range of modern and enterprise databases:

| Database          | Compatibility     | Specific Optimizations                                  |
| :---------------- | :---------------- | :------------------------------------------------------ |
| **PostgreSQL**    | Native            | Full `async` support via `sqlx`.                        |
| **SQLite**        | Native            | High-performance local and embedded storage.            |
| **MySQL**         | Native            | Standard industry support with backtick quoting.        |
| **MariaDB**       | Native            | Specialized MariaDB dialect optimizations.              |
| **Oracle**        | Custom            | Custom `:1` placeholders & `FETCH NEXT` pagination. |
| **MS SQL Server** | Native            | Support for `[]` quoting and `@p1` placeholders.        |
| **CockroachDB**   | Postgres-Protocol | Optimized for distributed UUIDs and SERIAL8.        |
| **PlanetScale**   | MySQL-Protocol    | Optimized for Vitess-based serverless pooling.          |
| **Neon**          | Postgres-Protocol | Native support for serverless Postgres architecture.    |
| **YugabyteDB**    | Postgres-Protocol | Built for distributed SQL workloads.                    |
| **Cloudflare D1** | SQLite-Protocol   | Optimized for serverless SQLite environments.           |
| **Dolt**          | MySQL-Protocol    | Native support for versioned SQL databases.             |

---

## Security Mandate

When I started designing Bridge, my absolute priority was **Security**. I've tried my best to build a secure wall around your data.

Here is how Bridge protection works around your data:

1. **Forbidden String Interpolation**: String interpolation in queries is strictly forbidden. If it's not parameterized, it doesn't run. Period.
2. **Rust-Level Guardrails**: Before any dynamic SQL identifier reaches the database, the Rust Engine forces it through a strict regular expression validator. No sneak attacks.
3. **Panic-Proof FFI**: The language boundary is wrapped in `catch_unwind` blocks. If something goes wrong in the Rust core, your Python app won't crash; it gets a clean exception.
4. **Strict Type Coercion**: We don't guess types. Every piece of data crossing the bridge is validated against your model's metadata. No silent data corruption.
5. **The Circuit Breaker**: If your database starts failing or slowing down, our internal circuit breaker trips. This protects your application from cascading failures and keeps your threads alive.

---

## Feature Flags

Bridge uses Cargo feature flags to keep the core lightweight:

| Feature | Description |
|---------|-------------|
| `default` | No default features — build only what you need |
| `allow-raw-sql` | Enables `execute_raw()` and `Raw` SQL expressions |
| `data-science` | Enables Arrow IPC via `fetch_arrow()` |
| `java-interop` | Enables JNI bindings for Java interop |

---

## CLI Commands

```
bridge reflect --url <DATABASE_URL> --table <TABLE_NAME>   # Introspect table → Python model
bridge makemigrations --dialect <sqlite|postgres>           # Generate migration SQL from models
bridge migrate --url <DATABASE_URL>                         # Apply pending migrations
```

---

## Rules

Collaborator Must Follow This Rules:

1. **Self-Documenting Code**: Meaningful identifiers are a must. If something doesn't make sense to you, rename it so its logic inspires clarity.
2. **Single Responsibility Principle**: Each method must have only one responsibility and be of equal simplicity.
3. **D.R.Y. Principle**: Do not duplicate common functionality; instead, use a single reference point when using common functionality.
4. **Meaningful Identifier**: Write your identifiers as if they were spoken words. Use common sense when naming them; avoid unnecessary jargon; choose names with clarity as the focus.
5. **Avoid Magic Numbers/Strings**: Use named constants for hard-coded values so their meaning is clear.
6. **Explicit Handling of Errors**: Fix the actual problem first (fix the code), then use typed return values or exception handling to guarantee errors are visible and cannot go unaddressed.
7. **Consistent Formatting**: Use automated tools for visual consistency across the entire codebase.
8. **Provide Explanation to your Intent**: Comment on your code to explain *why* you made those coding decisions, not just *what* the code is doing.
9. **FFI Boundary Safety**: Any new feature crossing the Python/Rust boundary MUST be wrapped in the `ffi_guard!` macro to ensure the application remains crash-safe.
10. **Dialect Agnosticism**: Never write SQL specific to one database in the core engine. Always use the `Dialect` trait to ensure changes work across all supported databases.
11. **Telemetry Integrity**: Every new database operation must include tracing spans. If we can't measure it, we shouldn't merge it.

---

## Current Limitations

- **Eager Loading**: Relations support both high-speed Lazy Loading (per-relation) and batch `prefetch_related` via `QueryBuilder.with_relation()` / `.prefetch_related()`.
- **SQL Complexity**: Advanced operations like CTEs and Window Functions require the `execute_raw()` fallback (requires `allow-raw-sql` feature).
- **Identity Isolation**: The Identity Map is strictly scoped per `asyncio.Task` to ensure memory safety.
- **ALTER COLUMN Nullability**: Changing column nullability via migration requires dialect-specific SQL syntax (SET NOT NULL / DROP NOT NULL).
- **Composite Foreign Keys**: Relations assume single-column foreign keys; composite FKs are not yet supported.
