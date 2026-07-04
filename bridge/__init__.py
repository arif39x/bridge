import bridge_rs
import logging

from .core import BaseModel, transaction, HasMany, BelongsToMany, SelfReferential, Session
from .common import (
    BridgeError, ConnectionError, QueryError, NotFoundError, 
    ConstraintError, HookAbortedError, ValidationError, DatabaseError,
    ProjectionError, CompositeKeyError
)

# Setup internal telemetry bridge
class TelemetryBridge:
    def __init__(self):
        self.logger = logging.getLogger("bridge.telemetry")

    def handle_telemetry(self, event: dict):
        """Standard handler for Rust telemetry events."""
        msg = f"[{event['operation']}] {event['table']} | {event['duration_micros']}μs | SQL: {event['sql']}"
        if event['duration_micros'] > 100000: # 100ms
            self.logger.warning(f"SLOW QUERY: {msg}")
        else:
            self.logger.debug(msg)

_bridge = TelemetryBridge()
bridge_rs.set_telemetry_logger(_bridge)

async def connect(url: str):
    """Initialise the database connection pool."""
    return await bridge_rs.connect(url)

if hasattr(bridge_rs, "execute_raw"):
    async def execute_raw(sql: str):
        """Execute raw SQL statement (requires Cargo feature `allow-raw-sql`)."""
        return await bridge_rs.execute_raw(sql)
else:
    async def execute_raw(sql: str):
        """Execute raw SQL statement (requires Cargo feature `allow-raw-sql`)."""
        raise RuntimeError(
            "bridge_rs was compiled without `allow-raw-sql` feature. "
            "Rebuild with `cargo build --features allow-raw-sql` to use execute_raw."
        )


def configure_logging(level: str = "info", slow_query_ms: int = 100):
    """Configure structured query logging."""
    bridge_rs.configure_logging(level, slow_query_ms)

# Pre-defined models for convenience
class User(BaseModel):
    table = "users"
    _fields = ["id", "username", "email", "created_at", "updated_at"]

    id: str
    username: str
    email: str
    created_at: str
    updated_at: str

class Post(BaseModel):
    table = "posts"
    _fields = ["id", "title", "user_id", "created_at", "updated_at"]

    id: str
    title: str
    user_id: str
    created_at: str
    updated_at: str
