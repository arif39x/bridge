import pytest
import pytest_asyncio
import asyncio
import os
from bridge import connect, execute_raw, transaction
from .models import User

@pytest_asyncio.fixture
async def db_setup(request):
    db_file = f"identity_test_{request.node.name}.db"
    if os.path.exists(db_file):
        os.remove(db_file)
    await connect(f"sqlite:{db_file}?mode=rwc")
    await execute_raw("""
        CREATE TABLE users (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL,
            email TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
    """)
    yield
    if os.path.exists(db_file):
        os.remove(db_file)

@pytest.mark.asyncio
async def test_identity_map_same_session(db_setup):
    """Verify that fetching the same ID in the same session returns the same object instance."""
    async with transaction() as session:
        miku = await User.create(username="Miku", email="miku@vocaloid.com", tx=session)
        
        # First fetch
        user_a = await User.find_one(id=miku.id, tx=session)
        # Second fetch
        user_b = await User.find_one(id=miku.id, tx=session)
        
        # Assert physical identity (referential integrity)
        assert user_a is user_b
        assert id(user_a) == id(user_b)
        
        # Mutation on one affects the other (in-memory)
        user_a.username = "Hatsune Miku"
        assert user_b.username == "Hatsune Miku"

@pytest.mark.asyncio
async def test_identity_map_different_sessions(db_setup):
    """Verify that different sessions have isolated identity maps."""
    async with transaction() as session_setup:
        miku = await User.create(username="Miku", email="miku@vocaloid.com", tx=session_setup)
    
    async with transaction() as session_a:
        user_a = await User.find_one(id=miku.id, tx=session_a)
        
        async with transaction() as session_b:
            user_b = await User.find_one(id=miku.id, tx=session_b)
            
            assert user_a.id == user_b.id
            # They should be different instances
            assert user_a is not user_b
            assert id(user_a) != id(user_b)
