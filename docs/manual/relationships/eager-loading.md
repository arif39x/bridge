# Eager Loading

By default, relationship descriptors are lazy — they fetch data only when awaited. For lists of parent objects, this causes N+1 queries. Eager loading eliminates the problem.

## Eager loading strategies

| Strategy | When to use | How it works |
|----------|-------------|-------------|
| `SELECT_IN_FOR_TO_MANY` | Any relation type | Fetches all parents, collects IDs, runs `SELECT ... WHERE fk IN (...)` |
| `JOINED_FOR_TO_ONE` | To-one relations only | Adds a JOIN to the parent query |

## Using `prefetch_related()`

The simplest API, modeled after Django:

```python
from bridge import BaseModel, HasMany, BelongsToMany

class User(BaseModel):
    table = "users"
    _fields = ["id", "username"]
    id: str
    username: str
    posts = HasMany("Post", foreign_key="user_id")
    groups = BelongsToMany("Group", junction="memberships",
                           left_key="user_id", right_key="group_id")

# Batch-load all relations in one extra query each
users = await User.query().prefetch_related("posts", "groups").fetch()
# Each user now has .posts and .groups pre-populated
for user in users:
    for post in user.posts:  # no await needed, already resolved
        print(post.title)
```

`prefetch_related()` defaults to `SELECT_IN_FOR_TO_MANY` for all relations. This is safe for both to-one and to-many relations because it never causes Cartesian explosion.

## Using `with_relation()`

For explicit strategy control:

```python
from bridge.core.query import EagerLoadingStrategy

users = await User.query()\
    .with_relation("profile", EagerLoadingStrategy.JOINED_FOR_TO_ONE)\
    .with_relation("posts", EagerLoadingStrategy.SELECT_IN_FOR_TO_MANY)\
    .fetch()
```

## How batch loading works

1. The parent query executes and returns all matching parent instances.
2. For each eager load request, `fetch()` identifies the relation type and calls the corresponding Rust batch function:
   - `HasMany` → `bridge_rs.batch_fetch_one_to_many()`
   - `BelongsToMany` → `bridge_rs.batch_fetch_many_to_many()`
   - `SelfReferential` → `bridge_rs.batch_fetch_self_ref()`
3. The Rust side collects all parent IDs, runs batch SELECT queries, and returns results grouped by parent ID.
4. Results are attached to each parent instance as resolved lists (not lazy proxies).

## Identity map integration

Eager-loaded related instances are registered in the session's identity map if a session is provided. This means repeated lookups of the same related row return the cached instance.

## N+1 detection

If you access a lazy relationship inside a loop without eager loading, each iteration triggers a separate query:

```python
users = await User.query().fetch()
for user in users:
    posts = await user.posts  # N extra queries
```

Use `prefetch_related()` or `with_relation()` to collapse these into a single batch query.
