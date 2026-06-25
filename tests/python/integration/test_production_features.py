import asyncio
import pytest
import os
from bridge import connect, BaseModel, User, Post
from bridge.core.relations import HasMany, BelongsToMany, SelfReferential
from bridge.common.exceptions import ProjectionError, CompositeKeyError
from bridge.api.generate import generate_router
from fastapi import FastAPI
from httpx import AsyncClient, ASGITransport

# Define a model with a composite key
class Membership(BaseModel):
    table = "memberships"
    _fields = ["user_id", "group_id", "role"]
    _primary_keys = ["user_id", "group_id"]
    
    user_id: str
    group_id: str
    role: str

# Define models for many-to-many
class Group(BaseModel):
    table = "groups"
    _fields = ["id", "name"]
    id: str
    name: str

# Re-define User to have relations
class EnhancedUser(BaseModel):
    table = "users"
    _fields = ["id", "username", "email"]
    id: str
    username: str
    email: str
    
    posts = HasMany("Post", foreign_key="user_id")
    groups = BelongsToMany("Group", junction="memberships", left_key="user_id", right_key="group_id")

@pytest.mark.asyncio
async def test_production_features():
    db_url = "sqlite::memory:"
        
    await connect(db_url)
    
    # Setup tables
    import bridge_rs
    await bridge_rs.execute_raw("CREATE TABLE users (id TEXT PRIMARY KEY, username TEXT, email TEXT)")
    await bridge_rs.execute_raw("CREATE TABLE posts (id TEXT PRIMARY KEY, title TEXT, user_id TEXT)")
    await bridge_rs.execute_raw("CREATE TABLE groups (id TEXT PRIMARY KEY, name TEXT)")
    await bridge_rs.execute_raw("CREATE TABLE memberships (user_id TEXT, group_id TEXT, role TEXT, PRIMARY KEY (user_id, group_id))")

    # 1. Test Partial Selects
    user = await EnhancedUser.create(username="miku", email="miku@vocaloid.jp")
    
    # Fetch only username
    partial_user = await EnhancedUser.query().filter(id=user.id).select("username").first()
    assert partial_user.username == "miku"
    
    with pytest.raises(ProjectionError):
        print(partial_user.email)

    # 2. Test Composite Keys
    group = await Group.create(name="Singers")
    membership = await Membership.create(user_id=user.id, group_id=group.id, role="Lead")
    
    # Find by composite key
    found_mem = await Membership.find_one(user_id=user.id, group_id=group.id)
    assert found_mem.role == "Lead"
    
    with pytest.raises(CompositeKeyError):
        await Membership.find_one(user_id=user.id) # Missing group_id

    # 3. Test Many-to-Many
    user_groups = await user.groups
    assert len(user_groups) == 1
    assert user_groups[0].name == "Singers"

    # 4. Test API Generation
    app = FastAPI()
    router = generate_router(EnhancedUser)
    app.include_router(router)
    
    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as ac:
        response = await ac.get(f"/users/{user.id}")
        assert response.status_code == 200
        assert response.json()["username"] == "miku"

    print("All production features verified successfully!")

if __name__ == "__main__":
    asyncio.run(test_production_features())
