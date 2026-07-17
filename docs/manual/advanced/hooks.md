# Lifecycle Hooks

Hooks let you run logic before or after model create operations.

## Available hook points

| Hook point | When it fires |
|------------|---------------|
| `before_create` | After validation, before the INSERT query |
| `after_create` | After the INSERT query, before returning |

## Defining hooks

Use the `hook_decorator` factory to register hooks on a model class:

```python
from bridge.core.hooks import hook_decorator, dispatch_hooks
from bridge import BaseModel

class User(BaseModel):
    table = "users"
    _fields = ["id", "username", "email"]
    id: str
    username: str
    email: str

@hook_decorator("before_create")
def validate_email(instance):
    if "@" not in instance.email:
        return False  # cancels the create operation
```

## Cancelling operations

If a `before_*` hook returns `False`, the operation is cancelled and `HookAbortedError` is raised:

```python
try:
    user = await User.create(username="bad", email="not-an-email")
except HookAbortedError:
    print("Create was cancelled by hook")
```

## How hooks work

`hook_decorator(hook_point)` returns a decorator that registers the function into `cls._hooks[hook_point]`. When `BaseModel.create()` runs, it calls `dispatch_hooks(cls, "before_create", instance)` which iterates registered hooks and checks return values.

Hooks are inherited by subclasses but stored per-class in `cls._hooks`. A subclass gets its parent's hooks only if it accesses `_get_hooks()` which falls back to the parent via normal attribute lookup.
