import os
import shutil
import pytest
import asyncio
from bridge import BaseModel, connect, execute_raw
from bridge.schema import MigrationEngine, MIGRATIONS_DIR

class MigrationUser(BaseModel):
    table = "migration_users_gen"
    _fields = ["id", "username", "age"]
    
    id: str
    username: str
    age: int

@pytest.fixture
def clean_migrations():
    if os.path.exists(MIGRATIONS_DIR):
        shutil.rmtree(MIGRATIONS_DIR)
    from bridge.core import _MODEL_REGISTRY
    # Preserve original core models but clear test models
    keys_to_remove = [k for k in _MODEL_REGISTRY.keys() if "migration" in k]
    for k in keys_to_remove:
        del _MODEL_REGISTRY[k]
    # Re-register for current test
    _MODEL_REGISTRY[MigrationUser.table] = MigrationUser
    yield
    if os.path.exists(MIGRATIONS_DIR):
        shutil.rmtree(MIGRATIONS_DIR)

def test_migration_generation(clean_migrations):
    engine = MigrationEngine(dialect="sqlite")
    engine.generate_migration("test_init")
    
    files = os.listdir(MIGRATIONS_DIR)
    sql_files = [f for f in files if f.endswith(".sql")]
    assert len(sql_files) >= 1
    
    # Find the one we just created (contains migration_users_gen)
    target_file = None
    for sf in sql_files:
        with open(os.path.join(MIGRATIONS_DIR, sf), "r") as f:
            if "CREATE TABLE migration_users_gen" in f.read():
                target_file = sf
                break
    
    assert target_file is not None

def test_migration_diff(clean_migrations):
    engine = MigrationEngine(dialect="sqlite")
    engine.generate_migration("test_init")
    
    # Simulate adding a field
    if "email" not in MigrationUser._fields:
        MigrationUser._fields.append("email")
    MigrationUser.__annotations__["email"] = str
    
    engine.generate_migration("add_email")
    
    files = sorted(os.listdir(MIGRATIONS_DIR))
    sql_files = [f for f in files if f.endswith(".sql")]
    
    # Find the migration that adds the column
    found_alter = False
    for sf in sql_files:
        with open(os.path.join(MIGRATIONS_DIR, sf), "r") as f:
            if "ALTER TABLE migration_users_gen ADD COLUMN email TEXT NOT NULL" in f.read():
                found_alter = True
                break
    assert found_alter

@pytest.mark.asyncio
async def test_migration_apply(clean_migrations):
    db_file = "migration_apply_test.db"
    if os.path.exists(db_file):
        os.remove(db_file)
        
    engine = MigrationEngine(dialect="sqlite")
    engine.generate_migration("init")
    
    from bridge.cli import main
    import sys
    from unittest.mock import patch
    
    # Mock CLI arguments
    test_args = ["bridge-orm", "migrate", "--url", f"sqlite:{db_file}?mode=rwc"]
    with patch.object(sys, 'argv', test_args):
        await main()
        
    # Verify table exists by attempting an insert
    await connect(f"sqlite:{db_file}")
    # Note: MigrationUser might have 'email' from previous test if not careful, 
    # but we'll just check if it works with what's in _fields
    data = {"id": "1", "username": "miku", "age": "16"}
    if "email" in MigrationUser._fields:
        data["email"] = "miku@example.com"
        
    cols = ", ".join(data.keys())
    vals = ", ".join([f"'{v}'" for v in data.values()])
    await execute_raw(f"INSERT INTO migration_users_gen ({cols}) VALUES ({vals})")
    
    if os.path.exists(db_file):
        os.remove(db_file)
