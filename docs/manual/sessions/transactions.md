# Transactions and Sessions

Bridge uses a Unit of Work pattern. A `Session` wraps a database transaction with an identity map and dirty tracking.

## The `transaction()` context manager

The primary entry point:

```python
from bridge import connect, transaction
from models import User

async def main():
    await connect("sqlite::memory:")

    async with transaction() as tx:
        user = await User.create(tx, username="alice")
        # ... more operations ...
        # Auto-commits on success, auto-rollbacks on exception
```

`transaction()` creates a new `Session`, yields it, then commits on success or rolls back on exception.

## Explicit session lifecycle

```python
from bridge import connect, begin_session

async def main():
    await connect("sqlite::memory:")

    session = await begin_session()
    try:
        user = await User.create(session, username="alice")
        # Read operations populate the identity map
        cached = await User.find_one(session, id=user.id)  # from cache
        await session.commit()
    except BaseException:
        await session.rollback()
        raise
```

## Session features

| Feature | Details |
|---------|---------|
| Identity Map | Returns the same Python object for the same PK within a session |
| Dirty Tracking | `snapshot_entity()` stores pre-modification state; `flush()` computes diffs |
| Automatic Flush | `commit()` calls `flush()` which executes UPDATE statements for changed entities |
| Cache Eviction | LRU eviction when `cache_size` (default 1000) is exceeded |
| Session Expiry | Sessions expire after `max_lifetime` seconds (default 3600), raising `SessionExpiredError` |
| Stats | `session.get_stats()` returns cache size, eviction count, lifetime |

## Identity map semantics

Within a session, fetching the same primary key returns the same Python object:

```python
user_a = await User.find_one(session, id=uid)
user_b = await User.find_one(session, id=uid)
assert user_a is user_b  # same object

user_a.username = "new name"
assert user_b.username == "new name"  # mutation reflected
```

Different sessions are fully isolated:

```python
async with transaction() as tx1:
    async with transaction() as tx2:
        u1 = await User.find_one(tx1, id=uid)
        u2 = await User.find_one(tx2, id=uid)
        assert u1 is not u2  # different instances
```

## Dirty tracking and flush

When you modify a tracked entity and call `session.commit()`, Bridge snapshots the entity at fetch time, computes a diff, and generates UPDATE statements:

```python
async with transaction() as tx:
    user = await User.find_one(tx, id=uid)
    user.username = "new_username"
    # On commit/close: snapshot diff detects change, generates UPDATE
```

The Rust-side dirty tracker stores snapshots keyed by `table:pk_values`. `flush()` iterates tracked entities, retrieves their current values via `to_dict()`, and passes dirty data to `bridge_rs.flush()` which executes the UPDATEs.

## Configuration

```python
from bridge import begin_session

# Custom cache size and lifetime
session = await begin_session(cache_size=5000, max_lifetime=7200)
```

Parameters:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `cache_size` | 1000 | Maximum number of entities in the identity map |
| `max_lifetime` | 3600 | Session lifetime in seconds before expiry |

## Limitation: Task-scoped identity map

The identity map is scoped per `asyncio.Task`. Entities from different tasks do not share a cache, even if they share the same session reference.
