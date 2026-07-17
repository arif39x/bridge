# Testing with Bridge

Bridge provides a pytest fixture and a factory_boy base class for test data creation.

## Pytest fixtures

```python
# conftest.py
pytest_plugins = ["bridge.ecosystem.pytest_plugin"]
```

Or import directly:

```python
import pytest
from bridge.ecosystem.pytest_plugin import db_session, migrated_db

@pytest.mark.asyncio
async def test_user_creation(db_session):
    user = await User.create(db_session, username="test", email="test@example.com")
    assert user.id is not None

    found = await User.find_one(db_session, id=user.id)
    assert found is user  # identity map: same object
```

`db_session` fixture:
- Opens a session with an active transaction.
- Yields the session.
- Rolls back the transaction on teardown (even on success), so tests are isolated.

`migrated_db` fixture:
- Connects to the database (default: `sqlite::memory:`).
- Ready for explicit migration calls if needed.

## factory_boy integration

```python
from bridge.ecosystem.factory_boy import BridgeFactory
import factory

class UserFactory(BridgeFactory):
    class Meta:
        model = User

    username = factory.Sequence(lambda n: f"user_{n}")
    email = factory.LazyAttribute(lambda o: f"{o.username}@example.com")

# Creation requires async
user = await UserFactory.create()
# => User(id=UUID(...), username="user_0", email="user_0@example.com")

# Batches
users = await UserFactory.create_batch_async(10)
# => [User(...), ...]

# With explicit session
async with transaction() as tx:
    user = await UserFactory.create(session=tx)
```

## Test patterns from the test suite

### Lifecycle test

```python
@pytest.mark.asyncio
async def test_user_lifecycle(db_session):
    user = await User.create(db_session, username="Miku", email="miku@example.com")
    assert user.username == "Miku"

    found = await User.find_one(db_session, id=user.id)
    assert found.username == "Miku"

    results = await User.query().filter(username="Miku").fetch()
    assert len(results) == 1
```

### Bulk insert

```python
@pytest.mark.asyncio
async def test_bulk_insert(db_session):
    users = await User.create_many([
        {"username": f"user_{i}", "email": f"user_{i}@example.com"}
        for i in range(10)
    ])
    assert len(users) == 10
```

### Lazy iterator

```python
@pytest.mark.asyncio
async def test_lazy_iterator(db_session):
    stream = User.query().fetch_lazy()
    users = [u async for u in stream]
    assert len(users) > 0
```

### Transaction rollback

```python
@pytest.mark.asyncio
async def test_transaction_rollback():
    try:
        async with transaction() as tx:
            await User.create(tx, username="Temporary", email="temp@example.com")
            raise ValueError("Forced Rollback")
    except ValueError:
        pass

    found = await User.query().filter(username="Temporary").fetch()
    assert len(found) == 0
```
