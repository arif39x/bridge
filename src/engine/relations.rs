use crate::engine::db::{generic_select_in, validate_identifier, SqlDialect};
use crate::error::{BridgeError, BridgeResult, DiagnosticInfo};
use sqlx::{any::AnyRow, Row, AnyPool};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn fetch_one_to_many(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    child_table: &str,
    foreign_key: &str,
    parent_id: &str,
) -> BridgeResult<Vec<AnyRow>> {
    validate_identifier(child_table)?;
    validate_identifier(foreign_key)?;

    let dialect = SqlDialect::from_url(url).to_dialect();
    let sql = format!(
        "SELECT * FROM {} WHERE {} = {}",
        child_table,
        foreign_key,
        dialect.get_placeholder(0)
    );

    let mut query = sqlx::query(&sql).bind(parent_id);

    let rows = if let Some(tx_mutex) = tx {
        let mut tx_guard = tx_mutex.lock().await;
        let tx_conn = tx_guard.as_mut().ok_or_else(|| {
            BridgeError::Validation(
                "Transaction already closed".to_string(),
                DiagnosticInfo::default(),
            )
        })?;
        query.fetch_all(&mut **tx_conn).await?
    } else {
        query.fetch_all(pool).await?
    };

    Ok(rows)
}

pub async fn fetch_many_to_many(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    target_table: &str,
    junction_table: &str,
    left_key: &str,
    right_key: &str,
    parent_id: &str,
) -> BridgeResult<Vec<AnyRow>> {
    validate_identifier(target_table)?;
    validate_identifier(junction_table)?;
    validate_identifier(left_key)?;
    validate_identifier(right_key)?;

    let dialect = SqlDialect::from_url(url).to_dialect();
    let sql = format!(
        "SELECT t.* FROM {} t
         JOIN {} j ON t.id = j.{}
         WHERE j.{} = {}",
        target_table,
        junction_table,
        right_key,
        left_key,
        dialect.get_placeholder(0)
    );

    let mut query = sqlx::query(&sql).bind(parent_id);

    let rows = if let Some(tx_mutex) = tx {
        let mut tx_guard = tx_mutex.lock().await;
        let tx_conn = tx_guard.as_mut().ok_or_else(|| {
            BridgeError::Validation(
                "Transaction already closed".to_string(),
                DiagnosticInfo::default(),
            )
        })?;
        query.fetch_all(&mut **tx_conn).await?
    } else {
        query.fetch_all(pool).await?
    };

    Ok(rows)
}

pub async fn fetch_self_ref(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    table: &str,
    parent_key: &str,
    parent_id: &str,
) -> BridgeResult<Vec<AnyRow>> {
    validate_identifier(table)?;
    validate_identifier(parent_key)?;

    let dialect = SqlDialect::from_url(url).to_dialect();
    let sql = format!(
        "SELECT * FROM {} WHERE {} = {}",
        table,
        parent_key,
        dialect.get_placeholder(0)
    );

    let mut query = sqlx::query(&sql).bind(parent_id);

    let rows = if let Some(tx_mutex) = tx {
        let mut tx_guard = tx_mutex.lock().await;
        let tx_conn = tx_guard.as_mut().ok_or_else(|| {
            BridgeError::Validation(
                "Transaction already closed".to_string(),
                DiagnosticInfo::default(),
            )
        })?;
        query.fetch_all(&mut **tx_conn).await?
    } else {
        query.fetch_all(pool).await?
    };

    Ok(rows)
}

/// Batch-fetches one-to-many relations for multiple parent IDs using SELECT IN.
/// Returns a map of parent_id -> rows for the given foreign_key.
pub async fn batch_fetch_one_to_many(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    child_table: &str,
    foreign_key: &str,
    parent_ids: &[String],
) -> BridgeResult<HashMap<String, Vec<AnyRow>>> {
    let rows = generic_select_in(pool, tx, url, child_table, foreign_key, parent_ids).await?;
    let mut grouped: HashMap<String, Vec<AnyRow>> = HashMap::new();
    for row in &rows {
        if let Ok(fk) = row.try_get::<String, _>(foreign_key) {
            grouped.entry(fk).or_default().push(row.clone());
        }
    }
    Ok(grouped)
}

/// Batch-fetches many-to-many relations for multiple parent IDs.
/// Returns a map of left_key value -> target rows.
pub async fn batch_fetch_many_to_many(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    target_table: &str,
    junction_table: &str,
    left_key: &str,
    right_key: &str,
    parent_ids: &[String],
) -> BridgeResult<HashMap<String, Vec<AnyRow>>> {
    if parent_ids.is_empty() {
        return Ok(HashMap::new());
    }
    validate_identifier(target_table)?;
    validate_identifier(junction_table)?;
    validate_identifier(left_key)?;
    validate_identifier(right_key)?;
    let dialect = SqlDialect::from_url(url).to_dialect();
    let placeholders: Vec<String> = (0..parent_ids.len())
        .map(|i| dialect.get_placeholder(i))
        .collect();
    let sql = format!(
        "SELECT t.*, j.{} AS __bridge_left_id FROM {} t JOIN {} j ON t.id = j.{} WHERE j.{} IN ({})",
        left_key,
        target_table,
        junction_table,
        right_key,
        left_key,
        placeholders.join(", ")
    );
    let mut query = sqlx::query(&sql);
    for id in parent_ids {
        query = query.bind(id);
    }
    let rows = if let Some(tx_mutex) = tx {
        let mut tx_guard = tx_mutex.lock().await;
        let tx_conn = tx_guard.as_mut().ok_or_else(|| {
            BridgeError::Validation(
                "Transaction already closed".to_string(),
                DiagnosticInfo::default(),
            )
        })?;
        query.fetch_all(&mut **tx_conn).await?
    } else {
        query.fetch_all(pool).await?
    };
    let mut grouped: HashMap<String, Vec<AnyRow>> = HashMap::new();
    for row in &rows {
        if let Ok(left_id) = row.try_get::<String, _>("__bridge_left_id") {
            grouped.entry(left_id).or_default().push(row.clone());
        }
    }
    Ok(grouped)
}

/// Batch-fetches self-referential relations for multiple parent IDs.
/// Returns a map of parent_id -> child rows.
pub async fn batch_fetch_self_ref(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    table: &str,
    parent_key: &str,
    parent_ids: &[String],
) -> BridgeResult<HashMap<String, Vec<AnyRow>>> {
    let rows = generic_select_in(pool, tx, url, table, parent_key, parent_ids).await?;
    let mut grouped: HashMap<String, Vec<AnyRow>> = HashMap::new();
    for row in &rows {
        if let Ok(pk) = row.try_get::<String, _>(parent_key) {
            grouped.entry(pk).or_default().push(row.clone());
        }
    }
    Ok(grouped)
}
