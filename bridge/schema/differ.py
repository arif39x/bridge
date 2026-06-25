from typing import List, Dict, Tuple, Optional
from .snapshot import SchemaSnapshot, TableSnapshot, ColumnSnapshot


class DiffOp:
    pass


class CreateTable(DiffOp):
    def __init__(self, table: TableSnapshot):
        self.table = table


class DropTable(DiffOp):
    def __init__(self, table_name: str):
        self.table_name = table_name


class RenameTable(DiffOp):
    def __init__(self, old_name: str, new_name: str):
        self.old_name = old_name
        self.new_name = new_name


class AddColumn(DiffOp):
    def __init__(self, table_name: str, column: ColumnSnapshot):
        self.table_name = table_name
        self.column = column


class DropColumn(DiffOp):
    def __init__(self, table_name: str, column_name: str):
        self.table_name = table_name
        self.column_name = column_name


class RenameColumn(DiffOp):
    def __init__(self, table_name: str, old_name: str, new_name: str):
        self.table_name = table_name
        self.old_name = old_name
        self.new_name = new_name


class AlterColumn(DiffOp):
    def __init__(self, table_name: str, old_column: ColumnSnapshot, new_column: ColumnSnapshot):
        self.table_name = table_name
        self.old_column = old_column
        self.new_column = new_column


def diff(old_schema: SchemaSnapshot, new_schema: SchemaSnapshot) -> List[DiffOp]:
    ops = []

    old_tables_by_id = {t.stable_id: t for t in old_schema.tables.values()}
    new_tables_by_id = {t.stable_id: t for t in new_schema.tables.values()}

    # Detect Created or Renamed Tables
    for new_id, new_table in new_tables_by_id.items():
        if new_id not in old_tables_by_id:
            ops.append(CreateTable(new_table))
        else:
            old_table = old_tables_by_id[new_id]
            if old_table.name != new_table.name:
                ops.append(RenameTable(old_table.name, new_table.name))

            # Diff columns
            ops.extend(_diff_columns(old_table, new_table))

    # Detect Dropped Tables
    for old_id, old_table in old_tables_by_id.items():
        if old_id not in new_tables_by_id:
            ops.append(DropTable(old_table.name))

    return ops


def _diff_columns(old_table: TableSnapshot, new_table: TableSnapshot) -> List[DiffOp]:
    ops = []
    old_cols_by_id = {c.stable_id: c for c in old_table.columns.values()}
    new_cols_by_id = {c.stable_id: c for c in new_table.columns.values()}

    # Detect Created or Renamed Columns
    for new_id, new_col in new_cols_by_id.items():
        if new_id not in old_cols_by_id:
            ops.append(AddColumn(new_table.name, new_col))
        else:
            old_col = old_cols_by_id[new_id]
            if old_col.name != new_col.name:
                ops.append(RenameColumn(new_table.name, old_col.name, new_col.name))
            
            if (old_col.data_type != new_col.data_type or 
                old_col.is_nullable != new_col.is_nullable or
                old_col.default_value != new_col.default_value):
                ops.append(AlterColumn(new_table.name, old_col, new_col))

    # Detect Dropped Columns
    for old_id, old_col in old_cols_by_id.items():
        if old_id not in new_cols_by_id:
            ops.append(DropColumn(new_table.name, old_col.name))

    return ops
