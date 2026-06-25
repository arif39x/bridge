import pytest
import pytest_asyncio
import logging
import asyncio
from bridge import connect, User, execute_raw

class MockLogHandler(logging.Handler):
    def __init__(self):
        super().__init__()
        self.records = []

    def emit(self, record):
        self.records.append(record)

@pytest_asyncio.fixture
async def db_setup():
    await connect("sqlite::memory:?cache=shared")
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

@pytest.mark.asyncio
async def test_unified_observability(db_setup):
    """Verify that Rust spans are correctly bridged into Python logging."""
    logger = logging.getLogger("bridge.telemetry")
    logger.setLevel(logging.DEBUG)
    handler = MockLogHandler()
    logger.addHandler(handler)

    # Trigger a database operation
    await User.create(username="Miku", email="miku@vocaloid.com")
    
    # Give some time for GIL processing if needed (though it's synchronous in this bridge)
    await asyncio.sleep(0.1)

    # Verify log entry exists
    assert len(handler.records) > 0
    telemetry_msg = handler.records[0].getMessage()
    
    assert "[INSERT]" in telemetry_msg
    assert "users" in telemetry_msg
    assert "μs" in telemetry_msg
    assert "INSERT INTO users" in telemetry_msg

    # Verify query template (no bound values)
    assert "Miku" not in telemetry_msg
    assert "miku@vocaloid.com" not in telemetry_msg
