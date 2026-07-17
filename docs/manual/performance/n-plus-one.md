# N+1 Queries and Eager Loading

## The N+1 problem

Without eager loading, accessing a relationship on each parent triggers a separate query:

```python
users = await User.query().fetch()          # 1 query
for user in users:
    posts = await user.posts                # N queries
```

For 100 users, this runs 101 queries. Total time grows linearly with the number of parents.

## Solution: eager loading

### `prefetch_related()` (batch SELECT IN)

```python
users = await User.query().prefetch_related("posts").fetch()
# 1 query for users + 1 query for all posts = 2 queries total
```

This collects all parent IDs (e.g., `[1, 2, 3, ...]`) and runs `SELECT * FROM posts WHERE user_id IN (?, ?, ...)`. The N extra queries collapse into one.

### `with_relation()` (explicit strategy)

```python
from bridge.core.query import EagerLoadingStrategy

# JOIN-based for to-one relations
users = await User.query()\
    .with_relation("profile", EagerLoadingStrategy.JOINED_FOR_TO_ONE)\
    .fetch()
```

## When eager loading hurts

- **Cartesian explosion**: JOIN-based eager loading on a to-many relation multiplies result rows. A user with 10 posts produces 10 rows, inflated further for additional to-many relations. `SELECT_IN_FOR_TO_MANY` avoids this.
- **Over-fetching**: If you only access a relation for one or two parents, lazy loading may be cheaper. Eager loading pays the cost of fetching for all parents upfront.
- **Large IN lists**: Some databases have a limit on IN-list size (SQLite: 999, PostgreSQL: unlimited but performance degrades). Batch loading may need chunking for very large parent sets.

## Measurement

Use the OpenTelemetry integration to measure query counts and durations:

```python
from bridge import configure_logging

configure_logging(level="debug", slow_query_ms=100)
# Logs every query with duration
```

Or inspect the telemetry events directly:

```python
from bridge import TelemetryBridge
# TelemetryBridge logs slow queries (>100ms) as warnings
```

## Best practices

1. Always use `prefetch_related()` when iterating parents and accessing relationships.
2. Profile with real data volumes before optimizing — lazy loading is fine for single parent access.
3. For nested relations, eager load at each level. Bridge does not automatically cascade.
4. Consider `fetch_lazy()` for read-only iteration over large datasets where eager loading would consume too much memory.
