# Defining Models

## Basic declaration

```python
from bridge import BaseModel

class User(BaseModel):
    table = "users"
    _fields = ["id", "username", "email", "created_at", "updated_at"]

    id: str
    username: str
    email: str
    created_at: str
    updated_at: str
```

Required class attributes:

| Attribute | Purpose |
|-----------|---------|
| `table` | SQL table name (used in all generated queries) |
| `_fields` | Ordered list of column names |
| `_primary_keys` | List of primary key column names (defaults to `["id"]`) |

Type annotations determine the SQL column type via a mapping:

| Python type | SQL type |
|-------------|----------|
| `str` | `TEXT` / `VARCHAR` |
| `int` | `INTEGER` |
| `float` | `REAL` |
| `bool` | `BOOLEAN` |
| `uuid.UUID` | `UUID` |
| `datetime` | `TIMESTAMP` |
| `dict` / `list` | `JSON` |
| `Optional[T]` | nullable version of T |

## Composite primary keys

```python
class Membership(BaseModel):
    table = "memberships"
    _fields = ["user_id", "group_id", "role"]
    _primary_keys = ["user_id", "group_id"]

    user_id: str
    group_id: str
    role: str
```

Composite keys require all PK fields in `find_one()`:

```python
member = await Membership.find_one(user_id=user.id, group_id=group.id)
# Missing a PK field raises CompositeKeyError
```

## Separate registries and multiple bases

Use `create_base()` and `registry_scope` for multiple model registries (e.g., plugins or multi-tenant setups):

```python
from bridge import BaseModel, Registry, create_base, registry_scope

plugin_registry = Registry()
PluginBase = create_base("PluginBase")

with registry_scope(PluginBase, plugin_registry):
    class PluginModel(PluginBase):
        table = "plugin_data"
        _fields = ["id", "value"]
        id: str
        value: str
```

## Auto-generated defaults

If a `_primary_keys` field named `id` is missing from `create()`, a UUID v4 is generated automatically. If `created_at` or `updated_at` are in `_fields` and not provided, they default to `datetime.now(timezone.utc)`.
