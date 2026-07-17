# FAQ

## How does Bridge compare to SQLAlchemy?

Bridge is a different design: Python expression layer over a Rust execution engine. This gives it a different performance profile and security model compared to pure-Python ORMs. The trade-off is a smaller ecosystem and fewer third-party extensions.

## Does Bridge support synchronous usage?

No. Bridge is async-only. All database operations require an async context (asyncio event loop). This is by design — the Rust engine runs on tokio, and blocking calls would defeat the purpose.

## Can I use Bridge with an existing database?

Yes. Use `bridge reflect --url <URL> --table <TABLE_NAME>` to introspect a table and generate a model class definition. You can then use that model directly.

## Does Bridge support connection pooling?

Yes. `connect()` initializes a connection pool managed by the Rust engine (`PoolManager`). Sessions borrow connections from the pool. Pool configuration is handled internally — you get a pool per database URL.

## How does Bridge handle transactions?

Sessions always run within a transaction. `transaction()` creates a session, begins a transaction, and commits or rolls back on context exit. You can also create sessions explicitly with `begin_session()`.

## What happens to unflushed changes if the application crashes?

Bridge does not auto-flush on every mutation. Unflushed changes are held in the identity map's dirty tracker and are lost on crash. Only committed data is durable. For partial safety, call `session.flush()` at checkpoints.

## Can Bridge handle composite foreign keys?

Not yet. Relations assume single-column foreign keys. Composite foreign keys are tracked as a known limitation.

## Does Bridge support async streaming?

Yes. `fetch_lazy()` returns an `AsyncIterator` that yields model instances one at a time without loading the full result set into memory. This is suitable for large datasets.

## How does the identity map work across tasks?

The identity map is scoped per `asyncio.Task`. Different tasks within the same session have separate identity maps. This prevents memory leaks from long-lived tasks.

## What databases are supported?

PostgreSQL, SQLite, MySQL, MariaDB, Oracle, MS SQL Server, CockroachDB, PlanetScale, Neon, YugabyteDB, Cloudflare D1, and Dolt.

## How do I contribute?

See [contributing.md](contributing.md).
