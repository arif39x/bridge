from typing import Any, Dict, List, Optional, Type, TYPE_CHECKING
import pyarrow as pa
from ..common.exceptions import ProjectionError

if TYPE_CHECKING:
    from .base import BaseModel

class LazyModelProxy:
    __slots__ = ("_batch", "_row_index", "_model_class", "_session", "_projected_fields", "_materialized_instance")

    def __init__(
        self,
        batch: pa.RecordBatch,
        row_index: int,
        model_class: Type["BaseModel"],
        session: Any = None,
        projected_fields: Optional[List[str]] = None
    ) -> None:
        self._batch = batch
        self._row_index = row_index
        self._model_class = model_class
        self._session = session
        self._projected_fields = projected_fields
        self._materialized_instance: Optional["BaseModel"] = None

    def _materialize(self) -> "BaseModel":
        if self._materialized_instance is None:
            data = {}
            for name in self._batch.schema.names:
                scalar = self._batch.column(name)[self._row_index]
                data[name] = scalar.as_py()
            
            instance = self._model_class(**data)
            instance._session = self._session
            if self._projected_fields:
                instance._projected_fields = self._projected_fields
            
            # Identity Map population
            if hasattr(self._session, "set_entity") and not self._projected_fields:
                pk_values = tuple(getattr(instance, k) for k in self._model_class._primary_keys)
                self._session.set_entity(self._model_class, pk_values, instance)
            
            self._materialized_instance = instance
        return self._materialized_instance

    def __getattr__(self, name: str) -> Any:
        # Avoid recursion for internal slots
        if name.startswith('_') and name in self.__slots__:
             return super().__getattribute__(name)
             
        if self._materialized_instance:
            return getattr(self._materialized_instance, name)

        if name in self._batch.schema.names:
            if self._projected_fields and name not in self._projected_fields:
                raise ProjectionError(
                    f"Field '{name}' was not included in the SELECT projection. "
                    f"Projected fields: {sorted(self._projected_fields)}."
                )
            
            scalar = self._batch.column(name)[self._row_index]
            return scalar.as_py()

        # Delegate class attributes (table, _fields, etc)
        if hasattr(self._model_class, name):
            return getattr(self._model_class, name)

        # If it's not a column or class attr, materialize and delegate (e.g. for methods or relations)
        return getattr(self._materialize(), name)

    def __setattr__(self, name: str, value: Any) -> None:
        if name in self.__slots__:
            super().__setattr__(name, value)
            return
        
        # Any mutation triggers materialization
        setattr(self._materialize(), name, value)

    @property
    def __class__(self):
        return self._model_class

    def __repr__(self) -> str:
        if self._materialized_instance:
            return repr(self._materialized_instance)
        return f"<Lazy{self._model_class.__name__} row={self._row_index}>"

    def to_dict(self) -> Dict[str, Any]:
        if self._materialized_instance:
            return self._materialized_instance.to_dict()
            
        if self._projected_fields:
            return {f: getattr(self, f) for f in self._projected_fields}
        return {f: getattr(self, f) for f in self._model_class._fields}
    
    def to_json(self, indent: Optional[int] = None) -> str:
        import json
        return json.dumps(self.to_dict(), indent=indent)

    def to_xml(self) -> str:
        # Reuse logic from materialize to be safe
        return self._materialize().to_xml()
