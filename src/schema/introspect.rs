use crate::engine::db::SqlDialect;
use crate::error::BridgeResult;
use serde::{Deserialize, Serialize};
use sqlx::{AnyPool, Row};

/// Metadata for a database column.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary_key: bool,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexMeta {
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableMeta {
    pub name: String,
    pub columns: Vec<ColumnMeta>,
    pub indexes: Vec<IndexMeta>,
}

/// Reflects the entire schema of the database.
#[must_use]
pub async fn reflect_schema(pool: &AnyPool, url: &str) -> BridgeResult<Vec<TableMeta>> {
    let dialect = SqlDialect::from_url(url);

    // Get list of tables
    let tables: Vec<String> = match dialect {
        SqlDialect::Sqlite => sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| r.get(0))
        .collect(),
        SqlDialect::Postgres => sqlx::query(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'",
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| r.get(0))
        .collect(),
        _ => sqlx::query(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = DATABASE()",
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| r.get(0))
        .collect(),
    };

    let mut schema = Vec::new();
    for table_name in tables {
        let columns = reflect_table(pool, url, &table_name).await?;
        // For indexes, need dialect-specific logic here too.
        schema.push(TableMeta {
            name: table_name,
            columns,
            indexes: Vec::new(),
        });
    }
    Ok(schema)
}

/// Reflects table metadata based on the database dialect.
#[must_use]
pub async fn reflect_table(
    pool: &AnyPool,
    url: &str,
    table_name: &str,
) -> BridgeResult<Vec<ColumnMeta>> {
    let dialect = SqlDialect::from_url(url);

    match dialect {
        SqlDialect::Sqlite => reflect_sqlite(pool, table_name).await,
        _ => reflect_information_schema(pool, table_name).await,
    }
}

/// Introspection for PostgreSQL, MySQL, and MS SQL Server using standard Information Schema.
#[must_use]
async fn reflect_information_schema(
    pool: &AnyPool,
    table_name: &str,
) -> BridgeResult<Vec<ColumnMeta>> {
    let rows = sqlx::query(
        "SELECT column_name, data_type, is_nullable, column_default
         FROM information_schema.columns
         WHERE table_name = $1",
    )
    .bind(table_name)
    .fetch_all(pool)
    .await?;

    let mut columns = Vec::new();
    for row in rows {
        columns.push(ColumnMeta {
            name: row.get("column_name"),
            data_type: row.get("data_type"),
            is_nullable: row.get::<String, _>("is_nullable") == "YES",
            is_primary_key: false,
            default_value: row.try_get("column_default").ok(),
        });
    }
    Ok(columns)
}

/// Specialized introspection for SQLite.
#[must_use]
async fn reflect_sqlite(pool: &AnyPool, table_name: &str) -> BridgeResult<Vec<ColumnMeta>> {
    let sql = format!("PRAGMA table_info({})", table_name);
    let rows = sqlx::query(&sql).fetch_all(pool).await?;

    let mut columns = Vec::new();
    for row in rows {
        columns.push(ColumnMeta {
            name: row.get("name"),
            data_type: row.get("type"),
            is_nullable: row.get::<i32, _>("notnull") == 0,
            is_primary_key: row.get::<i32, _>("pk") == 1,
            default_value: row.try_get("dflt_value").ok(),
        });
    }
    Ok(columns)
}
