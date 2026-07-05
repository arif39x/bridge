use crate::engine::db::{bind_query_value, SqlDialect};
use crate::engine::query::QueryValue;
use sqlx::any::AnyRow;
use sqlx::{Column, Row};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::instrument;

pub struct BatchRelationLoader<'dialect> {
    dialect: &'dialect SqlDialect,
}

impl<'dialect> BatchRelationLoader<'dialect> {
    pub fn new(dialect: &'dialect SqlDialect) -> Self {
        Self { dialect }
    }

    /// Loads one-to-many relations for a collection of parent IDs using
    /// SELECT * FROM child_table WHERE foreign_key_column IN (...).
    #[instrument(
        name = "batch_loader.load_to_many_relations",
        fields(
            parent_table  = %parent_table,
            related_table = %related_table,
            parent_id_count = parent_ids.len()
        ),
        skip(self, parent_ids, pool, tx)
    )]
    pub async fn load_to_many_relations(
        &self,
        parent_table: &str,
        related_table: &str,
        foreign_key_column: &str,
        parent_ids: &[String],
        pool: &sqlx::AnyPool,
        tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    ) -> Result<HashMap<String, Vec<serde_json::Value>>, BatchLoaderError> {
        if parent_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let dialect = self.dialect.to_dialect();
        let (sql, _) = dialect.build_select_in(related_table, foreign_key_column, parent_ids.len())
            .map_err(|e| BatchLoaderError::DialectQueryBuildFailure {
                reason: e.to_string(),
            })?;

        let values: Vec<QueryValue> = parent_ids
            .iter()
            .map(|id| QueryValue::String(id.clone()))
            .collect();

        let raw_rows = self.execute_read_query(&sql, &values, pool, tx).await?;

        Ok(self.group_rows_by_foreign_key(raw_rows, foreign_key_column))
    }

    /// Loads many-to-many relations for a collection of parent IDs using
    /// SELECT t.*, j.left_key AS __bridge_left_id FROM target t
    /// JOIN junction j ON t.id = j.right_key WHERE j.left_key IN (...).
    #[instrument(
        name = "batch_loader.load_many_to_many_relations",
        fields(
            parent_id_count = parent_ids.len()
        ),
        skip(self, parent_ids, pool, tx)
    )]
    pub async fn load_many_to_many_relations(
        &self,
        target_table: &str,
        junction_table: &str,
        left_key: &str,
        right_key: &str,
        parent_ids: &[String],
        pool: &sqlx::AnyPool,
        tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    ) -> Result<HashMap<String, Vec<serde_json::Value>>, BatchLoaderError> {
        if parent_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let dialect = self.dialect.to_dialect();
        let (sql, _) = dialect
            .build_many_to_many_select_in(
                target_table,
                junction_table,
                left_key,
                right_key,
                parent_ids.len(),
            )
            .map_err(|e| BatchLoaderError::DialectQueryBuildFailure {
                reason: e.to_string(),
            })?;

        let values: Vec<QueryValue> = parent_ids
            .iter()
            .map(|id| QueryValue::String(id.clone()))
            .collect();

        let raw_rows = self.execute_read_query(&sql, &values, pool, tx).await?;

        Ok(self.group_rows_by_foreign_key(raw_rows, "__bridge_left_id"))
    }

    /// Groups a flat list of rows into a map keyed by the foreign key value.
    /// WHY: Kept as a separate method so it can be unit-tested without any
    /// database involvement — pure data transformation, no side effects.
    pub fn group_rows_by_foreign_key(
        &self,
        rows: Vec<serde_json::Value>,
        foreign_key_column: &str,
    ) -> HashMap<String, Vec<serde_json::Value>> {
        let mut grouped: HashMap<String, Vec<serde_json::Value>> = HashMap::new();

        for row in rows {
            if let Some(fk_value) = row.get(foreign_key_column).and_then(|v| v.as_str()) {
                grouped.entry(fk_value.to_owned()).or_default().push(row);
            }
        }

        grouped
    }

    /// Executes a parameterized read query through the dialect system and
    /// sqlx, returning the result rows as JSON values.
    pub async fn execute_read_query(
        &self,
        sql: &str,
        values: &[QueryValue],
        pool: &sqlx::AnyPool,
        tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    ) -> Result<Vec<serde_json::Value>, BatchLoaderError> {
        let mut query = sqlx::query(sql);
        for val in values {
            query = bind_query_value(query, val)?;
        }

        let rows: Vec<AnyRow> = if let Some(tx_mutex) = tx {
            let mut tx_guard = tx_mutex.lock().await;
            let tx_conn = tx_guard.as_mut().ok_or_else(|| {
                BatchLoaderError::DatabaseExecutionFailure {
                    reason: "Transaction already closed".to_string(),
                }
            })?;
            query.fetch_all(&mut **tx_conn).await.map_err(|e| {
                BatchLoaderError::DatabaseExecutionFailure {
                    reason: e.to_string(),
                }
            })?
        } else {
            query.fetch_all(pool).await.map_err(|e| {
                BatchLoaderError::DatabaseExecutionFailure {
                    reason: e.to_string(),
                }
            })?
        };

        Ok(rows.iter().map(any_row_to_json).collect())
    }
}

/// Converts an sqlx AnyRow to a serde_json::Value by iterating over columns
/// and attempting type-appropriate extraction.
fn any_row_to_json(row: &AnyRow) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for column in row.columns() {
        let name = column.name();
        let value = if let Ok(v) = row.try_get::<String, _>(name) {
            serde_json::Value::String(v)
        } else if let Ok(v) = row.try_get::<i64, _>(name) {
            serde_json::json!(v)
        } else if let Ok(v) = row.try_get::<f64, _>(name) {
            serde_json::json!(v)
        } else if let Ok(v) = row.try_get::<bool, _>(name) {
            serde_json::json!(v)
        } else {
            serde_json::Value::Null
        };
        map.insert(name.to_string(), value);
    }
    serde_json::Value::Object(map)
}

#[derive(Debug, thiserror::Error)]
pub enum BatchLoaderError {
    #[error("Dialect failed to build SELECT IN query: {reason}")]
    DialectQueryBuildFailure { reason: String },

    #[error("Database execution failed: {reason}")]
    DatabaseExecutionFailure { reason: String },
}

impl From<crate::error::BridgeError> for BatchLoaderError {
    fn from(e: crate::error::BridgeError) -> Self {
        BatchLoaderError::DatabaseExecutionFailure { reason: e.to_string() }
    }
}
