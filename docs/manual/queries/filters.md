# Filters

## Equality filters

The `filter(**kwargs)` method on `QueryBuilder` accepts column=value pairs combined with AND:

```python
await User.query().filter(role="admin", active=True).fetch()
# WHERE role = ? AND active = ?
```

Supported value types: `str`, `int`, `float`, `bool`, `uuid.UUID`, `datetime`, `None` (becomes SQL NULL).

## Raw SQL filters

For non-equality operators (comparisons, LIKE, IN, etc.), use `raw_filter()` with a `Raw` expression. Requires the `allow-raw-sql` feature.

```python
from bridge.core.query import Raw

# Greater than
await User.query().raw_filter("age", Raw("> ?", 18)).fetch()
# WHERE age > ?

# IN clause
await User.query().raw_filter("role", Raw("IN (?, ?)", "admin", "moderator")).fetch()

# LIKE
await User.query().raw_filter("username", Raw("LIKE ?", "%alice%")).fetch()
```

## Type coercion

Values passed to `filter()` are coerced through the Rust type system before reaching the database:

| Python input | Coerced to | SQL parameter |
|---|---|---|
| `str` | `QueryValue::String` | `TEXT` / `VARCHAR` |
| `int` | `QueryValue::Int` | `INTEGER` |
| `float` | `QueryValue::Float` | `REAL` / `FLOAT` |
| `bool` | `QueryValue::Bool` | `BOOLEAN` |
| `uuid.UUID` | `QueryValue::Uuid` | `UUID` |
| `datetime` | `QueryValue::DateTime` | `TIMESTAMP` |
| `dict` / `list` | `QueryValue::Json` | `JSON` |
| `None` | `QueryValue::Null` | `NULL` |

Invalid types raise `BridgeError::TypeMismatch` at the FFI boundary.

## Security

String interpolation in SQL is forbidden. Every value passes through `sqlx` parameter binding. SQL identifiers (table names, column names) are validated against `VALID_SQL_IDENTIFIER_PATTERN` before reaching any query.
