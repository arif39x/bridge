# Relationships

Bridge supports three relationship types via descriptor classes on model definitions.

All relationships are **lazy by default** — they resolve on first `await`. Use [eager loading](../manual/relationships/eager-loading.md) to batch-fetch.

## HasMany (one-to-many)

```python
from bridge import BaseModel, HasMany

class Post(BaseModel):
    table = "posts"
    _fields = ["id", "title", "user_id"]
    id: str
    title: str
    user_id: str

class User(BaseModel):
    table = "users"
    _fields = ["id", "username"]
    id: str
    username: str
    posts = HasMany("Post", foreign_key="user_id")
```

Usage:

```python
user = await User.find_one(id=uid)
posts = await user.posts  # resolves lazy proxy
# => [Post(id="...", title="Hello", user_id="..."), ...]
```

## BelongsToMany (many-to-many)

```python
class Group(BaseModel):
    table = "groups"
    _fields = ["id", "name"]
    id: str
    name: str

class User(BaseModel):
    table = "users"
    _fields = ["id", "username"]
    id: str
    username: str
    groups = BelongsToMany("Group", junction="memberships",
                           left_key="user_id", right_key="group_id")
```

The `junction` table must exist with at least the two foreign key columns.

## SelfReferential

```python
class Category(BaseModel):
    table = "categories"
    _fields = ["id", "name", "parent_id"]
    id: str
    name: str
    parent_id: str
    children = SelfReferential("Category", parent_key="parent_id")
```

```python
cat = await Category.find_one(id=some_id)
children = await cat.children
# => [Category(...), ...]
```

## Relationship resolution order

1. Resolve the target model class by name from the model registry.
2. Call the corresponding `bridge_rs.fetch_*` function across FFI.
3. Wrap results in model instances.
4. Populate the identity map if a session is active.

Lazy proxies share the parent's session. If no session is available (standalone create without `tx`), lazy loading raises `RuntimeError`.
