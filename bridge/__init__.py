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

async def execute_raw(sql: str):
    """Execute raw SQL statement."""
    return await bridge_rs.execute_raw(sql)


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

    async def load_related(self, model_class):
        # Using the new generic relation fetcher logic if needed, 
        # but modern code should use RelationDescriptors.
        if model_class.__name__ == "Post":
            return await bridge_rs.fetch_one_to_many("posts", "user_id", str(self.id))
        return []

class Post(BaseModel):
    table = "posts"
    _fields = ["id", "title", "user_id", "created_at", "updated_at"]

    id: str
    title: str
    user_id: str
    created_at: str
    updated_at: str
