import pytest
import bridge_rs
from ..core.session import begin_session

@pytest.fixture
async def db_session():
    """Fixture that provides a transactional session for each test."""
    session = await begin_session()
    # Note: the session already has an open transaction (begin_session calls begin_transaction internally)
    try:
        yield session
    finally:
        await session.rollback()

@pytest.fixture
async def migrated_db(db_url="sqlite::memory:"):
    """Fixture that ensures the database is connected and potentially migrated."""
    await bridge_rs.connect(db_url)
    # Here we could call migration engine to apply initial schema if needed
    return db_url
