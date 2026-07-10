from typing import TYPE_CHECKING, Any, List, Optional, Type, Union
import bridge_rs
from .proxy import LazyProxy

if TYPE_CHECKING:
    from .base import BaseModel

class RelationDescriptor:
    def __init__(self, target_model: Union[Type["BaseModel"], str]):
        self._target_model = target_model

    def _resolve_target(self):
        if isinstance(self._target_model, str):
            from .base import _MODEL_REGISTRY

            # assume it's a table name or class name
            for cls in _MODEL_REGISTRY.values():
                if (
                    cls.__name__ == self._target_model
                    or cls.table == self._target_model
                ):
                    return cls
            raise ImportError(f"Could not resolve model {self._target_model}")
        return self._target_model

    def _get_session(self, instance):
        return getattr(instance, "_session", None)


class HasMany(RelationDescriptor):
    def __init__(self, target_model: Union[Type["BaseModel"], str], foreign_key: str):
        super().__init__(target_model)
        self.foreign_key = foreign_key

    def __get__(self, instance, owner):
        if instance is None:
            return self
        
        target_cls = self._resolve_target()
        session = self._get_session(instance)
        
        async def load():
            rs_tx = session._rs_session if session else None
            raw_results = await bridge_rs.fetch_one_to_many(
                target_cls.table, self.foreign_key, str(instance.id), tx=rs_tx
            )
            instances = []
            for res in raw_results:
                inst = target_cls(**res)
                if session:
                    inst._session = session
                    pk_values = tuple(getattr(inst, k) for k in target_cls._primary_keys)
                    session.set_entity(target_cls, pk_values, inst)
                instances.append(inst)
            return instances

        return LazyProxy(session, load)


class BelongsToMany(RelationDescriptor):
    def __init__(self, target_model: Union[Type["BaseModel"], str], junction: str, left_key: str, right_key: str):
        super().__init__(target_model)
        self.junction = junction
        self.left_key = left_key
        self.right_key = right_key

    def __get__(self, instance, owner):
        if instance is None:
            return self
        
        target_cls = self._resolve_target()
        session = self._get_session(instance)

        async def load():
            rs_tx = session._rs_session if session else None
            raw_results = await bridge_rs.fetch_many_to_many(
                target_cls.table,
                self.junction,
                self.left_key,
                self.right_key,
                str(instance.id),
                tx=rs_tx
            )
            instances = []
            for res in raw_results:
                inst = target_cls(**res)
                if session:
                    inst._session = session
                    pk_values = tuple(getattr(inst, k) for k in target_cls._primary_keys)
                    session.set_entity(target_cls, pk_values, inst)
                instances.append(inst)
            return instances

        return LazyProxy(session, load)


class SelfReferential(RelationDescriptor):
    def __init__(self, target_model: Union[Type["BaseModel"], str], parent_key: str):
        super().__init__(target_model)
        self.parent_key = parent_key

    def __get__(self, instance, owner):
        if instance is None:
            return self
        
        target_cls = self._resolve_target()
        session = self._get_session(instance)

        async def load():
            rs_tx = session._rs_session if session else None
            raw_results = await bridge_rs.fetch_self_ref(
                target_cls.table, self.parent_key, str(instance.id), tx=rs_tx
            )
            instances = []
            for res in raw_results:
                inst = target_cls(**res)
                if session:
                    inst._session = session
                    pk_values = tuple(getattr(inst, k) for k in target_cls._primary_keys)
                    session.set_entity(target_cls, pk_values, inst)
                instances.append(inst)
            return instances

        return LazyProxy(session, load)
