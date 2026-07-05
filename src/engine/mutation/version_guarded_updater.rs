use crate::engine::db::{bind_query_value, Dialect, SqlDialect};
use crate::engine::identity_map::shared_identity_map::SharedIdentityMap;
use crate::engine::query::QueryValue;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::instrument;

const VERSION_COLUMN_NAME: &str = "_bridge_row_version";

pub struct VersionGuardedUpdater<'dialect> {
    dialect: &'dialect SqlDialect,
    identity_map: SharedIdentityMap,
    pool: sqlx::AnyPool,
    tx: Option<Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
}

impl<'dialect> VersionGuardedUpdater<'dialect> {
    pub fn new(
        dialect: &'dialect SqlDialect,
        identity_map: SharedIdentityMap,
        pool: sqlx::AnyPool,
        tx: Option<Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    ) -> Self {
        Self {
            dialect,
            identity_map,
            pool,
            tx,
        }
    }

    #[instrument(
        name = "version_guarded_updater.update",
        fields(table = %table_name, pk = %primary_key_value, known_version = %known_version),
        skip(self, column_value_pairs)
    )]
    pub async fn update_with_version_guard(
        &self,
        table_name: &str,
        primary_key_column: &str,
        primary_key_value: &str,
        known_version: u64,
        column_value_pairs: Vec<(String, serde_json::Value)>,
    ) -> Result<(), VersionGuardedUpdateError> {
        let next_version = known_version + 1;

        let converted: Vec<(String, QueryValue)> = column_value_pairs
            .into_iter()
            .map(|(col, val)| (col, json_to_query_value(val)))
            .collect();

        let dialect = self.dialect.to_dialect();
        let (sql, values) = dialect
            .build_version_guarded_update(
                table_name,
                primary_key_column,
                primary_key_value,
                VERSION_COLUMN_NAME,
                known_version,
                next_version,
                &converted,
            )
            .map_err(|e| VersionGuardedUpdateError::DialectQueryBuildFailure {
                reason: e.to_string(),
            })?;

        let affected_row_count = self.execute_update(&sql, &values).await?;

        if affected_row_count == 0 {
            self.identity_map.evict(table_name, primary_key_value);

            return Err(VersionGuardedUpdateError::ConcurrentUpdateDetected {
                table: table_name.to_owned(),
                primary_key_value: primary_key_value.to_owned(),
                stale_version: known_version,
            });
        }

        Ok(())
    }

    async fn execute_update(
        &self,
        sql: &str,
        values: &[QueryValue],
    ) -> Result<u64, VersionGuardedUpdateError> {
        let mut query = sqlx::query(sql);
        for val in values {
            query = bind_query_value(query, val)?;
        }

        let result = if let Some(tx_mutex) = &self.tx {
            let mut tx_guard = tx_mutex.lock().await;
            let tx_conn = tx_guard.as_mut().ok_or_else(|| {
                VersionGuardedUpdateError::DatabaseExecutionFailure {
                    reason: "Transaction already closed".to_string(),
                }
            })?;
            query.execute(&mut **tx_conn).await.map_err(|e| {
                VersionGuardedUpdateError::DatabaseExecutionFailure {
                    reason: e.to_string(),
                }
            })?
        } else {
            query.execute(&self.pool).await.map_err(|e| {
                VersionGuardedUpdateError::DatabaseExecutionFailure {
                    reason: e.to_string(),
                }
            })?
        };

        Ok(result.rows_affected())
    }
}

fn json_to_query_value(value: serde_json::Value) -> QueryValue {
    match value {
        serde_json::Value::String(s) => QueryValue::String(s),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                QueryValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                QueryValue::Float(f)
            } else {
                QueryValue::Null
            }
        }
        serde_json::Value::Bool(b) => QueryValue::Bool(b),
        serde_json::Value::Null => QueryValue::Null,
        other => QueryValue::Json(other),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VersionGuardedUpdateError {
    #[error(
        "Concurrent update detected on table '{table}' row '{primary_key_value}'. \
         Your local version ({stale_version}) is outdated. Re-fetch and retry."
    )]
    ConcurrentUpdateDetected {
        table: String,
        primary_key_value: String,
        stale_version: u64,
    },

    #[error("Dialect failed to build version-guarded UPDATE: {reason}")]
    DialectQueryBuildFailure { reason: String },

    #[error("Database execution failed: {reason}")]
    DatabaseExecutionFailure { reason: String },
}

impl From<crate::error::BridgeError> for VersionGuardedUpdateError {
    fn from(e: crate::error::BridgeError) -> Self {
        VersionGuardedUpdateError::DatabaseExecutionFailure { reason: e.to_string() }
    }
}
