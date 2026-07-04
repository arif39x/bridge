import argparse
import asyncio
import os

from .. import connect, execute_raw
from ..schema import MIGRATIONS_DIR, MigrationEngine, reflect_table


async def main():
    parser = argparse.ArgumentParser(description="Bridge CLI")
    subparsers = parser.add_subparsers(dest="command")

    # Reflect Command
    reflect_parser = subparsers.add_parser("reflect", help="Reflect a database table")
    reflect_parser.add_argument("--url", required=True, help="Database URL")
    reflect_parser.add_argument("--table", required=True, help="Table name to reflect")
    reflect_parser.add_argument("--output", help="Output file path")

    # Makemigrations Command
    mm_parser = subparsers.add_parser(
        "makemigrations", help="Generate SQL migrations from models"
    )
    mm_parser.add_argument(
        "--dialect",
        default="sqlite",
        choices=["sqlite", "postgres"],
        help="Database dialect",
    )
    mm_parser.add_argument("--name", default="auto", help="Migration name")

    # Migrate Command
    migrate_parser = subparsers.add_parser("migrate", help="Apply pending migrations")
    migrate_parser.add_argument("--url", required=True, help="Database URL")

    args = parser.parse_args()

    if args.command == "reflect":
        await connect(args.url)
        src = await reflect_table(args.table)
        if args.output:
            with open(args.output, "w") as f:
                f.write(src)
        else:
            print(src)

    elif args.command == "makemigrations":
        # Import models from current directory to register them
        import sys

        import import_module_from_path  # Helper if needed, but for now assume models are in models.py

        try:
            # Try to discover models in common locations
            if os.path.exists("models.py"):
                import models
            elif os.path.exists("bridge/models.py"):
                from . import models
        except (ImportError, SyntaxError, FileNotFoundError) as e:
            print(f"Warning: Failed to auto-discover models: {e}")

        engine = MigrationEngine(dialect=args.dialect)
        engine.generate_migration(args.name)

    elif args.command == "migrate":
        await connect(args.url)

        # Create migrations table if not exists
        await execute_raw("""
            CREATE TABLE IF NOT EXISTS _bridge_migrations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE,
                applied_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )
        """)

        # Get applied migrations

        files = sorted([f for f in os.listdir(MIGRATIONS_DIR) if f.endswith(".sql")])
        for f in files:
            try:
                with open(os.path.join(MIGRATIONS_DIR, f), "r") as sql_file:
                    sql = sql_file.read()
                    print(f"Applying {f}...")
                    await execute_raw(sql)
                    escaped = f.replace("'", "''")
                    await execute_raw(
                        f"INSERT INTO _bridge_migrations (name) VALUES ('{escaped}')"
                    )
            except (OSError, RuntimeError, ValueError, KeyError) as e:
                if "UNIQUE constraint failed" in str(e):
                    continue  # Already applied
                print(f"Error applying {f}: {e}")
                break


if __name__ == "__main__":
    asyncio.run(main())
