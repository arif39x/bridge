# REST API Generator

The `generate_router()` function creates a FastAPI router from a model class, giving you a CRUD REST API with zero boilerplate.

## Usage

```python
from fastapi import FastAPI
from bridge.api import generate_router
from models import User

app = FastAPI()
app.include_router(generate_router(User))
```

## Generated endpoints

For a `User` model with `table = "users"`:

| Method | Path | Description |
|--------|------|-------------|
| GET | `/users/` | List users (pagination via `?limit=100&offset=0`) |
| GET | `/users/{id}` | Get user by primary key |
| POST | `/users/` | Create user |

### GET `/users/`

```python
response = await client.get("/users/?limit=10")
# => [{"id": "...", "username": "alice", ...}, ...]
```

### GET `/users/{id}`

```python
response = await client.get(f"/users/{user_id}")
# => {"id": "...", "username": "alice", ...}
```

Returns 404 if not found.

### POST `/users/`

```python
response = await client.post("/users/", json={"username": "bob", "email": "bob@example.com"})
# => 201 {"id": "...", "username": "bob", ...}
```

Returns 400 on validation error or hook rejection.

## Customization

```python
router = generate_router(
    User,
    prefix="/api/v1/users",     # default: /{table}
    tags=["Users"]               # default: [table]
)
```

## Current limitations

- Only generates `GET /`, `GET /{id}`, and `POST /`. No PUT or DELETE.
- No pagination metadata in responses (returns a flat list).
- No filtering or search parameters in the default router.
- Assumes string primary key.
