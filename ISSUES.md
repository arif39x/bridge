# Bridge ORM — Issues & Weaknesses

> Generated from codebase audit — v0.1.0

This document catalogs every known weakness, bug, security concern, and design issue found across the Bridge ORM codebase (Rust + Python). Issues are ordered by severity.

---

## BLOCKER — Stubbed/Incomplete Code 

These code paths will **panic at runtime** or return incorrect results because the implementation was never finished.

### 1. Batch relation loading is completely non-functional (Done)

**File:** `src/engine/loading/batch_relation_loader.rs` — Line 91

The `execute_read_query` method contains `todo!()` and will panic if called. All real query building logic (lines 48–54) is commented out. The `load_to_many_relations` method always returns an empty `Vec` instead of actually querying the database. This means **any code that triggers batch eager loading of relations will crash**.

### 2. Optimistic concurrency control is completely non-functional (Done)

**File:** `src/engine/mutation/version_guarded_updater.rs` — Line 78

The `execute_update` method contains `todo!()` and will panic if called. The updater always hardcodes `let affected_row_count = 1` (line 56), meaning **OCC is completely bypassed** — every update is reported as successful regardless of version conflicts. The `VERSION_COLUMN_NAME` constant is defined but never referenced in executable code.

### 3. Session identity map eviction is broken (FIFO, not LRU) (DONE)

**File:** `bridge/core/session.py` — Lines 45–55

The `get_entity()` method contains `pass` on line 54 where the `_tracked_entities` `OrderedDict` should be re-ordered on access (moving accessed entities to the most-recently-used position). The developer left a comment "This is a bit tricky with just the key" acknowledging the gap. As a result, **the LRU cache is actually FIFO** — the oldest entries are evicted regardless of access patterns, defeating the purpose of LRU.

### 4. Destructive migration down paths are unrecoverable stubs (Done)

**File:** `bridge/schema/migrations.py` — Lines 144, 164

`DropTable` and `DropColumn` down migrations produce `"-- TODO: Manual recovery for DROP TABLE <name>"` and `"-- TODO: Manual recovery for DROP COLUMN <name>"` as SQL comments. These are **not valid SQL** — if a developer needs to roll back a destructive migration, they must write the recovery SQL by hand with no assistance from the framework.

---

## CRITICAL — Runtime Panic / Crash Risk

### 5. Unwrapped Python FFI operations can abort the process   [[Done]]

**File:** `src/ffi/mod.rs` — Lines 106–124

The FFI code calls `py.import_bound("json").unwrap()`, `uuid_module.call_method1(...).unwrap()`, `datetime_cls.getattr("fromisoformat").unwrap()`, `json_module.loads(...).unwrap()`, and more. If the Python runtime is missing these standard library modules, or if a Python exception is raised during these calls, the `.unwrap()` will panic. Because this is inside a PyO3 `#[pyfunction]`, a Rust panic that crosses the FFI boundary can abort the Python interpreter.

### 6. Metadata registry lock poisoning aborts Python [[Done]]

**File:** `src/ffi/mod.rs` — Line 153

`REGISTRY.read().unwrap()` accesses the global metadata `RwLock`. If any previous writer panicked (poisoning the lock), this `.unwrap()` will panic, propagating the abort to the Python interpreter.

### 7. Arrow builder downcasts panic on type mismatch [[Done]]

**File:** `src/engine/arrow.rs` — Lines 57–130

Eleven `.unwrap()` calls on `downcast_mut::<Int64Builder>().unwrap()`, `downcast_mut::<BooleanBuilder>().unwrap()`, `RecordBatch::try_new().unwrap()`, `StreamWriter::try_new().unwrap()`, `writer.write().unwrap()`, and `writer.finish().unwrap()`. If the column type metadata doesn't match the actual data (or any I/O error occurs), these will panic.

### 8. Pool manager and circuit breaker unprotected against poison [[Done]]

**File:** `src/engine/pool_manager.rs` — Lines 22–57

All `RwLock` and `Mutex` accesses use `.unwrap()` — if any lock is poisoned, every pool operation panics.

**File:** `src/engine/circuit_breaker.rs` — Lines 49, 69

