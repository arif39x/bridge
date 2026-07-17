# Raw SQL

Bridge's query builder handles the common cases. For operations outside its scope — CTEs, window functions, complex joins, DDL — use `execute_raw()`.

## Availability

`execute_raw()` is gated behind the `allow-raw-sql` Cargo feature. Without it, calling the function raises `RuntimeError`.

## Usage

```python
from bridge import execute_raw

await execute_raw("CREATE TABLE IF NOT EXISTS audit_log (id INTEGER PRIMARY KEY, event TEXT)")

# Raw select — results are returned as list of dicts
rows = await execute_raw("SELECT * FROM users WHERE age > 18 AND city = 'NYC'")
# => [{"id": "...", "username": "alice", ...}, ...]
```

## The `Raw` expression helper

For raw expressions within the query builder, use `Raw` and `raw_filter()`:

```python
from bridge.core.query import Raw

# In filter context
users = await User.query()\
    .raw_filter("created_at", Raw(">= NOW() - INTERVAL '7 days'"))\
    .fetch()
```

## Raw in filter context

`Raw` is rejected by the standard `filter()` method. You must use `raw_filter()` to make the raw SQL opt-in explicit:

```python
# This raises TypeError:
users = await User.query().filter(age=Raw("> ?", 18)).fetch()
```

## When to use raw SQL

- CTEs (`WITH ... AS`)
- Window functions (`ROW_NUMBER() OVER ...`)
- Full-text search
- Database-specific DDL
- Bulk operations with dialect-specific syntax

## When not to use raw SQL

- Routine CRUD — use the query builder and model methods
- Anything that could be expressed through `filter()` and `raw_filter()`
- Operations where dialect portability matters

## Security

`execute_raw()` passes your SQL string directly to the database driver. It bypasses all Bridge security layers: no identifier validation, no type coercion, no parameter binding guardrails. If your application uses `execute_raw()`, you are responsible for preventing SQL injection. Never interpolate user input into raw SQL strings.
