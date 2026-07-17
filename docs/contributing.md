# Contributing

## Development setup

```bash
# Install maturin for Rust/Python dev
pip install maturin pytest pytest-asyncio

# Build and install in dev mode
maturin develop

# Run tests
pytest
```

## Codebase conventions

See the [project rules](../Readme.md#rules) for coding standards.

Key points:

- **FFI boundary safety**: Every new function crossing the Python/Rust boundary must use `ffi_guard!` to prevent panics from propagating.
- **Dialect agnosticism**: Never write SQL specific to one database in the core engine. SQL generation must go through the `Dialect` trait.
- **No string interpolation in queries**: All values must be parameterized through `sqlx` binding. Any code that builds SQL by concatenating strings will be rejected.
- **Telemetry integrity**: Every database operation must include tracing spans.

## Project structure

```
bridge/              # Python package (expression layer)
  __init__.py        # Package root, lazy telemetry setup
  core/
    base.py          # BaseModel, Registry, field validation
    query.py         # QueryBuilder, EagerLoadingStrategy, Raw
    session.py       # Session, begin_session
    transaction.py   # transaction() context manager
    relations.py     # HasMany, BelongsToMany, SelfReferential
    hooks.py         # hook_decorator, dispatch_hooks
    proxy.py         # LazyProxy for relationship resolution
    lazy.py          # LazyModelProxy for Arrow materialization
  schema/
    migrations.py    # MigrationEngine
    differ.py        # Schema diff operations
    snapshot.py      # Schema snapshot data classes
    introspect.py    # Table reflection
  admin/             # FastAPI admin panel
  api/               # REST API generator
  cli/               # CLI entry point
  common/
    exceptions.py    # Error hierarchy
  ecosystem/
    pytest_plugin.py # Test fixtures
    factory_boy.py   # Factory base class
src/                 # Rust engine (bridge_rs)
  lib.rs             # Crate root
  engine/
    db.rs            # Dialect trait, SQL generation, query execution
    query.rs         # QueryValue enum
    session.rs       # Rust-side Session
    transaction.rs   # TxHandle
    metadata.rs      # MetadataRegistry
    pool_manager.rs  # Connection pool management
    hydrator.rs      # Row → Python dict conversion
    dirty_tracker.rs # Snapshot-based diff
    relations.rs     # Relation fetching
    circuit_breaker.rs # Circuit breaker pattern
    mutation/        # Version-guarded updates
    identity_map/    # DashMap-based row cache
    loading/         # Batch relation loader
  ffi/               # PyO3 FFI layer
  schema/            # Schema introspection
  telemetry/         # OpenTelemetry integration
tests/
  python/
    unit/            # Unit tests
    integration/     # Integration tests (models, migrations, etc.)
```

## Running tests

```bash
# All tests
pytest

# Specific test file
pytest tests/python/integration/test_async_core.py -v

# With coverage
pytest --cov=bridge
```

## Adding a new database dialect

1. Add a variant to `SqlDialect` enum in `src/engine/db.rs`.
2. Implement the `Dialect` trait for that variant.
3. Add the `from_url()` mapping for connection strings.
4. Document the dialect in the README's supported databases table.

## Feature flags

Features that add significant dependencies or have narrow use cases should be gated behind Cargo features. See `Cargo.toml` for existing flags.
