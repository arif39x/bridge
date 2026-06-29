from dataclasses import dataclass, field
from enum import Enum, auto
from typing import Any, AsyncIterator, Dict, List, Optional, Type

import bridge_rs

from ..common.exceptions import DatabaseError, ValidationError


class EagerLoadingStrategy(Enum):
    # Explicit enum prevents callers from passing magic strings like
    # "joined" or "select_in" that would silently degrade to a default.
    JOINED_FOR_TO_ONE = auto()
    SELECT_IN_FOR_TO_MANY = auto()


@dataclass
class EagerLoadRequest:
    """
    Describes a single relation the caller wants pre-loaded.

    WHY: Using a dataclass instead of a raw dict forces callers to be
    explicit about strategy — the compiler/type-checker enforces intent.
    """

    relation_name: str
    strategy: EagerLoadingStrategy


class Raw:
    """Wrapper for raw SQL expressions with bound parameters."""

    __slots__ = ("sql", "params")

    def __init__(self, sql: str, *params: Any) -> None:
        self.sql = sql
        self.params = list(params)


class QueryBuilder:
    # Fluent interface for building and executing database queries.
    # Rule: Use __slots__ on hot-path classes.

    __slots__ = (
        "model_class",
        "_filters",
        "_raw_filters",
        "_limit",
        "_projection",
        "_eager_load_requests",
    )

    def __init__(self, model_class: Type["BaseModel"]) -> None:

        # Initialize the QueryBuilder.
        # Args:
        #   model_class: The model class to query.

        self.model_class = model_class
        self._filters: Dict[str, Any] = {}
        self._raw_filters: Dict[str, "Raw"] = {}
        self._limit: Optional[int] = None
        self._projection: Optional[List[str]] = None
        self._eager_load_requests: List[EagerLoadRequest] = []

    def select(self, *fields: str) -> "QueryBuilder":

        # Restrict the SQL projection to the specified columns.
        # Args:
        #    *fields: Column names to select.
        # Returns:
        #    The QueryBuilder instance for chaining.

        self._projection = list(fields)
        return self

    def filter(self, **kwargs: Any) -> "QueryBuilder":
        for col, val in kwargs.items():
            if isinstance(val, Raw):
                raise TypeError(
                    f"Raw SQL expressions are not allowed in filter(). "
                    f"Use raw_filter('{col}', {val!r}) for explicit opt-in."
                )
        self._filters.update(kwargs)
        return self

    def raw_filter(self, column: str, raw_expr: "Raw") -> "QueryBuilder":
        self._raw_filters[column] = raw_expr
        return self

    def limit(self, count: int) -> "QueryBuilder":
        """
        Limit the number of results returned.

        Args:
            count: Maximum number of rows to return.

        Returns:
            The QueryBuilder instance for chaining.
        """

        if count < 0:
            raise ValidationError("Limit count must be non-negative")
        self._limit = count
        return self

    def with_relation(
        self,
        relation_name: str,
        strategy: EagerLoadingStrategy,
    ) -> "QueryBuilder":
        """
        Registers a relation to be eagerly loaded using the specified strategy.

        WHY: Fluent API allows chaining while each call mutates only the
        eager-load list — a single, well-scoped responsibility.
        """
        self._eager_load_requests.append(
            EagerLoadRequest(
                relation_name=relation_name,
                strategy=strategy,
            )
        )
        return self

    def prefetch_related(self, *relation_names: str) -> "QueryBuilder":
        """
        Django-style API for eager loading relations using batch SELECT IN.

        WHY: Defaults to SELECT_IN strategy which is safe for all relation
        types (both to-one and to-many) without risk of Cartesian explosion.
        """
        for name in relation_names:
            self._eager_load_requests.append(
                EagerLoadRequest(
                    relation_name=name,
                    strategy=EagerLoadingStrategy.SELECT_IN_FOR_TO_MANY,
                )
            )
        return self

    def _build_query_ast_payload(self) -> dict:
        """
        Converts internal builder state to a JSON-serialisable dict that
        the Rust Query AST compiler understands.

        WHY: Isolated as a private method so it can be tested independently
        without triggering a real database call.
        """
        return {
            "table": self.model_class.table,
            "filters": self._merged_filters(),
            "raw_filters": dict(self._raw_filters),
            "limit": self._limit,
            "projection": self._projection,
            "eager_loads": [
                {
                    "relation_name": request.relation_name,
                    "strategy": request.strategy.name,
                }
                for request in self._eager_load_requests
            ],
        }

    def _merged_filters(self) -> Dict[str, Any]:
        filters = dict(self._filters)
        filters.update(self._raw_filters)
        return filters

    async def fetch(self, tx: Any = None) -> List[Any]:
        """
        Execute the query and return all results as model instances.

        Returns:
            A list of model instances.

        Raises:
            DatabaseError: If the database engine returns an error.
        """
        filters = self._merged_filters()
        try:
            # Handle Session or TxHandle
            rs_tx = tx._rs_session if hasattr(tx, "_rs_session") else tx
            eager_loads_payload = [
                {
                    "relation_name": req.relation_name,
                    "strategy": req.strategy.name,
                }
                for req in self._eager_load_requests
            ]
            raw_results = await bridge_rs.fetch_all(
                self.model_class.table,
                filters,
                self._limit,
                self._projection,
                eager_loads_payload,
                tx=rs_tx,
            )
            instances = []
            for res in raw_results:
                instance = self.model_class(**res)
                if hasattr(tx, "set_entity"):
                    instance._session = tx
                if self._projection:
                    instance._projected_fields = self._projection

                # Identity Map population
                if hasattr(tx, "set_entity") and not self._projection:
                    pk_values = tuple(
                        getattr(instance, k) for k in self.model_class._primary_keys
                    )
                    tx.set_entity(self.model_class, pk_values, instance)

                instances.append(instance)

            # Eager loading — batch-fetch relations for all parent instances
            if self._eager_load_requests and instances:
                from .relations import BelongsToMany, HasMany, SelfReferential

                pk_col = self.model_class._primary_keys[0]
                parent_ids = [str(getattr(inst, pk_col)) for inst in instances]

                for req in self._eager_load_requests:
                    descriptor = getattr(self.model_class, req.relation_name)
                    target_cls = descriptor._resolve_target()

                    if isinstance(descriptor, HasMany):
                        grouped = await bridge_rs.batch_fetch_one_to_many(
                            target_cls.table,
                            descriptor.foreign_key,
                            parent_ids,
                            tx=rs_tx,
                        )
                    elif isinstance(descriptor, BelongsToMany):
                        grouped = await bridge_rs.batch_fetch_many_to_many(
                            target_cls.table,
                            descriptor.junction,
                            descriptor.left_key,
                            descriptor.right_key,
                            parent_ids,
                            tx=rs_tx,
                        )
                    elif isinstance(descriptor, SelfReferential):
                        grouped = await bridge_rs.batch_fetch_self_ref(
                            target_cls.table,
                            descriptor.parent_key,
                            parent_ids,
                            tx=rs_tx,
                        )
                    else:
                        continue

                    for inst in instances:
                        parent_id = str(getattr(inst, pk_col))
                        related_data_list = grouped.get(parent_id, [])
                        related = []
                        for data in related_data_list:
                            rel_inst = target_cls(**data)
                            if hasattr(tx, "set_entity"):
                                rel_inst._session = tx
                                pk_vals = tuple(
                                    getattr(rel_inst, k)
                                    for k in target_cls._primary_keys
                                )
                                tx.set_entity(target_cls, pk_vals, rel_inst)
                            related.append(rel_inst)
                        setattr(inst, req.relation_name, related)

            return instances
        except Exception as e:
            raise DatabaseError(f"Fetch failed: {e}") from e

    async def fetch_arrow(self, tx: Any = None) -> List[Any]:
        """
        Execute the query using Apache Arrow for high-performance marshalling.
        Returns LazyModelProxy instances that materialize on access.
        """
        import io

        import pyarrow as pa

        from .lazy import LazyModelProxy

        filters = self._merged_filters()
        try:
            # Handle Session or TxHandle
            rs_tx = tx._rs_session if hasattr(tx, "_rs_session") else tx
            ipc_bytes = await bridge_rs.fetch_all_arrow(
                self.model_class.table, filters, self._limit, self._projection, tx=rs_tx
            )

            if not ipc_bytes:
                return []

            with pa.ipc.open_stream(io.BytesIO(ipc_bytes)) as reader:
                batch = reader.read_next_batch()

            return [
                LazyModelProxy(
                    batch,
                    i,
                    self.model_class,
                    session=tx,
                    projected_fields=self._projection,
                )
                for i in range(batch.num_rows)
            ]
        except Exception as e:
            raise DatabaseError(f"Arrow fetch failed: {e}") from e

    async def fetch_lazy(self, tx: Any = None) -> AsyncIterator[Any]:
        """
        Execute the query and return an async iterator for the results.

        Returns:
            An async iterator of model instances.
        """
        filters = self._merged_filters()
        # Handle Session or TxHandle
        rs_tx = tx._rs_session if hasattr(tx, "_rs_session") else tx
        stream = bridge_rs.fetch_lazy(
            self.model_class.table, filters, self._limit, self._projection, tx=rs_tx
        )

        async for item in stream:
            instance = self.model_class(**item)
            if hasattr(tx, "set_entity"):
                instance._session = tx
            if self._projection:
                instance._projected_fields = self._projection

            # Identity Map population
            if hasattr(tx, "set_entity") and not self._projection:
                pk_values = tuple(
                    getattr(instance, k) for k in self.model_class._primary_keys
                )
                tx.set_entity(self.model_class, pk_values, instance)

            yield instance

    async def first(self, tx: Any = None) -> Optional[Any]:
        """
        Execute the query and return the first result, or None if no results.

        Returns:
            A model instance or None.
        """
        res = await self.limit(1).fetch(tx=tx)
        return res[0] if res else None
