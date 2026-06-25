import bridge_rs
from typing import Any, Dict, Optional, Type
from collections import OrderedDict
import time

class Session:
    """The Persistence Manager / Session (x_4) — The Unit of Work Managing Object Lifecycle."""

    def __init__(self, rs_session: bridge_rs.Session, cache_size: int = 1000, max_lifetime: int = 3600):
        self._rs_session = rs_session
        # (model_class, pk_values) -> key
        self._tracked_entities = OrderedDict()
        self._cache_size = cache_size
        self._evictions = 0
        self._created_at = time.time()
        self._max_lifetime = max_lifetime

    def _check_lifetime(self):
        if time.time() - self._created_at > self._max_lifetime:
            raise RuntimeError("Session has expired. Please open a new session.")

    async def commit(self):
        self._check_lifetime()
        await self.flush()
        await self._rs_session.commit()

    async def rollback(self):
        await self._rs_session.rollback()

    async def flush(self):
        """Computes changes and executes SQL UPDATEs."""
        self._check_lifetime()
        dirty_data = []
        for (model_class, pk_values), key in self._tracked_entities.items():
            entity = self._rs_session.get_entity(key)
            if entity:
                # Prepare data for Rust diffing
                current_values = entity.to_dict()
                pk_filters = {k: getattr(entity, k) for k in model_class._primary_keys}
                dirty_data.append((key, model_class.table, current_values, pk_filters))
        
        if dirty_data:
            await bridge_rs.flush(self._rs_session, dirty_data)

    def get_entity(self, table: str, pk_values: tuple) -> Optional[Any]:
        self._check_lifetime()
        key = f"{table}:{pk_values}"
        entity = self._rs_session.get_entity(key)
        if entity:
            # Move to end (most recently used)
            # Find the model class from registry if needed, but here we just need to move it
            # We need to know which (model_class, pk_values) it corresponds to.
            # This is a bit tricky with just the key.
            pass
        return entity

    def set_entity(self, model_class: Type["BaseModel"], pk_values: tuple, entity: Any):
        self._check_lifetime()
        key = f"{model_class.table}:{pk_values}"
        
        # If already tracked, move to end
        cache_key = (model_class, pk_values)
        if cache_key in self._tracked_entities:
            self._tracked_entities.move_to_end(cache_key)
        else:
            # Add to tracker
            self._tracked_entities[cache_key] = key
            
            # Check for eviction
            if len(self._tracked_entities) > self._cache_size:
                oldest_key, oldest_rs_key = self._tracked_entities.popitem(last=False)
                self._rs_session.remove_entity(oldest_rs_key)
                self._evictions += 1

        self._rs_session.set_entity(key, entity)
        
        # Take initial snapshot in Rust if it's a new or freshly loaded entity
        bridge_rs.snapshot_entity(self._rs_session, key, model_class.table, entity.to_dict())

    def remove_entity(self, model_class: Type["BaseModel"], pk_values: tuple):
        cache_key = (model_class, pk_values)
        key = self._tracked_entities.pop(cache_key, None)
        if key:
            self._rs_session.remove_entity(key)

    def clear(self):
        """Clears all tracked entities and snapshots."""
        self._tracked_entities.clear()
        self._rs_session.clear_identity_map()
        self._evictions = 0

    def get_stats(self) -> Dict[str, Any]:
        rs_stats = self._rs_session.get_stats()
        return {
            "identity_map_size": len(self._tracked_entities),
            "cache_size": self._cache_size,
            "evictions": self._evictions,
            "rs_stats": rs_stats,
            "lifetime_seconds": time.time() - self._created_at
        }

    async def __aenter__(self) -> "Session":
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        if exc_type:
            await self.rollback()
        else:
            await self.commit()

async def begin_session(cache_size: int = 1000, max_lifetime: int = 3600) -> Session:
    """Entry point for creating a new session."""
    rs_session = await bridge_rs.begin_session()
    return Session(rs_session, cache_size=cache_size, max_lifetime=max_lifetime)
