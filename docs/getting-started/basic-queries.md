# Basic Queries

## Creating records

```python
# Single record
user = await User.create(username="bob", email="bob@example.com")

# With explicit transaction
async with transaction() as tx:
    user = await User.create(tx, username="bob", email="bob@example.com")

# Bulk insert (batched in chunks of 1000)
users = await User.create_many([
    {"username": f"user_{i}", "email": f"user_{i}@example.com"}
    for i in range(100)
])
```

## Finding records

```python
# By primary key
user = await User.find_one(id="some-uuid")

# By any field
user = await User.find_one(username="alice")

# Returns None if not found
user = await User.find_one(id="nonexistent")
# => None

# Partial select (avoids fetching all columns)
user = await User.query().filter(id=uid).select("username").first()
print(user.username)  # OK
print(user.email)     # raises ProjectionError
```

## Query builder

```python
# All records
users = await User.query().fetch()

# Filtered
users = await User.query().filter(username="alice").fetch()

# Filtered with limit
users = await User.query().filter(role="admin").limit(10).fetch()

# First result only
user = await User.query().filter(email="alice@example.com").first()

# Lazy streaming (async iterator, no full materialization)
async for user in User.query().fetch_lazy():
    print(user.username)

# Arrow IPC (requires `data-science` feature)
users = await User.query().fetch_arrow()
for proxy in users:
    print(proxy.username)  # materializes on access
```

## Deleting records

```python
# Instance delete
user = await User.find_one(id=uid)
await user.delete()

# Bulk delete by filter
await User.delete_many(role="inactive")
```

## Serialization

```python
user = await User.find_one(id=uid)

user.to_dict()
# => {"id": "...", "username": "alice", "email": "alice@example.com"}

user.to_json(indent=2)
# => JSON string

user.to_xml()
# => XML string
```

Partial models serialize only projected fields:

```python
partial = await User.query().select("username").first()
partial.to_dict()
# => {"username": "alice"}
```
