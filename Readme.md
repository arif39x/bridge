<div align="center">

# Bridge

[![Performance: Native FFI](https://img.shields.io/badge/Performance-Native_FFI-red.svg)](#architecture)
[![Async: Tokio/Asyncio](https://img.shields.io/badge/Async-Tokio%2FAsyncio-blue.svg)](#architecture)
[![Security: SQL_Injection_Proof](https://img.shields.io/badge/Security-Injection_Proof-success.svg)](#security-mandate)
[![Reliability: Circuit_Breaker](https://img.shields.io/badge/Reliability-Circuit_Breaker-gold.svg)](#security-mandate)
[![Observability: OpenTelemetry](https://img.shields.io/badge/Observability-OpenTelemetry-blueviolet.svg)](#architecture)

**Bridge** is a cross-language ORM (Rust+Python). It is lightweight, secure by default.

</div>

---

## Architecture

**Bridge** uses the **Performance Bridge** principle by splitting the ORM into two distinct parts to maximize both **Speed** and **Developer Ergonomics**:

1.  **Expression Layer (Python)**: A thin, expressive API that allows developers to write intuitive queries and models. It handles high-level logic and task-local identity mapping.
2.  **Execution Engine (Rust)**: An ultra-fast core built on `sqlx` and `tokio`. It handles connection pooling, SQL construction, row hydration, and cross-language telemetry.

Instead of slow HTTP or JSON-over-pipe communication, Bridge utilizes **Native Memory Bindings (FFI)**, allowing data to flow between Python and Rust with near-zero latency.

---

## Supported SQL Databases

Bridge provides native and protocol-compatible support for a wide range of modern and enterprise databases:

| Database          | Compatibility     | Specific Optimizations                                  |
| :---------------- | :---------------- | :------------------------------------------------------ |
| **PostgreSQL**    | Native            | Full `async` support via `sqlx`.                        |
| **SQLite**        | Native            | High-performance local and embedded storage.            |
| **MySQL**         | Native            | Standard industry support with backtick quoting.        |
| **MariaDB**       | Native            | Specialized MariaDB dialect optimizations.              |
| **Oracle**        | Custom            | **Custom `:1` placeholders** & `FETCH NEXT` pagination. |
| **MS SQL Server** | Native            | Support for `[]` quoting and `@p1` placeholders.        |
| **CockroachDB**   | Postgres-Protocol | Optimized for distributed `UUID`s and `SERIAL8`.        |
| **PlanetScale**   | MySQL-Protocol    | Optimized for Vitess-based serverless pooling.          |
| **Neon**          | Postgres-Protocol | Native support for serverless Postgres architecture.    |
| **YugabyteDB**    | Postgres-Protocol | Built for distributed SQL workloads.                    |
| **Cloudflare D1** | SQLite-Protocol   | Optimized for serverless SQLite environments.           |
| **Dolt**          | MySQL-Protocol    | Native support for versioned SQL databases.             |

---

## Security Mandate

When I started designing Bridge, my absolute priority was **Security**. I’ve tried my best to build a secure wall around your data..
Here is how Bridge protection works around your data:

1.  **Forbidden String Interpolation**: I have strictly forbidden string interpolation in queries. If it's not parameterized, it doesn't run. Period.
2.  **Rust-Level Guardrails**: Before any dynamic SQL identifier even reaches the database, the **Rust Engine** forces it through a strict regular expression validator. No sneak attacks.
3.  **Panic-Proof FFI**: The language boundary is wrapped in `catch_unwind` blocks. If something goes wrong in the Rust core, your Python app won't crash; it gets a clean exception.
4.  **Strict Type Coercion**: We don't guess types. Every piece of data crossing the bridge is validated against your model's metadata. No silent data corruption.
5.  **The Circuit Breaker**: If your database starts failing or slowing down, our internal circuit breaker trips. This protects your application from cascading failures and keeps your threads alive.

---

## Rules

Collaborator Must Follow This Rules:

1.  **Self-Documenting Code**: Meaningful identifiers Must. If something doesn't make sense to you; then rename it to something appropriate so that its logic will inspire clarity.
2.  **Single Responsibility Principle**: Each method must have only one responsibility and be of equal simplicity.
3.  **D.R.Y. Principle**: Do not duplicate common functionality; instead; utilise a single reference point when using common functionality.
4.  **Meaningful Identifier**: Write your identifiers as if they were spoken words. Use common sense when naming them; avoid unnecessary jargon names; and choose names with the focus of clarity.
5.  **Avoid Magic Number/Strings**: Use variable constants for hard-coded numbers/strings, so their meaning is clear.
6.  **Explicit Handling of Errors**: Fix the actual problem first (i.e., fix the code), then use either typed return value or exception handling to guarantee that the error is visible and cannot go unaddressed.
7.  **Consistent Formatting**: Use automated tools for visual consistency across the entire codebase (if possible).
8.  **Provide Explanation to your Intent**: Comment on your code to explain "why" you made those coding decisions versus only providing commentary on "what" the code is doing.
9.  **FFI Boundary Safety**: Any new feature crossing the Python/Rust boundary MUST be wrapped in the `ffi_guard!` macro to ensure the application remains crash-safe.
10. **Dialect Agnosticism**: Never write SQL that is specific to one database in the core engine. Always use the `Dialect` trait to ensure your change works across Postgres, MySQL, Oracle, and others.
11. **Telemetry Integrity**: Every new database operation must include tracing spans. If we can't measure it, we shouldn't merge it.

---

## Current Limitations

- **Eager Loading**: Relations support both high-speed Lazy Loading (per-relation) and batch `prefetch_related` via `QueryBuilder.with_relation()` / `.prefetch_related()`.
- **SQL Complexity**: Advanced operations like CTEs and Window Functions require the `execute_raw()` fallback.
- **Identity Isolation**: The Identity Map is strictly scoped per `asyncio.Task` to ensure memory safety.