Same pattern: `state.lock().unwrap()` will panic on poison.

### 9. Session lock ordering deadlock risk [[Done]]

**File:** `src/engine/session.rs` — Lines 38–70

The methods `remove_entity`, `clear_identity_map`, and `get_stats` all acquire `identity_map.lock()` **first** and then `dirty_tracker.lock()` **second**. If any future code path (or any concurrent operation within the same session) acquires these locks in reverse order, a **deadlock** occurs. There is no documentation or compile-time enforcement of lock ordering.

### 10. `ffi_guard!` uses `AssertUnwindSafe` unsafely [[Complete]]

**File:** `src/ffi/mod.rs` — Lines 1–18

The `ffi_guard!` macro wraps closures with `catch_unwind(AssertUnwindSafe(|| ...))`. `AssertUnwindSafe` suppresses the compiler's `UnwindSafe` check. If the closure captures a `&mut` reference or a non-`UnwindSafe` type, a panic could leave the borrow in an undefined state. Additionally, many PyO3 functions (`insert_row`, `fetch_all`, `delete_row`, batch fetchers) are **not wrapped in `ffi_guard!`** at all, so panics in those functions bypass PyO3's exception conversion.

---

## CRITICAL — Security Vulnerabilities

### 11. SQL injection in migration tracker

**File:** `bridge/cli/cli.py` — Line 86

The migration command interpolates a filename directly into a SQL query:
```python
await execute_raw(
    f"INSERT INTO _bridge_migrations (name) VALUES ('{f}')"
)
```
If an attacker can place a file with a malicious name in the migrations directory (e.g., `'); DROP TABLE users;--.sql`), this executes arbitrary SQL. **CWE-89**

### 12. Hardcoded secret key

**File:** `bridge/admin/auth.py` — Line 7

