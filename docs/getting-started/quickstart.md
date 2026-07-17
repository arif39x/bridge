# Quickstart

This guide takes you from `pip install` to a working query in under 5 minutes.

## Install

```bash
pip install bridge
```

Build from source (requires Rust toolchain):

```bash
pip install maturin && maturin develop
```

## Define a model

Create a file called `models.py`:

```python
from bridge import BaseModel

class User(BaseModel):
    table = "users"
    _fields = ["id", "username", "email"]

    id: str
    username: str
    email: str
```

Every model needs:
- `table` — the SQL table name
- `_fields` — the list of column names
- Type annotations for each field

## Connect and create a record

```python
import asyncio
from bridge import connect, transaction
from models import User

async def main():
    await connect("sqlite::memory:")

    async with transaction() as tx:
        user = await User.create(tx, username="alice", email="alice@example.com")
        print(f"Created user {user.id}")

        found = await User.find_one(tx, id=user.id)
        print(found.to_dict())

asyncio.run(main())
```

Output:

```
Created user 550e8400-e29b-41d4-a716-446655440000
{'id': '550e8400-e29b-41d4-a716-446655440000', 'username': 'alice', 'email': 'alice@example.com'}
```

## What happened

1. `connect()` initializes a connection pool to the database (SQLite in-memory here).
2. `transaction()` opens a session with an active transaction.
3. `User.create()` inserts a row and auto-generates a UUID primary key.
4. `User.find_one()` looks up by primary key (checks the identity map first).
5. The `async with` block commits on success, rolls back on exception.

## Next steps

- [Defining models](defining-models.md) — fields, types, composite keys
- [Basic queries](basic-queries.md) — filter, limit, select, delete
- [Relationships](relationships.md) — one-to-many, many-to-many
