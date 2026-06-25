from contextlib import asynccontextmanager
from .session import begin_session

@asynccontextmanager
async def transaction():
    """Async context manager for atomic database transactions and session lifecycle."""
    session = await begin_session()
    try:
        yield session
        await session.commit()
    except Exception as e:
        await session.rollback()
        raise e
