import uuid
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Any


@dataclass
class ColumnSnapshot:
    name: str
    data_type: str
    is_nullable: bool = True
    is_primary_key: bool = False
    default_value: Optional[str] = None
    stable_id: str = field(default_factory=lambda: str(uuid.uuid4()))

    def to_dict(self) -> Dict[str, Any]:
        return {
            "name": self.name,
            "data_type": self.data_type,
            "is_nullable": self.is_nullable,
            "is_primary_key": self.is_primary_key,
            "default_value": self.default_value,
            "stable_id": self.stable_id,
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "ColumnSnapshot":
        return cls(**data)


@dataclass
class TableSnapshot:
    name: str
    columns: Dict[str, ColumnSnapshot]
    stable_id: str = field(default_factory=lambda: str(uuid.uuid4()))

    def to_dict(self) -> Dict[str, Any]:
        return {
            "name": self.name,
            "columns": {name: col.to_dict() for name, col in self.columns.items()},
            "stable_id": self.stable_id,
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "TableSnapshot":
        columns = {
            name: ColumnSnapshot.from_dict(col_data)
            for name, col_data in data["columns"].items()
        }
        return cls(name=data["name"], columns=columns, stable_id=data["stable_id"])


@dataclass
class SchemaSnapshot:
    tables: Dict[str, TableSnapshot]
    version: int = 1

    def to_dict(self) -> Dict[str, Any]:
        return {
            "tables": {name: table.to_dict() for name, table in self.tables.items()},
            "version": self.version,
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "SchemaSnapshot":
        tables = {
            name: TableSnapshot.from_dict(table_data)
            for name, table_data in data["tables"].items()
        }
        return cls(tables=tables, version=data.get("version", 1))