`SECRET_KEY = "bridge_secret_key"` is hardcoded as a string literal. It is not configurable via environment variable or configuration file. While not currently used for JWT signing (see #13), this would be a critical issue if token signing were enabled. **CWE-798**

### 13. Authentication token scheme is trivially forgeable

**File:** `bridge/admin/auth.py` — Lines 16–26

The "token" is literally `"bridge_token_" + username`. There is no signature, HMAC, or JWT validation — the `SECRET_KEY` defined on line 7 is never used. Any user can forge `bridge_token_admin` to gain admin access. **CWE-287**

### 14. XML injection via unescaped values

**File:** `bridge/core/base.py` — Lines 293–300

The `to_xml()` method interpolates field values directly into XML output without escaping `<`, `>`, `&`, or `"`. If this XML is rendered in a browser, it creates an XSS vector. **CWE-79**

### 15. Unquoted SQL identifiers in relation queries

**File:** `src/engine/relations.rs` — Lines 20–25, 61–70, 102–107, 170–178

Relation fetch functions interpolate table and column names via `format!()` instead of using `dialect.quote_identifier()`. While `validate_identifier()` is called, the identifiers are still emitted bare. This breaks with SQL reserved words, case-sensitive identifiers, or identifiers containing special characters. **CWE-89**

---

## CRITICAL — Security Considerations for the Admin Panel

### 16. No CSRF protection

**File:** `bridge/admin/panel.py`, `bridge/admin/views.py`

The admin panel accepts POST/PUT/DELETE requests without any CSRF token verification. Any authenticated user can be targeted by cross-site request forgery attacks. **CWE-352**

### 17. No rate limiting

None of the API endpoints implement rate limiting, leaving them vulnerable to brute-force attacks on authentication and resource exhaustion through bulk operations.

### 18. No input size limits

**File:** `bridge/admin/views.py`, `bridge/core/base.py`

There are no limits on the size of input data for create/update operations or the pagination offset/limit parameters. An attacker could cause out-of-memory conditions by requesting large offsets or sending oversized payloads. **CWE-770**

### 19. Weak password handling

**File:** `bridge/admin/auth.py`

The admin panel uses a hardcoded token scheme with no password hashing, no password policies, no account lockout, and no multi-factor authentication.

---

## HIGH — Panic via `unwrap()`/`expect()` in Production Paths

Approximately **60+ `unwrap()` and `expect()` calls** exist across all Rust production files. Every one of these can panic at runtime. Key concentrations:

| File | Count | Notable Examples |
|------|-------|-----------------|
| `src/engine/arrow.rs` | 11 | `downcast_mut().unwrap()`, `RecordBatch::try_new().unwrap()`, `writer.write().unwrap()` |
| `src/engine/pool_manager.rs` | 12 | `pools.write().unwrap()`, `urls.read().unwrap()` |
| `src/ffi/mod.rs` | 16+ | `uuid_module.call_method1().unwrap()`, `json_module.loads().unwrap()` |
| `src/engine/db.rs` | ~15 | `pool.acquire().await.unwrap()`, `query.map().unwrap()` |
| `src/engine/hydrator.rs` | 4 | `py.import_bound("json").unwrap()`, `json.loads().unwrap()` |
| `src/engine/session.rs` | 5 | `identity_map.lock().unwrap()` |
| `src/engine/circuit_breaker.rs` | 2 | `state.lock().unwrap()` |
| `src/telemetry/logger.rs` | 3 | `SLOW_QUERY_THRESHOLD.read().unwrap()`, `.expect("setting default subscriber")` |
| `src/ffi/java.rs` | 5 | `.expect("Failed to create Tokio runtime")` |

---

## HIGH — Concurrency & Thread Safety Issues

### 20. Session state has no thread-safety

**File:** `bridge/core/session.py` — Line 12

`_tracked_entities` is a plain `OrderedDict` with no mutex, lock, or any synchronization. In an asyncio context, multiple coroutines sharing a session can call `set_entity`, `get_entity`, or `flush` concurrently, causing data races, corrupted ordering, duplicate entries, or lost updates.

### 21. Session expiration has a TOCTOU race

**File:** `bridge/core/session.py` — Lines 18–20

`_check_lifetime()` uses `time.time()` non-atomically. A session can expire between the check and the actual operation, allowing use of an expired session.

### 22. Pool manager nested lock acquisition

**File:** `src/engine/pool_manager.rs` — Lines 22–26, 48–50

`register()` acquires `pools.write()` then `urls.write()` while holding both write locks. `remove()` acquires three write locks sequentially (`pools`, `urls`, `default_key`). Consistent ordering is only enforced by convention — no compile-time check prevents future code from acquiring them in a different order and causing deadlock.

### 23. Unknown contention on hot-path Mutexes

**File:** `src/engine/db.rs`, `src/engine/metadata.rs`

Heavy use of `std::sync::Mutex` and `RwLock` on hot database operation paths (connection pool access, metadata lookups). Contention behavior under realistic multi-threaded load has never been measured (no benchmarks exist).

---

## HIGH — Error Handling & Exception Safety Issues

### 24. Model registration failures silently swallowed

**File:** `bridge/core/base.py` — Lines 36–42

The entire `try` block around `bridge_rs.register_entity()` catches broad `Exception` and only prints a warning to stderr. If the Rust FFI call fails (bad column definitions, registry locked, version mismatch), **the model is not registered on the Rust side with no indication**. Downstream code that assumes the model is registered will fail with confusing errors.

### 25. Broad `except Exception` throughout Python code

Eight locations catch `Exception` broadly, masking unexpected errors:

| File | Lines | Context |
|------|-------|---------|
| `bridge/core/base.py` | 36 | Model registration — silently skips FFI errors |
| `bridge/api/generate.py` | 28, 38 | API route generation — returns HTTP 400/404 with lost context |
| `bridge/core/query.py` | 241, 277 | Query fetching — wraps all errors as `DatabaseError` |
| `bridge/core/transaction.py` | 11 | Transaction rollback — catches `KeyboardInterrupt` etc. |
| `bridge/cli/cli.py` | 58, 88 | CLI commands — prints warning and continues |

### 26. `raise e` loses stack trace

**File:** `bridge/core/transaction.py` — Line 13

`raise e` instead of bare `raise` resets the traceback, making debugging more difficult.

### 27. Unhandled diff operations silently become SQL comments

**File:** `bridge/schema/migrations.py` — Line 183

`return "-- Unknown OP", "-- Unknown OP"` — if a new diff operation type is added to `differ.py` but not handled in `_render_op`, it silently produces invalid SQL comments instead of failing with an error.

---

## MEDIUM — Dead Code

### 28. `_build_query_ast_payload` is never called

**File:** `bridge/core/query.py` — Lines 133–154

This method builds a query AST dictionary but is never invoked by `fetch()`, `fetch_arrow()`, `fetch_lazy()`, `first()`, or any other method. The docstring says it was "isolated as a private method so it can be tested independently" but no tests call it. It is 100% dead code.

### 29. `_merged_filters()` duplication

**File:** `bridge/core/query.py` — Lines 156–159

`_merged_filters()` is only called by `fetch()`; `fetch_arrow()` and `fetch_lazy()` duplicate the filter-merging logic inline instead of reusing this method.

### 30. Shadowed `to_dict()` method

**File:** `bridge/core/base.py` — Lines 262–268 vs 282–285

The first `to_dict()` definition (lines 262–268) is immediately shadowed by the second (lines 282–285). The first definition is completely unreachable.

### 31. `User.load_related()` legacy stub

**File:** `bridge/__init__.py` — Lines 59–64

This method uses the old per-instance fetch pattern. The docstring itself says "modern code should use RelationDescriptors". It is never called by the current eager-loading code paths.

### 32. `to_dict()` vs `__getattr__` inconsistency

**File:** `bridge/core/base.py` — Lines 270–280, 284

`__getattr__` raises `ProjectionError` for field access on unselected projected fields, but `to_dict()` silently returns `None` via `getattr(self, f, None)`. This means iterating projected-field values via `to_dict()` silently omits fields while direct access throws errors.

---

## MEDIUM — Design & API Issues

### 33. `QueryBuilder.first()` has mutating side effects

**File:** `bridge/core/query.py` — Line 315

`first()` calls `self.limit(1)`, which **permanently mutates** `self._limit`. Calling `first()` and then `fetch()` on the same builder instance returns only 1 row from the `fetch()` call. The builder cannot be reused for both operations.

### 34. Session lifetime raises generic exception

**File:** `bridge/core/session.py` — Lines 18–20

`raise RuntimeError("Session has expired")` uses a generic Python exception instead of a Bridge-specific exception from `bridge/common/exceptions.py`. Callers cannot catch it selectively.

### 35. Migrations directory is hardcoded relative

**File:** `bridge/schema/migrations.py` — Lines 13–14

`MIGRATIONS_DIR = "migrations"` is a relative path with no environment variable, parameter, or configuration override. The engine fails (or creates directories in unexpected locations) if the working directory is not the project root.

### 36. AddColumn/AlterColumn ignores nullability

**File:** `bridge/schema/migrations.py` — Lines 132, 157, 177

The rendered SQL for `ADD COLUMN` and `ALTER COLUMN` does not include `NOT NULL` or `NULL` constraints. The `is_nullable` field from the column snapshot is silently ignored.

### 37. `limit(0)` produces invalid SQL silently

**File:** `bridge/core/query.py` — Line 94

`limit(0)` passes the `count < 0` check but produces SQL `LIMIT 0`, which returns no rows in most databases. This is a silent semantic issue rather than an error.

### 38. Migration engine depends on optional feature gate

**File:** `bridge/schema/migrations.py` — Lines 34–54

The migration engine calls `bridge_rs.execute_raw()`, which requires the `allow-raw-sql` Cargo feature. If the Rust library is compiled without this feature, migrations fail at runtime with a confusing `RuntimeError` rather than a clear pre-check message at import time.

### 39. Reachable `panic!` in query binding

**File:** `src/engine/db.rs` — Line 477

`panic!("RawExpression should have been expanded before binding")` — the `Raw` query value variant can reach this panic path at runtime if the expansion logic has a bug.

---

## MEDIUM — Global Mutable State

### 40. Python model registry is a global mutable dict

**File:** `bridge/core/base.py` — Line 12

`_MODEL_REGISTRY: Dict[str, Type["BaseModel"]]` is a module-level mutable dictionary. Every subclass of `BaseModel` is automatically registered into it via `__init_subclass__`. There is no mechanism to unregister, isolate per-application, or prevent registration. This is re-exported and consumed by the migration engine, creating implicit coupling.

### 41. Rust metadata registry is a global static

**File:** `src/engine/metadata.rs` — Lines 34–35

`REGISTRY: Lazy<RwLock<MetadataRegistry>>` is a global mutable static shared across all threads. It is locked after initialization (no new entities can be registered after locking), but is still a single point of contention and a poisoned-lock risk.

### 42. Global pool manager creates implicit coupling

**File:** `src/engine/pool_manager.rs` — Lines 61–62

`POOL_MANAGER: Lazy<PoolManager>` is a process-wide singleton. Multi-tenant or multi-database setups share this state implicitly. There is no per-request or per-application pool isolation.

### 43. Circuit breaker is process-wide

**File:** `src/engine/db.rs` — Lines 28–31

`CIRCUIT_BREAKER: Lazy<CircuitBreaker>` is process-wide. One misbehaving query against one database table opens the circuit for **all** queries against **all** databases, not just the problematic one.

### 44. Import-time side effects in package init

**File:** `bridge/__init__.py` — Lines 24–25, 49–74

`import bridge` creates a global telemetry singleton (which calls into FFI) and registers two example models (`User`, `Post`) into the global registry. This means importing the framework for lightweight use (e.g., just using the types system) triggers database-related FFI calls.

---

## MEDIUM — Debug/Development Leaks

### 45. `println!` debug statements in production metadata path

**File:** `src/engine/metadata.rs` — Lines 49, 52–54

```rust
println!("DEBUG: Registering entity: {}", table_name);
println!("DEBUG:   Column: {} (type: {}, nullable: {}, pk: {})", ...);
```

These print to stdout on every entity registration, even in production builds.

### 46. `println!` debug statements in hydrator hot path

**File:** `src/engine/hydrator.rs` — Lines 16–35

```rust
println!("DEBUG: No mapping found for table: {}");
println!("DEBUG: Available mappings: {:?}", ...);
println!("DEBUG: Hydrating column: {} (has_meta: {})", ...);
println!("DEBUG:   Meta data_type: {}", ...);
```

These four `println!` calls execute on **every row hydration** (every query result processed). This causes significant performance overhead and leaking internal state in logs.

### 47. `print()` to stderr in model registration

**File:** `bridge/core/base.py` — Line 39

`print(f"Warning: Failed to register entity ...)", file=sys.stderr)` is unstructured output. Should use Python logging.

---

## LOW — Type Safety & Code Quality

### 48. Heavy use of `Any` in Python type hints

| File | Lines | Parameter |
|------|-------|-----------|
| `bridge/core/query.py` | 33, 68, 161, 244, 280, 308 | `*params: Any`, `**kwargs: Any`, `tx: Any` |
| `bridge/core/proxy.py` | 9 | `session: Any` |
| `bridge/core/lazy.py` | 16, 71 | `session: Any`, `value: Any` |
| `bridge/core/session.py` | 57 | `entity: Any` |
| `bridge/core/relations.py` | 6, 28, 58, 95 | `target_model: Any` |

This makes the public API type-unsafe and undermines static analysis with mypy (which is configured as strict).

### 49. `setattr` with model field values from user input

**File:** `bridge/core/base.py` — Lines 106, 260

`BaseModel.create()` and `BaseModel.__init__()` call `setattr(instance, k, v)` for every keyword argument. While keys are filtered to model fields in the API layer, the `create()` and `__init__()` methods themselves do not validate attribute names. If called directly with user data, arbitrary attributes can be set.

### 50. MSSQL dialect migration table syntax is non-portable

**File:** `bridge/schema/migrations.py` — Lines 46–51

The MSSQL migration table `CREATE TABLE` uses `IF NOT EXISTS` wrapped in `IF (SELECT ...)` which is only valid in specific SQL Server versions. Does not handle `NVARCHAR(255)` length for the `name` column.

### 51. False positives in datetime type coercion

**File:** `src/ffi/mod.rs` — Lines 187–194

Any Python object with an `isoformat()` method (e.g., `datetime.date`, custom user classes) is matched as `QueryValue::DateTime`. This can silently convert non-datetime types, potentially causing hard-to-debug errors.

### 52. Fragile typing introspection

**File:** `bridge/core/base.py` — Lines 63, 66, 87

Uses `getattr(type_obj, "__origin__")`, `getattr(type_obj, "__args__")`, `getattr(type_obj, "__name__")` to introspect Python typing metadata. These are CPython implementation details that change between Python versions and typing PEPs (e.g., PEP 604 `X|Y` syntax, `types.GenericAlias`).

---

## LOW — Test Coverage Gaps

### 53. Zero Rust unit tests

**File:** `src/` (all files)

No `#[cfg(test)]` modules or `#[test]` functions exist anywhere in the Rust source code. The `benches/` directory is also empty despite `criterion` being a dev-dependency.

### 54. Zero Python unit tests

**File:** `tests/python/unit/` — contains only `__init__.py`

The unit test directory is empty. All tests are integration-level.

### 55. Only 7 integration tests, all SQLite-only

**File:** `tests/python/integration/`

All 7 integration test files use only SQLite in-memory. The other 11 supported databases (PostgreSQL, MySQL, MSSQL, Oracle, CockroachDB, MariaDB, PlanetScale, Neon, YugabyteDB, Cloudflare D1, Dolt) have **zero test coverage**.

### 56. No CI/CD pipeline

No `.github/workflows/`, `.gitlab-ci.yml`, or any CI configuration file found. There is no automated test execution, linting, or type-checking pipeline.

### 57. No performance benchmarks

`criterion` is listed as a Rust dev-dependency but the `benches/` directory is empty. The single Python benchmark test (`test_benchmarks.py`) measures round-trip time for CRUD operations but covers only SQLite.

---

## LOW — Miscellaneous

### 58. String round-trip inefficiency in FFI

**File:** `src/ffi/mod.rs` — Lines 107, 114

UUIDs are serialized to strings (`.to_string()`) and then parsed back into Python UUID objects via `uuid.UUID(...)`. Datetimes are serialized to RFC 3339 strings and then parsed via `fromisoformat`. This is an unnecessary serialization/deserialization step across the FFI boundary.

### 59. Raw SQL feature gate has a fallthrough path

**File:** `src/ffi/mod.rs` — Lines 201–228

When `allow-raw-sql` is disabled and a user passes an object with `sql` and `params` attributes, the attribute extraction on lines 201–203 happens **before** the feature gate check. If the object doesn't have these attributes, execution silently falls through to default string conversion instead of raising a clear error.

### 60. Migrations CLI uses raw SQL via f-string

**File:** `bridge/cli/cli.py` — Line 86

Beyond the SQL injection concern (#11), the CLI uses `execute_raw()` with an f-string for all migration tracking operations. This bypasses the parameterized query system that Bridge's security model is built on.

### 61. `ffi/java.rs` untested

**File:** `src/ffi/java.rs`

The JNI bindings have no tests and use `.expect()` calls that panic if Java interop fails. Panics across JNI are undefined behavior in many cases.

### 62. Data-science feature is untested

**File:** `src/engine/arrow.rs` (feature-gated behind `data-science`)

The `fetch_arrow()` and `LazyModelProxy` code paths have no test coverage. The Arrow code contains 11 `.unwrap()` calls (#7) that will panic on any data type mismatch.

---

## Severity Distribution

| Severity | Count | Key Concerns |
|----------|-------|-------------|
| 🔴 Blocker | 4 | `todo!()` stubs in batch loader, OCC, LRU eviction, migration down paths |
| 🔴 Critical | 11 | Unprotected panics, SQL injection, forged auth, XML injection, unquoted identifiers |
| 🟠 High | 4 | ~60 `unwrap()`/`expect()`, deadlock risk, race conditions, broad exception catches |
| 🟡 Medium | 16 | Dead code, global mutable state, design issues, debug leak in hot path |
| 🔵 Low | 22 | Type safety, test gaps, inefficiencies, untested features |
| **Total** | **57** | |

> **Note on counts:** Items with multiple locations (e.g., "60+ unwrap calls") are grouped under a single row. The numbered items total 62 individual findings, with the `unwrap`/`expect` cluster (item #17) and test coverage cluster (items #53–57) each counting as one.
>
> *Last updated: 2026-06-28*
