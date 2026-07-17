# Admin Panel

Bridge provides an optional admin panel built on FastAPI with JWT authentication, CSRF protection, rate limiting, and an auto-generated web interface.

## Setup

```python
from fastapi import FastAPI
from bridge.admin import AdminPanel
from models import User, Post

app = FastAPI()

panel = AdminPanel(title="My App Admin")
panel.register(User)
panel.register(Post)
app.include_router(panel.build())
```

The panel requires `BRIDGE_SECRET_KEY` environment variable for JWT signing.

## Authentication

Built-in credentials:
- Admin: username `admin`, password from `BRIDGE_ADMIN_PASSWORD` env var (default `admin123`)
- Viewer: username `viewer`, password from `BRIDGE_VIEWER_PASSWORD` env var (default `viewer123`)

Authentication uses PBKDF2 password hashing (600,000 iterations) and HS256 JWT tokens with 24-hour expiry.

### Password policy

Configurable via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `BRIDGE_PASSWORD_MIN_LENGTH` | `8` | Minimum password length |
| `BRIDGE_PASSWORD_REQUIRE_UPPER` | `1` | Require uppercase letter |
| `BRIDGE_PASSWORD_REQUIRE_DIGIT` | `1` | Require digit |
| `BRIDGE_PASSWORD_REQUIRE_SPECIAL` | `0` | Require special character |

### Account lockout

- `BRIDGE_MAX_LOGIN_ATTEMPTS` (default 5): Failed attempts before lockout.
- `BRIDGE_LOCKOUT_DURATION` (default 900s/15min): Lockout duration.

## CSRF protection

All mutating endpoints (POST, PUT, DELETE) are protected by:
- Double-submit cookie pattern (random 32-byte hex token).
- Origin/referer header validation.
- Enabled via `csrf_protect` and `origin_check` dependencies.

## Rate limiting

In-memory sliding window rate limiter:
- Default: 100 requests per 60 seconds per IP (or per user if authenticated).
- Applied to all mutating admin API endpoints.
- Returns `Retry-After` header on limit exceeded.

## Admin API endpoints

Registered at `/admin/api`:

| Method | Path | Auth required | Description |
|--------|------|---------------|-------------|
| GET | `/admin/api/{table}` | JWT | List items (paginated, max offset 10000) |
| POST | `/admin/api/{table}` | Admin + CSRF | Create item (max payload 1MB) |
| GET | `/admin/api/{table}/{pk}` | JWT | Get item by primary key |
| PUT | `/admin/api/{table}/{pk}` | Admin + CSRF | Update item |
| DELETE | `/admin/api/{table}/{pk}` | Admin + CSRF | Delete item |

## Web interface

The panel serves an HTML interface at `/admin/` with:
- Model listing (links to each registered model).
- Per-model table views (first 50 rows).
- Login page at `/admin/login`.
- CSRF token endpoint at `/admin/csrf-token`.

## Security considerations

- `BRIDGE_SECRET_KEY` must be set to a strong random value in production.
- Rate limiter is in-memory — not shared across processes. Use a Redis-backed limiter for multi-process deployments.
- Input sanitization enforces 64KB max per string field and 1MB max payload.
- Pagination is capped at offset 10,000 to prevent full table scans.
