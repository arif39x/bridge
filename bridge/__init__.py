import bridge_rs
import logging

from .core import BaseModel, transaction, HasMany, BelongsToMany, SelfReferential, Session, Registry, create_base, registry_scope
from .common import (
    BridgeError, ConnectionError, QueryError, NotFoundError, 
    ConstraintError, HookAbortedError, ValidationError, DatabaseError,
    ProjectionError, CompositeKeyError
)

# Lazy telemetry bridge setup (avoids FFI calls at import time)
class TelemetryBridge:
    def __init__(self):
        self.logger = logging.getLogger("bridge.telemetry")

    def handle_telemetry(self, event: dict):
        """Standard handler for Rust telemetry events."""
        msg = f"[{event['operation']}] {event['table']} | {event['duration_micros']}μs | SQL: {event['sql']}"
        if event['duration_micros'] > 100000:  # 100ms
            self.logger.warning(f"SLOW QUERY: {msg}")
        else:
            self.logger.debug(msg)

_telemetry_installed = False

def _install_telemetry():
    global _telemetry_installed
    if not _telemetry_installed:
        bridge_rs.set_telemetry_logger(TelemetryBridge())
        _telemetry_installed = True

async def connect(url: str):
    """Initialise the database connection pool."""
    _install_telemetry()
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
    _install_telemetry()
    bridge_rs.configure_logging(level, slow_query_ms)
