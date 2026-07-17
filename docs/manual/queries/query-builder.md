# Query Builder

The query builder provides a fluent interface for constructing and executing SELECT queries. Start from any model class with `Model.query()`.

## Chain order

```
query()
  → select() / filter() / raw_filter()
  → with_relation() / prefetch_related()
  → limit()
  → fetch() / first() / fetch_lazy() / fetch_arrow()
```

## Methods

### `select(*fields)`

Restrict the SQL projection to specified columns. Accessing unselected fields raises `ProjectionError`.

```python
user = await User.query().select("username", "email").first()
# SQL: SELECT username, email FROM users LIMIT 1
```

### `filter(**kwargs)`

Add equality filters. Multiple kwargs are combined with AND.

```python
users = await User.query().filter(role="admin", active=True).fetch()
# SQL: SELECT * FROM users WHERE role = ? AND active = ?
```

Only exact equality. For raw SQL expressions, use `raw_filter()`.

### `raw_filter(column, raw_expr)`

Add a filter with a raw SQL expression (requires `allow-raw-sql` feature).

```python
from bridge.core.query import Raw

users = await User.query().raw_filter("age", Raw("> ?", 18)).fetch()
# SQL: SELECT * FROM users WHERE age > ?
```

### `limit(count)`

Limit the number of results. Must be a positive integer.

```python
users = await User.query().limit(10).fetch()
# SQL: SELECT * FROM users LIMIT 10
```

### `with_relation(relation_name, strategy)`

Register a relation for eager loading with a specific strategy.

```python
from bridge.core.query import EagerLoadingStrategy

users = await User.query().with_relation(
    "posts", EagerLoadingStrategy.SELECT_IN_FOR_TO_MANY
).fetch()
```

### `prefetch_related(*relation_names)`

Django-style shorthand for `with_relation()` using `SELECT_IN_FOR_TO_MANY` for all named relations. Safe for both to-one and to-many relations — no Cartesian explosion risk.

```python
users = await User.query().prefetch_related("posts", "groups").fetch()
```

### `fetch(tx=None)`

Execute the query. Returns a list of model instances.

```python
users = await User.query().filter(active=True).fetch(tx=session)
```

If a session is provided, results are registered in the identity map.

### `first(tx=None)`

Execute the query with `LIMIT 1`. Returns a single model instance or `None`.

```python
user = await User.query().filter(email="alice@example.com").first()
```

### `fetch_lazy(tx=None)`

Execute the query and return an async iterator. Useful for large result sets — rows are streamed one at a time without full materialization.

```python
async for user in User.query().filter(active=True).fetch_lazy():
    process(user)
```

Identity map population still occurs per-row if a session is provided.

### `fetch_arrow(tx=None)`

Execute the query using Apache Arrow IPC for zero-copy data transfer. Returns `LazyModelProxy` instances that materialize individual columns on attribute access. Requires the `data-science` Cargo feature.

```python
proxies = await User.query().filter(active=True).fetch_arrow()
for proxy in proxies:
    print(proxy.username)  # reads from Arrow columnar data
```

## Full chain example

```python
users = await User.query()\
    .select("id", "username", "email")\
    .filter(active=True)\
    .prefetch_related("posts")\
    .limit(20)\
    .fetch(tx=session)
# SQL: SELECT id, username, email FROM users WHERE active = ? LIMIT 20
# Then: batch SELECT IN for posts relation
```

## How it works

Each builder method returns `self` for chaining. `fetch()` merges regular filters and raw filters, serializes eager load requests, and calls `bridge_rs.fetch_all()` across FFI. The Rust side constructs the SQL via the appropriate `Dialect` implementation, executes it via `sqlx`, and returns rows as Python dicts.

For eager loading, after the parent query completes, `fetch()` iterates each `EagerLoadRequest`, calls the appropriate `bridge_rs.batch_fetch_*` function for the relation type, and attaches results to parent instances.
