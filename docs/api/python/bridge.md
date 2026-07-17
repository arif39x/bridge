# Python API Reference

## Package root (`bridge`)

### `connect(url: str)`

Initialize the database connection pool. Must be called before any database operations.

```python
await connect("sqlite::memory:")
await connect("postgres://user:pass@localhost/mydb")
```

### `transaction()`

Async context manager that creates a session, yields it, commits on success, and rolls back on exception.

```python
async with transaction() as tx:
    user = await User.create(tx, username="alice")
```

### `execute_raw(sql: str)`

Execute raw SQL. Requires `allow-raw-sql` Cargo feature. Raises `RuntimeError` if the feature is not enabled.

```python
rows = await execute_raw("SELECT * FROM users")
```

### `configure_logging(level: str = "info", slow_query_ms: int = 100)`

Configure structured query logging. Installs telemetry bridge if not already installed. Logs queries slower than `slow_query_ms` as warnings.

### `BaseModel`

Base class for all model definitions. See [Defining models](../../getting-started/defining-models.md).

| Method | Description |
|--------|-------------|
| `query()` | Create a `QueryBuilder` for this model |
| `create(**kwargs)` | Insert a new record |
| `create_many(items)` | Bulk insert records |
| `find_one(**filters)` | Find by primary key or filter |
| `delete()` | Delete this instance |
| `delete_many(**filters)` | Delete matching records |
| `to_dict()` | Serialize to dict |
| `to_json()` | Serialize to JSON string |
| `to_xml()` | Serialize to XML string |
| `get_field_definitions()` | Return field name → type mapping |

### Error hierarchy

| Exception | Base class | When raised |
|-----------|------------|-------------|
| `BridgeError` | `Exception` | Base for all Bridge errors |
| `ConnectionError` | `BridgeError` | Database connection failures |
| `QueryError` | `BridgeError` | Query execution failures |
| `NotFoundError` | `BridgeError, KeyError` | Resource not found |
| `ConstraintError` | `BridgeError` | Constraint violation |
| `ValidationError` | `BridgeError, ValueError` | Data validation failure |
| `DatabaseError` | `BridgeError` | Database engine error |
| `ProjectionError` | `BridgeError, AttributeError` | Accessing unselected field |
| `CompositeKeyError` | `BridgeError` | Composite key operation failure |
| `HookAbortedError` | `BridgeError` | Hook cancelled operation |
| `SessionExpiredError` | `BridgeError` | Session exceeded lifetime |

### `HasMany(target_model, foreign_key)`

One-to-many relationship descriptor. Requires `target_model` (class or string name) and `foreign_key` column name.

### `BelongsToMany(target_model, junction, left_key, right_key)`

Many-to-many relationship descriptor. Requires `junction` table name and the two foreign key column names.

### `SelfReferential(target_model, parent_key)`

Self-referential relationship descriptor. Requires `parent_key` column name.

### `Session`

Unit of Work session with identity map and dirty tracking.

| Method | Description |
|--------|-------------|
| `commit()` | Flush dirty entities and commit transaction |
| `rollback()` | Rollback transaction |
| `flush()` | Compute diffs and execute UPDATEs |
| `get_entity(table, pk_values)` | Look up from identity map |
| `set_entity(model_class, pk_values, entity)` | Store in identity map |
| `clear()` | Clear identity map |
| `get_stats()` | Returns cache metrics |

### `begin_session(cache_size=1000, max_lifetime=3600)`

Create a new Session. Connects to the database pool initialized by `connect()`.

### `Registry`

Model registry mapping table names to model classes. Methods: `register()`, `unregister()`, `get()`, `models()`, `clear()`. Supports dict-like access (`__getitem__`, `__setitem__`, `__contains__`, `__iter__`).

### `create_base(name)`

Create a new BaseModel subclass with its own registry. Useful for plugin systems or multi-tenant setups.

### `registry_scope(base, registry)`

Context manager that temporarily swaps a base class's registry. All new model declarations within the context use the provided registry.

## `bridge.core.query`

### `QueryBuilder`

Fluent query builder. See [Query builder reference](../../manual/queries/query-builder.md).

| Method | Returns |
|--------|---------|
| `select(*fields)` | `QueryBuilder` |
| `filter(**kwargs)` | `QueryBuilder` |
| `raw_filter(column, raw_expr)` | `QueryBuilder` |
| `limit(count)` | `QueryBuilder` |
| `with_relation(name, strategy)` | `QueryBuilder` |
| `prefetch_related(*names)` | `QueryBuilder` |
| `fetch(tx)` | `List[BaseModel]` |
| `first(tx)` | `Optional[BaseModel]` |
| `fetch_lazy(tx)` | `AsyncIterator[BaseModel]` |
| `fetch_arrow(tx)` | `List[LazyModelProxy]` |

### `EagerLoadingStrategy`

Enum: `JOINED_FOR_TO_ONE`, `SELECT_IN_FOR_TO_MANY`.

### `Raw(sql, *params)`

Wrapper for raw SQL expressions with bound parameters.

## `bridge.schema`

### `MigrationEngine`

See [Migration creation](../../manual/migrations/creating.md).

| Method | Description |
|--------|-------------|
| `generate_migration(description)` | Generate UP/DOWN SQL from model schema diff |
| `load_snapshot()` | Load saved schema snapshot |
| `save_snapshot(snapshot)` | Save schema snapshot |

### `reflect_table(table_name)`

Introspect a database table and return a Python model class definition as a string.

## `bridge.api.generate`

### `generate_router(model_class, prefix, tags)`

Generate a FastAPI router with CRUD endpoints for the model.

## `bridge.admin`

### `AdminPanel(title, registry)`

FastAPI-based admin panel with JWT auth, CSRF protection, and rate limiting.
