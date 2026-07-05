import json
import os
import uuid
from datetime import datetime
from typing import Any, Dict, List, Optional, Tuple, Type

import bridge_rs

from ..core import _MODEL_REGISTRY
from .snapshot import SchemaSnapshot, TableSnapshot, ColumnSnapshot
from .differ import diff, CreateTable, DropTable, RenameTable, AddColumn, DropColumn, RenameColumn, AlterColumn

MIGRATIONS_DIR = os.environ.get("BRIDGE_MIGRATIONS_DIR", "migrations")


class MigrationEngine:
    def __init__(self, dialect: str = "sqlite", migrations_dir: Optional[str] = None):
        self.dialect = dialect
        self._migrations_dir = migrations_dir or MIGRATIONS_DIR
        self._snapshot_path = os.path.join(self._migrations_dir, "schema_snapshot.json")
        if not os.path.exists(self._migrations_dir):
            os.makedirs(self._migrations_dir)

    def load_snapshot(self) -> SchemaSnapshot:
        if os.path.exists(self._snapshot_path):
            with open(self._snapshot_path, "r") as f:
                data = json.load(f)
                return SchemaSnapshot.from_dict(data)
        return SchemaSnapshot(tables={})

    def save_snapshot(self, snapshot: SchemaSnapshot):
        with open(self._snapshot_path, "w") as f:
            json.dump(snapshot.to_dict(), f, indent=4)

    async def _ensure_migration_table(self):
        sql = ""
        if "sqlite" in self.dialect:
            sql = ("CREATE TABLE IF NOT EXISTS _bridge_migrations ("
                   "id INTEGER PRIMARY KEY AUTOINCREMENT, "
                   "name TEXT NOT NULL, "
                   "applied_at TEXT NOT NULL);")
        elif "postgres" in self.dialect or "mysql" in self.dialect:
            sql = ("CREATE TABLE IF NOT EXISTS _bridge_migrations ("
                   "id SERIAL PRIMARY KEY, "
                   "name TEXT NOT NULL, "
                   "applied_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP);")
        elif "mssql" in self.dialect:
            sql = ("IF NOT EXISTS (SELECT * FROM sysobjects WHERE name='_bridge_migrations' AND xtype='U') "
                   "CREATE TABLE _bridge_migrations ("
                   "id INT IDENTITY(1,1) PRIMARY KEY, "
                   "name NVARCHAR(255) NOT NULL, "
                   "applied_at DATETIME DEFAULT GETDATE());")

        if sql:
            await bridge_rs.execute_raw(sql)

    def _get_current_model_schema(self) -> SchemaSnapshot:
        tables = {}
        # need to maintain stable IDs. If they exist in old snapshot, reuse them.
        old_snapshot = self.load_snapshot()

        for table_name, model_cls in _MODEL_REGISTRY.items():
            field_defs = model_cls.get_field_definitions()
            columns = {}

            old_table = old_snapshot.tables.get(table_name)
            table_stable_id = old_table.stable_id if old_table else str(uuid.uuid4())

            for field_name, field_type in field_defs.items():
                is_pk = field_name in model_cls._primary_keys
                is_nullable = "Optional" in field_type

                old_col = old_table.columns.get(field_name) if old_table else None
                col_stable_id = old_col.stable_id if old_col else str(uuid.uuid4())

                columns[field_name] = ColumnSnapshot(
                    name=field_name,
                    data_type=field_type,
                    is_nullable=is_nullable,
                    is_primary_key=is_pk,
                    stable_id=col_stable_id
                )

            tables[table_name] = TableSnapshot(
                name=table_name,
                columns=columns,
                stable_id=table_stable_id
            )

        return SchemaSnapshot(tables=tables)

    async def generate_migration(self, description: str = "auto_migration"):
        old_snapshot = self.load_snapshot()
        new_snapshot = self._get_current_model_schema()

        ops = diff(old_snapshot, new_snapshot)

        if not ops:
            print("No changes detected.")
            return

        up_sql = []
        down_sql = []

        for op in ops:
            up, down = self._render_op(op)
            up_sql.append(up)
            down_sql.append(down)

        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        filename = f"{timestamp}_{description}.sql"
        filepath = os.path.join(self._migrations_dir, filename)

        down_filename = f"{timestamp}_{description}_down.sql"
        down_filepath = os.path.join(self._migrations_dir, down_filename)

        with open(filepath, "w") as f:
            f.write(f"-- UP Migration: {description}\n")
            f.write("\n".join(up_sql))

        with open(down_filepath, "w") as f:
            f.write(f"-- DOWN Migration: {description}\n")
            f.write("\n".join(reversed(down_sql)))

        self.save_snapshot(new_snapshot)
        print(f"Generated migration: {filepath}")
        print(f"Generated down migration: {down_filepath}")

    def _column_type_sql(self, col) -> str:
        sql = bridge_rs.resolve_type(col.data_type, self.dialect)
        if col.is_primary_key:
            sql += " PRIMARY KEY"
        if not col.is_nullable:
            sql += " NOT NULL"
        return sql

    def _render_op(self, op) -> Tuple[str, str]:
        if isinstance(op, CreateTable):
            col_defs = []
            for col in op.table.columns.values():
                col_defs.append(f"  {col.name} {self._column_type_sql(col)}")

            up = f"CREATE TABLE {op.table.name} (\n" + ",\n".join(col_defs) + "\n);"
            down = f"DROP TABLE {op.table.name};"
            return up, down

        if isinstance(op, DropTable):
            up = f"DROP TABLE {op.table_name};"
            col_defs = []
            for col in op.table.columns.values():
                col_defs.append(f"  {col.name} {self._column_type_sql(col)}")
            down = f"CREATE TABLE {op.table_name} (\n" + ",\n".join(col_defs) + "\n);"
            return up, down

        if isinstance(op, RenameTable):
            if "sqlite" in self.dialect:
                up = f"ALTER TABLE {op.old_name} RENAME TO {op.new_name};"
                down = f"ALTER TABLE {op.new_name} RENAME TO {op.old_name};"
            else:
                up = f"ALTER TABLE {op.old_name} RENAME TO {op.new_name};"
                down = f"ALTER TABLE {op.new_name} RENAME TO {op.old_name};"
            return up, down

        if isinstance(op, AddColumn):
            up = f"ALTER TABLE {op.table_name} ADD COLUMN {op.column.name} {self._column_type_sql(op.column)};"
            down = f"ALTER TABLE {op.table_name} DROP COLUMN {op.column.name};"
            return up, down

        if isinstance(op, DropColumn):
            up = f"ALTER TABLE {op.table_name} DROP COLUMN {op.column_name};"
            down = f"ALTER TABLE {op.table_name} ADD COLUMN {op.column.name} {self._column_type_sql(op.column)};"
            return up, down

        if isinstance(op, RenameColumn):
            if "sqlite" in self.dialect:
                up = f"ALTER TABLE {op.table_name} RENAME COLUMN {op.old_name} TO {op.new_name};"
                down = f"ALTER TABLE {op.table_name} RENAME COLUMN {op.new_name} TO {op.old_name};"
            else:
                up = f"ALTER TABLE {op.table_name} RENAME COLUMN {op.old_name} TO {op.new_name};"
                down = f"ALTER TABLE {op.table_name} RENAME COLUMN {op.new_name} TO {op.old_name};"
            return up, down

        if isinstance(op, AlterColumn):
            new_type = bridge_rs.resolve_type(op.new_column.data_type, self.dialect)
            old_type = bridge_rs.resolve_type(op.old_column.data_type, self.dialect)
            new_type += " NOT NULL" if not op.new_column.is_nullable else " NULL"
            old_type += " NOT NULL" if not op.old_column.is_nullable else " NULL"
            up = f"ALTER TABLE {op.table_name} ALTER COLUMN {op.new_column.name} {new_type};"
            down = f"ALTER TABLE {op.table_name} ALTER COLUMN {op.new_column.name} {old_type};"
            return up, down

        raise ValueError(f"Unknown diff operation: {type(op).__name__}")
