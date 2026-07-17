# Creating Migrations

Bridge uses a snapshot-based migration engine. It compares your model definitions against the last saved schema snapshot and generates UP/DOWN SQL.

## Workflow

```
bridge makemigrations --dialect sqlite --name add_user_age
```

This produces two files in the `migrations/` directory:

```
migrations/
├── 20250101_120000_add_user_age.sql          # UP migration
└── 20250101_120000_add_user_age_down.sql      # DOWN migration
```

## How it works

1. `MigrationEngine.load_snapshot()` reads `migrations/schema_snapshot.json` (or returns an empty schema if none exists).
2. `MigrationEngine._get_current_model_schema()` reads all registered models, their fields, types, and primary keys.
3. `diff()` compares old and new schemas by stable IDs (not names), so renaming a table or column is detected correctly.
4. Each diff operation renders to UP and DOWN SQL via `_render_op()`.
5. The new snapshot is saved for the next comparison cycle.

## Diff operations detected

| Operation | Example SQL |
|-----------|-------------|
| `CreateTable` | `CREATE TABLE users (...)` |
| `DropTable` | `DROP TABLE users;` |
| `RenameTable` | `ALTER TABLE old_name RENAME TO new_name;` |
| `AddColumn` | `ALTER TABLE users ADD COLUMN age INTEGER NOT NULL;` |
| `DropColumn` | `ALTER TABLE users DROP COLUMN age;` |
| `RenameColumn` | `ALTER TABLE users RENAME COLUMN old TO new;` |
| `AlterColumn` | `ALTER TABLE users ALTER COLUMN age TEXT;` |

## Stable IDs

Each table and column gets a UUID `stable_id` on first discovery. Subsequent comparisons use this ID to track renames:

- Old table "users" with stable_id `abc` → new table "people" with stable_id `abc` → detected as `RenameTable`
- New table with no matching stable_id → detected as `CreateTable`
- Old stable_id missing from new schema → detected as `DropTable`

## Programmatic usage

```python
from bridge.schema import MigrationEngine

engine = MigrationEngine(dialect="postgres")
engine.generate_migration("add_email_to_users")
```

Constructor parameters:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `dialect` | `"sqlite"` | Database dialect for SQL generation |
| `migrations_dir` | `"migrations"` or `$BRIDGE_MIGRATIONS_DIR` | Output directory |
| `models` | `None` | Explicit iterable of model classes |
| `registry` | `None` | A `Registry` instance |
