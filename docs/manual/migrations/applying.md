# Applying Migrations

## Via CLI

```bash
bridge migrate --url sqlite:dev.db
```

This command:

1. Connects to the database.
2. Creates the `_bridge_migrations` tracking table if it doesn't exist.
3. Reads all `.sql` files from the `migrations/` directory, sorted alphabetically.
4. Applies each file, skipping any whose name already exists in `_bridge_migrations`.
5. Stops on the first error.

## The tracking table

Bridge records applied migrations in `_bridge_migrations`:

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Auto-incrementing PK |
| `name` | TEXT | Migration filename (unique) |
| `applied_at` | TIMESTAMP | When the migration was applied |

## Rollbacks

Each migration generation produces a corresponding `_down.sql` file. To roll back:

```sql
-- Read the down migration and execute it
cat migrations/20250101_120000_add_user_age_down.sql | bridge execute_raw
```

Then delete the tracking record:

```sql
DELETE FROM _bridge_migrations WHERE name = '20250101_120000_add_user_age.sql';
```

## Migration file format

Generated migration files are plain SQL:

```sql
-- UP Migration: add_user_age
ALTER TABLE users ADD COLUMN age INTEGER NOT NULL;
```

Down migration:

```sql
-- DOWN Migration: add_user_age
ALTER TABLE users DROP COLUMN age;
```

You can hand-edit these files for data migrations or complex schema changes. The engine will not overwrite an existing migration with the same name.

## Migration directory

Default location is `migrations/` relative to the working directory. Override with `$BRIDGE_MIGRATIONS_DIR` environment variable or the `migrations_dir` constructor parameter.

## Dialect awareness

The `dialect` parameter affects SQL generation:

- `sqlite`: `ALTER TABLE ... RENAME COLUMN` (3.25+), `DROP COLUMN` (3.35+)
- `postgres`: `ALTER TABLE ... ALTER COLUMN ... TYPE`
- `mysql` / `mssql`: dialect-specific syntax for `RENAME COLUMN` and `ALTER COLUMN`

## Limitations

- **ALTER COLUMN nullability**: Changing `NOT NULL` → `NULL` or vice versa requires dialect-specific syntax. Bridge generates `SET NOT NULL` / `DROP NOT NULL` for PostgreSQL, but SQLite does not natively support this operation — you may need a manual migration.
- **Data migrations**: The engine handles schema-only changes. For data transformations (backfill, reformat), hand-edit the generated SQL.
