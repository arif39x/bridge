from contextvars import ContextVar
from typing import Any, Dict, List, Optional, Type, Union, get_type_hints

import bridge_rs

from ..common import CompositeKeyError, HookAbortedError, NotFoundError, ProjectionError
from .hooks import dispatch_hooks
from .query import QueryBuilder

BULK_INSERT_CHUNK_SIZE = 1000

_MODEL_REGISTRY: Dict[str, Type["BaseModel"]] = {}


class BaseModel:
    table: str = ""
    _fields: List[str] = []
    _primary_keys: List[str] = ["id"]
    _projected_fields: Optional[List[str]] = None

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        if cls.table:
            _MODEL_REGISTRY[cls.table] = cls

            field_defs = cls.get_field_definitions()
            columns = []
            for field in cls._fields:
                data_type = field_defs.get(field, "str")
                is_nullable = "Optional" in data_type
                is_pk = field in cls._primary_keys
                columns.append((field, data_type, is_nullable, is_pk))

            bridge_rs.register_entity(cls.table, columns)

    @classmethod
    def _get_hooks(cls):
        if "_hooks" not in cls.__dict__:
            cls._hooks = {}
        return cls._hooks

    @classmethod
    def get_field_definitions(cls) -> Dict[str, str]:
        # Extract field types using Python type hints for the migration engine
        import uuid
        from datetime import datetime

        hints = get_type_hints(cls)
        defs = {}
        # Filter hints to only include fields in _fields
        for field in cls._fields:
            type_obj = hints.get(field, Any)

            # Extract the inner type if it's Optional[...]
            origin = getattr(type_obj, "__origin__", None)
            is_optional = False
            if origin is Union:
                args = getattr(type_obj, "__args__", ())
                if type(None) in args:
                    is_optional = True
                    type_obj = [a for a in args if a is not type(None)][0]

            # Map Python types to canonical names for Rust
            if type_obj is str:
                type_name = "str"
            elif type_obj is int:
                type_name = "int"
            elif type_obj is float:
                type_name = "float"
            elif type_obj is bool:
                type_name = "bool"
            elif type_obj is uuid.UUID:
                type_name = "uuid"
            elif type_obj is datetime:
                type_name = "datetime"
            elif type_obj is dict or type_obj is list:
                type_name = "json"
            else:
                type_name = getattr(type_obj, "__name__", str(type_obj)).lower()

            if is_optional:
                type_name = f"Optional[{type_name}]"
            defs[field] = type_name
        return defs

    @classmethod
    def query(cls) -> QueryBuilder:
        return QueryBuilder(cls)

    @classmethod
    async def create(cls, tx=None, **kwargs) -> Any:
        # Create instance locally for hooks
        class TempInstance:
            pass

        instance = TempInstance()
        for k, v in kwargs.items():
            setattr(instance, k, v)

        await dispatch_hooks(cls, "before_create", instance)

        # Keep original types for generic Rust interface
        data = {k: v for k, v in kwargs.items()}

        # Add default values if missing
        if "id" in cls._primary_keys and "id" not in data:
            import uuid

            data["id"] = uuid.uuid4()

        if "created_at" in cls._fields and "created_at" not in data:
            from datetime import datetime, timezone

            data["created_at"] = datetime.now(timezone.utc)

        if "updated_at" in cls._fields and "updated_at" not in data:
            from datetime import datetime, timezone

            data["updated_at"] = datetime.now(timezone.utc)

        # Call generic Rust insert. tx can be Session or TxHandle.
        # If it's a Session, the Rust FFI extracts its inner transaction.
        rs_tx = tx._rs_session if hasattr(tx, "_rs_session") else tx
        raw_res = await bridge_rs.insert_row(cls.table, data, tx=rs_tx)

        # Map back to model instance
        res = cls(**raw_res)
        if hasattr(tx, "set_entity"):
            res._session = tx

        # Populate Identity Map if in a session
        if hasattr(tx, "set_entity"):
            pk_values = tuple(getattr(res, k) for k in cls._primary_keys)
            tx.set_entity(cls, pk_values, res)

        await dispatch_hooks(cls, "after_create", res)
        return res

    @classmethod
    async def create_many(cls, items: List[Dict[str, Any]], tx=None) -> List[Any]:
        # Insert mulltiple records in batches to optimize FFI crossings.
        results = []
        for i in range(0, len(items), BULK_INSERT_CHUNK_SIZE):
            chunk = items[i : i + BULK_INSERT_CHUNK_SIZE]
            # Prep data with defaults for each item in chunk
            prepared_chunk = []
            for item in chunk:
                data = {k: v for k, v in item.items()}
                if "id" in cls._primary_keys and "id" not in data:
                    import uuid

                    data["id"] = uuid.uuid4()

                if "created_at" in cls._fields and "created_at" not in data:
                    from datetime import datetime, timezone

                    data["created_at"] = datetime.now(timezone.utc)

                if "updated_at" in cls._fields and "updated_at" not in data:
                    from datetime import datetime, timezone

                    data["updated_at"] = datetime.now(timezone.utc)
                prepared_chunk.append(data)

            # Single FFI crossiing for the entire chunk
            rs_tx = tx._rs_session if hasattr(tx, "_rs_session") else tx
            raw_results = await bridge_rs.insert_rows_bulk(
                cls.table, prepared_chunk, tx=rs_tx
            )

            # Map back to model instances and populate Identity Map
            for raw_res in raw_results:
                instance = cls(**raw_res)
                if hasattr(tx, "set_entity"):
                    instance._session = tx
                    pk_values = tuple(getattr(instance, k) for k in cls._primary_keys)
                    tx.set_entity(cls, pk_values, instance)
                results.append(instance)

        return results

    @classmethod
    async def find_one(
        cls, tx=None, fields: Optional[List[str]] = None, **kwargs
    ) -> Optional[Any]:
        #  Check Identity Map first if all primary keys provided and we have a session
        pk_provided = all(k in kwargs for k in cls._primary_keys)

        # Enforcement: If it's a composite key, don't allow 'id' unless it's actually a PK
        if (
            "id" in kwargs
            and "id" not in cls._primary_keys
            and len(cls._primary_keys) > 1
        ):
            raise CompositeKeyError(
                f"Model '{cls.__name__}' has a composite key {cls._primary_keys}. "
                f"Cannot use 'id' for lookup."
            )

        # Enforce full PK if doing a PK-like lookup
        # consider it a PK lookup if ANY of the PKs are in kwargs
        any_pk_provided = any(k in kwargs for k in cls._primary_keys)
        if any_pk_provided and not pk_provided:
            raise CompositeKeyError(
                f"Model '{cls.__name__}' requires ALL primary key fields for PK lookup. "
                f"Missing: {[k for k in cls._primary_keys if k not in kwargs]}"
            )

        if pk_provided and fields is None and hasattr(tx, "get_entity"):
            pk_values = tuple(kwargs[k] for k in cls._primary_keys)
            cached = tx.get_entity(cls.table, pk_values)
            if cached:
                return cached

        # Fetch from DB if not in cache or partial select requested
        filters = kwargs
        raw_res = await bridge_rs.find_one(cls.table, filters, fields=fields)

        if raw_res is None:
            return None

        res = cls(**raw_res)
        if hasattr(tx, "set_entity"):
            res._session = tx
        if fields:
            res._projected_fields = fields

        # Populate cache if it was a full PK fetch and we have a session
        if pk_provided and fields is None and hasattr(tx, "set_entity"):
            pk_values = tuple(getattr(res, k) for k in cls._primary_keys)
            tx.set_entity(cls, pk_values, res)
        return res

    async def delete(self, tx=None) -> None:
        pk_values = {k: getattr(self, k) for k in self._primary_keys if hasattr(self, k)}
        if not pk_values:
            raise ValueError(
                f"Cannot delete '{self.__class__.__name__}' instance: "
                f"no primary key values set"
            )
        rs_tx = tx._rs_session if hasattr(tx, "_rs_session") else tx
        await bridge_rs.delete_row(self.table, pk_values, tx=rs_tx)

    @classmethod
    async def delete_many(cls, tx=None, **filters) -> None:
        rs_tx = tx._rs_session if hasattr(tx, "_rs_session") else tx
        await bridge_rs.delete_row(cls.table, filters, tx=rs_tx)

    def __init__(self, **kwargs):
        self._session = None
        for k, v in kwargs.items():
            setattr(self, k, v)

    def __getattr__(self, name: str) -> Any:
        if self._projected_fields is not None and name in self._fields:
            if name not in self._projected_fields:
                raise ProjectionError(
                    f"Field '{name}' was not included in the SELECT projection. "
                    f"Projected fields: {sorted(self._projected_fields)}. "
                    f"Re-run the query with .select('{name}') or remove the projection."
                )
        raise AttributeError(
            f"'{self.__class__.__name__}' object has no attribute '{name}'"
        )

    def to_dict(self) -> Dict[str, Any]:
        if self._projected_fields:
            return {f: getattr(self, f) for f in self._projected_fields}
        return {f: getattr(self, f) for f in self._fields}

    def to_json(self, indent: Optional[int] = None) -> str:
        # Serialize model instance to JSON.
        import json

        return json.dumps(self.to_dict(), indent=indent)

    def to_xml(self) -> str:
        # Serialize model instance to a basic XML string.
        from xml.sax.saxutils import escape

        data = self.to_dict()
        lines = [f"<{self.table}>"]
        for k, v in data.items():
            lines.append(f"  <{k}>{escape(str(v))}</{k}>")
        lines.append(f"</{self.table}>")
        return "\n".join(lines)
