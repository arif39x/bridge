import pytest
import pytest_asyncio
import asyncio
from bridge import connect, transaction, execute_raw, NotFoundError
from .models import User, Post

@pytest_asyncio.fixture
async def db_session():
    import os
    if os.path.exists("test.db"):
        os.remove("test.db")
    await connect("sqlite:test.db?mode=rwc")
    await execute_raw("""
        CREATE TABLE users (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL,
            email TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
    """)
    await execute_raw("""
        CREATE TABLE posts (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            user_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
    """)
    yield

@pytest.mark.asyncio
async def test_user_lifecycle(db_session):
    # Test Create
    miku = await User.create(username="Miku", email="bluehairbluetie@gmail.com")
    assert miku.username == "Miku"
    assert miku.id is not None
    
    # Test Find One
    found = await User.find_one(id=miku.id)
    assert found.username == "Miku"
    assert found.id == miku.id
    
    # Test Query
    results = await User.query().filter(username="Miku").fetch()
    assert len(results) == 1
    assert results[0].username == "Miku"

@pytest.mark.asyncio
async def test_bulk_insert(db_session):
    users_data = [
        {"username": f"user_{i}", "email": f"user_{i}@example.com"}
        for i in range(10)
    ]
    results = await User.create_many(users_data)
    assert len(results) == 10
    assert results[0].username == "user_0"
    
    all_users = await User.query().fetch()
    assert len(all_users) == 10

@pytest.mark.asyncio
async def test_lazy_iterator(db_session):
    await User.create(username="Miku", email="miku@vocaloid.com")
    await User.create(username="Luka", email="luka@vocaloid.com")
    
    stream = User.query().fetch_lazy()
    users = []
    async for user in stream:
        users.append(user)
    
    assert len(users) == 2
    assert any(u.username == "Miku" for u in users)
    assert any(u.username == "Luka" for u in users)

@pytest.mark.asyncio
async def test_transaction_rollback(db_session):
    try:
        async with transaction() as tx:
            await User.create(username="Temporary", email="temp@example.com", tx=tx)
            raise ValueError("Forced Rollback")
    except ValueError:
        pass
    
    found = await User.query().filter(username="Temporary").fetch()
    assert len(found) == 0
