import pytest
import asyncio
from bridge import connect, execute_raw
from .models import User
import os
import uuid
from datetime import datetime, timezone

def run_async(coro):
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    try:
        return loop.run_until_complete(coro)
    finally:
        loop.close()

async def setup_db(count):
    db_file = f"bench_{count}.db"
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
    
    users_data = [
        {
            "id": str(uuid.uuid4()),
            "username": f"user_{i}",
            "email": f"user_{i}@example.com",
            "created_at": datetime.now(timezone.utc).isoformat(),
            "updated_at": datetime.now(timezone.utc).isoformat()
        }
        for i in range(count)
    ]
    
    # Bulk insert in chunks of 1000
    for i in range(0, len(users_data), 1000):
        chunk = users_data[i:i + 1000]
        await User.create_many(chunk)
    
    return db_file

def test_benchmark_fetch_10(benchmark):
    db_file = run_async(setup_db(10))
    
    async def run_fetch():
        return await User.query().fetch()
    
    results = benchmark.pedantic(lambda: run_async(run_fetch()), iterations=10, rounds=10)
    assert len(results) == 10
    if os.path.exists(db_file):
        os.remove(db_file)

def test_benchmark_fetch_1000(benchmark):
    db_file = run_async(setup_db(1000))
    
    async def run_fetch():
        return await User.query().fetch()
    
    results = benchmark.pedantic(lambda: run_async(run_fetch()), iterations=5, rounds=5)
    assert len(results) == 1000
    if os.path.exists(db_file):
        os.remove(db_file)

def test_benchmark_fetch_10000(benchmark):
    db_file = run_async(setup_db(10000))
    
    async def run_fetch():
        return await User.query().fetch()
    
    results = benchmark.pedantic(lambda: run_async(run_fetch()), iterations=1, rounds=3)
    assert len(results) == 10000
    if os.path.exists(db_file):
        os.remove(db_file)

def test_benchmark_fetch_arrow_10000(benchmark):
    db_file = run_async(setup_db(10000))
    
    async def run_fetch():
        return await User.query().fetch_arrow()
    
    results = benchmark.pedantic(lambda: run_async(run_fetch()), iterations=1, rounds=3)
    assert len(results) == 10000
    if os.path.exists(db_file):
        os.remove(db_file)
