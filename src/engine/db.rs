use crate::engine::query::QueryValue;
use crate::error::{BridgeError, BridgeResult, DiagnosticInfo};
use crate::telemetry::logger::{self, TelemetryEvent};
use futures::stream::BoxStream;
use futures::StreamExt;
use once_cell::sync::Lazy;
use regex::Regex;
use sqlx::any::{AnyConnectOptions, AnyRow};
use sqlx::AnyPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// Constants for SQL validation and formatting.
const VALID_IDENTIFIER_REGEX: &str = r"^[a-zA-Z_][a-zA-Z0-9_]*$";
/// SQL keywords prohibited as identifiers. The regex `VALID_IDENTIFIER_REGEX`
/// already prevents dangerous characters (`;`, `--`, `'`), so this is defense-in-depth.
const RESERVED_KEYWORDS: [&str; 18] = [
    "SELECT", "DROP", "TABLE", "DELETE", "UPDATE", "INSERT", "TRUNCATE", "ALTER",
    "CREATE", "EXEC", "EXECUTE", "UNION", "ALL", "INTO", "FROM", "WHERE",
    "GRANT", "REVOKE",
];

pub static VALID_SQL_IDENTIFIER_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(VALID_IDENTIFIER_REGEX).expect("Invalid hardcoded regex pattern"));

pub static CIRCUIT_BREAKER: Lazy<crate::engine::circuit_breaker::CircuitBreaker> =
    Lazy::new(|| {
        crate::engine::circuit_breaker::CircuitBreaker::new(5, std::time::Duration::from_secs(30))
    });

/// Represents supported SQL dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SqlDialect {
    Postgres,
    Sqlite,
    MySql,
    MsSql,
    Oracle,
    CockroachDb,
    MariaDb,
    PlanetScale,
    Neon,
    YugabyteDb,
    CloudflareD1,
    Dolt,
}

pub trait Dialect: Send + Sync {
    fn get_placeholder(&self, index: usize) -> String;
    fn quote_identifier(&self, identifier: &str) -> String {
        format!("\"{}\"", identifier)
    }
    fn build_select_in(
        &self,
        table: &str,
        column: &str,
        id_count: usize,
    ) -> BridgeResult<(String, Vec<QueryValue>)> {
        validate_identifier(table)?;
        validate_identifier(column)?;
        let placeholders: Vec<String> = (0..id_count)
            .map(|i| self.get_placeholder(i))
            .collect();
        let sql = format!(
            "SELECT * FROM {} WHERE {} IN ({})",
            self.quote_identifier(table),
            self.quote_identifier(column),
            placeholders.join(", ")
        );
        Ok((sql, Vec::new()))
    }
    fn build_many_to_many_select_in(
        &self,
        target_table: &str,
        junction_table: &str,
        left_key: &str,
        right_key: &str,
        id_count: usize,
    ) -> BridgeResult<(String, Vec<QueryValue>)> {
        validate_identifier(target_table)?;
        validate_identifier(junction_table)?;
        validate_identifier(left_key)?;
        validate_identifier(right_key)?;
        let placeholders: Vec<String> = (0..id_count)
            .map(|i| self.get_placeholder(i))
            .collect();
        let sql = format!(
            "SELECT t.*, j.{left_key} AS __bridge_left_id FROM {target} t \
             JOIN {junction} j ON t.id = j.{right_key} \
             WHERE j.{left_key} IN ({placeholders})",
            target = self.quote_identifier(target_table),
            junction = self.quote_identifier(junction_table),
            left_key = self.quote_identifier(left_key),
            right_key = self.quote_identifier(right_key),
            placeholders = placeholders.join(", "),
        );
        Ok((sql, Vec::new()))
    }
    fn build_select(
        &self,
        table: &str,
        columns: &[String],
        filters: &[(String, QueryValue)],
        limit: Option<i64>,
    ) -> BridgeResult<(String, Vec<QueryValue>)> {
        validate_identifier(table)?;
        let cols = if columns.is_empty() {
            "*".to_string()
        } else {
            let mut quoted = Vec::with_capacity(columns.len());
            for c in columns {
                validate_identifier(c)?;
                quoted.push(self.quote_identifier(c));
            }
            quoted.join(", ")
        };
        let mut sql = format!("SELECT {} FROM {}", cols, self.quote_identifier(table));
        let mut values = Vec::new();

        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            let mut conditions = Vec::new();
            for (col, val) in filters {
                validate_identifier(col)?;
                match val {
                    #[cfg(feature = "allow-raw-sql")]
                    QueryValue::Raw(_) => {
                        return Err(BridgeError::Validation(
                            "Raw SQL expressions are not allowed in WHERE clauses. Use `raw_filter()` for explicit opt-in.".to_string(),
                            DiagnosticInfo::default(),
                        ));
                    }
                    _ => {
                        conditions.push(format!(
                            "{} = {}",
                            self.quote_identifier(col),
                            self.get_placeholder(values.len())
                        ));
                        values.push(val.clone());
                    }
                }
            }
            sql.push_str(&conditions.join(" AND "));
        }

        if let Some(l) = limit {
            sql.push_str(&format!(" LIMIT {}", l));
        }

        Ok((sql, values))
    }
}

pub struct SqliteDialect;
impl Dialect for SqliteDialect {
    fn get_placeholder(&self, index: usize) -> String {
        format!("${}", index + 1)
    }
}

pub struct PostgreSqlDialect;
impl Dialect for PostgreSqlDialect {
    fn get_placeholder(&self, index: usize) -> String {
        format!("${}", index + 1)
    }
}

pub struct MySqlDialect;
impl Dialect for MySqlDialect {
    fn get_placeholder(&self, index: usize) -> String {
        "?".to_string()
    }
    fn quote_identifier(&self, identifier: &str) -> String {
        format!("`{}`", identifier)
    }
}

pub struct MsSqlDialect;
impl Dialect for MsSqlDialect {
    fn get_placeholder(&self, index: usize) -> String {
        format!("@p{}", index + 1)
    }
    fn quote_identifier(&self, identifier: &str) -> String {
        format!("[{}]", identifier)
    }
}

pub struct OracleDialect;
impl Dialect for OracleDialect {
    fn get_placeholder(&self, index: usize) -> String {
        format!(":{}", index + 1)
    }
    fn build_select(
        &self,
        table: &str,
        columns: &[String],
        filters: &[(String, QueryValue)],
        limit: Option<i64>,
    ) -> BridgeResult<(String, Vec<QueryValue>)> {
        validate_identifier(table)?;
        let cols = if columns.is_empty() {
            "*".to_string()
        } else {
            let mut quoted = Vec::with_capacity(columns.len());
            for c in columns {
                validate_identifier(c)?;
                quoted.push(self.quote_identifier(c));
            }
            quoted.join(", ")
        };
        let mut sql = format!("SELECT {} FROM {}", cols, self.quote_identifier(table));
        let mut values = Vec::new();

        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            let mut conditions = Vec::new();
            for (col, val) in filters {
                validate_identifier(col)?;
                match val {
                    #[cfg(feature = "allow-raw-sql")]
                    QueryValue::Raw(_) => {
                        return Err(BridgeError::Validation(
                            "Raw SQL expressions are not allowed in WHERE clauses. Use `raw_filter()` for explicit opt-in.".to_string(),
                            DiagnosticInfo::default(),
                        ));
                    }
                    _ => {
                        conditions.push(format!(
                            "{} = {}",
                            self.quote_identifier(col),
                            self.get_placeholder(values.len())
                        ));
                        values.push(val.clone());
                    }
                }
            }
            sql.push_str(&conditions.join(" AND "));
        }

        if let Some(l) = limit {
            // Oracle uses OFFSET/FETCH for modern pagination
            sql.push_str(&format!(" FETCH NEXT {} ROWS ONLY", l));
        }

        Ok((sql, values))
    }
}

impl SqlDialect {
    /// Infers the SQL dialect from the connection URL.
    #[must_use]
    pub fn from_url(url: &str) -> Self {
        let url = url.to_lowercase();
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            if url.contains("neon.tech") {
                return Self::Neon;
            }
            if url.contains("yugabyte") {
                return Self::YugabyteDb;
            }
            if url.contains("cockroach") || url.contains(":26257") {
                return Self::CockroachDb;
            }
            Self::Postgres
        } else if url.starts_with("sqlite:") || url.contains("d1.cloudflare") {
            if url.contains("d1.cloudflare") {
                return Self::CloudflareD1;
            }
            Self::Sqlite
        } else if url.starts_with("mysql://") || url.starts_with("mariadb://") {
            if url.contains("mariadb") {
                return Self::MariaDb;
            }
            if url.contains("psdb.cloud") {
                return Self::PlanetScale;
            }
            if url.contains("dolt") {
                return Self::Dolt;
            }
            Self::MySql
        } else if url.starts_with("mssql://") || url.starts_with("sqlserver://") {
            Self::MsSql
        } else if url.starts_with("oracle://") || url.starts_with("thin://") {
            Self::Oracle
        } else {
            Self::Postgres
        }
    }

    pub fn to_dialect(&self) -> Box<dyn Dialect> {
        match self {
            Self::Postgres | Self::CockroachDb | Self::Neon | Self::YugabyteDb => {
                Box::new(PostgreSqlDialect)
            }
            Self::Sqlite | Self::CloudflareD1 => Box::new(SqliteDialect),
            Self::MySql | Self::MariaDb | Self::PlanetScale | Self::Dolt => Box::new(MySqlDialect),
            Self::MsSql => Box::new(MsSqlDialect),
            Self::Oracle => Box::new(OracleDialect),
        }
    }
}

/// Validates that a string is a safe SQL identifier.
#[must_use]
pub fn validate_identifier(identifier: &str) -> BridgeResult<()> {
    if !VALID_SQL_IDENTIFIER_PATTERN.is_match(identifier) {
        return Err(BridgeError::Validation(
            format!(
                "Security Violation: Invalid SQL identifier '{}'",
                identifier
            ),
            DiagnosticInfo::default(),
        ));
    }

    if RESERVED_KEYWORDS.contains(&identifier.to_uppercase().as_str()) {
        return Err(BridgeError::Validation(
            format!(
                "Security Violation: Reserved keyword '{}' used as identifier",
                identifier
            ),
            DiagnosticInfo::default(),
        ));
    }
    Ok(())
}

/// Defense-in-depth: validates that a `QueryValue` used as a filter value
/// does not contain obvious SQL injection patterns.
/// NOTE: Parameterized queries already prevent injection through values;
/// this is an extra safety net.
#[must_use]
pub fn validate_filter_value(value: &QueryValue) -> BridgeResult<()> {
    if let QueryValue::String(s) = value {
        let lower = s.to_lowercase();
        if lower.contains("';") || lower.contains("--") || lower.contains("/*") {
            return Err(BridgeError::Validation(
                format!(
                    "Security Violation: Filter value contains suspicious SQL pattern",
                ),
                DiagnosticInfo::default(),
            ));
        }
    }
    Ok(())
}

/// Runtime schema validation: checks that filter column names exist in the
/// registered metadata and that `QueryValue` types are compatible with the
/// column's declared data type.
///
/// Only active in debug builds (`#[cfg(debug_assertions)]`); compiled away in
/// release builds so there is zero production overhead.
#[must_use]
pub fn validate_query_filters(
    table_name: &str,
    filters: &[(String, QueryValue)],
) -> BridgeResult<()> {
    #[cfg(debug_assertions)]
    {
        use crate::engine::metadata::REGISTRY;

        let registry_guard = REGISTRY.read().unwrap();
        if let Some(mapping) = registry_guard.mappings.get(table_name) {
            for (col, val) in filters {
                let meta = mapping.columns.get(col).ok_or_else(|| {
                    BridgeError::Validation(
                        format!(
                            "Schema validation: column '{}' not found in table '{}'. \
                             Available columns: {:?}",
                            col,
                            table_name,
                            mapping.columns.keys().collect::<Vec<_>>(),
                        ),
                        DiagnosticInfo::default(),
                    )
                })?;

                if !query_value_type_matches(val, &meta.data_type) {
                    return Err(BridgeError::TypeMismatch {
                        field: format!("{}.{}", table_name, col),
                        expected: meta.data_type.clone(),
                        got: format!("{:?}", val),
                        info: DiagnosticInfo::default(),
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn query_value_type_matches(value: &QueryValue, data_type: &str) -> bool {
    use QueryValue::*;
    match value {
        String(_) => matches!(
            data_type.to_lowercase().as_str(),
            "text" | "str" | "varchar" | "string"
        ),
        Int(_) => matches!(
            data_type.to_lowercase().as_str(),
            "int" | "bigint" | "integer" | "smallint"
        ),
        Float(_) => matches!(
            data_type.to_lowercase().as_str(),
            "float" | "double" | "real" | "double precision"
        ),
        Bool(_) => matches!(
            data_type.to_lowercase().as_str(),
            "bool" | "boolean"
        ),
        Uuid(_) => data_type.to_lowercase() == "uuid",
        DateTime(_) => matches!(
            data_type.to_lowercase().as_str(),
            "datetime" | "timestamp" | "timestamptz"
        ),
        Json(_) => matches!(
            data_type.to_lowercase().as_str(),
            "json" | "jsonb"
        ),
        Bytes(_) => matches!(
            data_type.to_lowercase().as_str(),
            "bytes" | "blob" | "bytea"
        ),
        Null => true,
        #[cfg(feature = "allow-raw-sql")]
        Raw(_) => true,
    }
}

/// Establishes a connection pool using the provided URL and configuration.
/// Uses sqlx's built-in pool.
#[must_use]
pub async fn connect(
    url: &str,
    config: Option<crate::ffi::pool_config::PoolConfig>,
) -> BridgeResult<AnyPool> {
    sqlx::any::install_default_drivers();

    let mut options = url
        .parse::<AnyConnectOptions>()
        .map_err(BridgeError::from)?;

    let mut pool_builder = sqlx::any::AnyPoolOptions::new();

    if let Some(cfg) = config {
        pool_builder = pool_builder
            .max_connections(cfg.max_connections)
            .min_connections(cfg.min_connections)
            .acquire_timeout(std::time::Duration::from_secs(cfg.connect_timeout_sec));

        if let Some(idle) = cfg.idle_timeout_sec {
            pool_builder = pool_builder.idle_timeout(std::time::Duration::from_secs(idle));
        }
        if let Some(lifetime) = cfg.max_lifetime_sec {
            pool_builder = pool_builder.max_lifetime(std::time::Duration::from_secs(lifetime));
        }
    }

    pool_builder
        .connect_with(options)
        .await
        .map_err(BridgeError::from)
}

/// Shared logic for building placeholders and values for queries.
#[must_use]
fn prepare_statement(
    dialect: &dyn Dialect,
    data: &HashMap<String, QueryValue>,
) -> BridgeResult<(Vec<String>, Vec<QueryValue>, Vec<String>)> {
    let mut columns = Vec::new();
    let mut values = Vec::new();
    let mut placeholders = Vec::new();

    for (idx, (col, val)) in data.iter().enumerate() {
        validate_identifier(col)?;
        columns.push(col.clone());
        values.push(val.clone());
        placeholders.push(dialect.get_placeholder(idx));
    }
    Ok((columns, values, placeholders))
}

/// Helper to bind QueryValue to a query.
pub(crate) fn bind_query_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>>,
    value: &'q QueryValue,
) -> sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>> {
    match value {
        QueryValue::String(s) => query.bind(s),
        QueryValue::Int(i) => query.bind(i),
        QueryValue::Float(f) => query.bind(f),
        QueryValue::Bool(b) => query.bind(b),
        QueryValue::Uuid(u) => query.bind(u.to_string()),
        QueryValue::DateTime(dt) => query.bind(dt.to_rfc3339()),
        QueryValue::Json(j) => query.bind(j.to_string()),
        QueryValue::Bytes(b) => query.bind(b),
        #[cfg(feature = "allow-raw-sql")]
        QueryValue::Raw(_) => panic!("RawExpression should have been expanded before binding"),
        QueryValue::Null => query.bind(None::<String>),
    }
}

/// Pure Rust generic update.
#[must_use]
#[tracing::instrument(skip(pool, tx))]
pub async fn generic_update(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    table_name: &str,
    data: HashMap<String, QueryValue>,
    filters: HashMap<String, QueryValue>,
) -> BridgeResult<()> {
    validate_identifier(table_name)?;
    let dialect_type = SqlDialect::from_url(url);
    let dialect = dialect_type.to_dialect();

    if data.is_empty() {
        return Ok(());
    }

    let data_vec: Vec<(String, QueryValue)> = data.into_iter().collect();
    for (col, _) in &data_vec {
        validate_identifier(col)?;
    }
    validate_query_filters(table_name, &data_vec)?;

    let mut sql = format!("UPDATE {} SET ", dialect.quote_identifier(table_name));
    let mut values = Vec::new();
    let mut set_clauses = Vec::new();

    for (col, val) in data_vec {
        match val {
            #[cfg(feature = "allow-raw-sql")]
            QueryValue::Raw(_) => {
                return Err(BridgeError::Validation(
                    "Raw SQL expressions are not allowed in SET clauses. Use `execute_raw()` instead.".to_string(),
                    DiagnosticInfo::default(),
                ));
            }
            _ => {
                set_clauses.push(format!(
                    "{} = {}",
                    dialect.quote_identifier(&col),
                    dialect.get_placeholder(values.len())
                ));
                values.push(val);
            }
        }
    }
    sql.push_str(&set_clauses.join(", "));

    if !filters.is_empty() {
        let filters_vec: Vec<(String, QueryValue)> = filters.into_iter().collect();
        for (col, val) in &filters_vec {
            validate_identifier(col)?;
            validate_filter_value(val)?;
        }
        validate_query_filters(table_name, &filters_vec)?;

        sql.push_str(" WHERE ");
        let mut where_clauses = Vec::new();
        for (col, val) in filters_vec {
            match val {
                #[cfg(feature = "allow-raw-sql")]
                QueryValue::Raw(_) => {
                    return Err(BridgeError::Validation(
                        "Raw SQL expressions are not allowed in WHERE clauses. Use `raw_filter()` for explicit opt-in.".to_string(),
                        DiagnosticInfo::default(),
                    ));
                }
                _ => {
                    where_clauses.push(format!(
                        "{} = {}",
                        dialect.quote_identifier(&col),
                        dialect.get_placeholder(values.len())
                    ));
                    values.push(val);
                }
            }
        }
        sql.push_str(&where_clauses.join(" AND "));
    }

    let mut query = sqlx::query(&sql);
    for val in &values {
        query = bind_query_value(query, val);
    }

    if let Some(tx_mutex) = tx {
        let mut tx_guard = tx_mutex.lock().await;
        let tx_conn = tx_guard.as_mut().ok_or_else(|| {
            BridgeError::Validation(
                "Transaction already closed".to_string(),
                DiagnosticInfo::default(),
            )
        })?;
        query.execute(&mut **tx_conn).await.map_err(|e| {
            BridgeError::from(e)
                .with_sql(sql.clone(), None)
                .add_breadcrumb("generic_update")
        })?;
    } else {
        query.execute(pool).await.map_err(|e| {
            BridgeError::from(e)
                .with_sql(sql.clone(), None)
                .add_breadcrumb("generic_update")
        })?;
    }

    Ok(())
}

/// Pure Rust generic delete.
#[must_use]
#[tracing::instrument(skip(pool, tx))]
pub async fn generic_delete(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    table_name: &str,
    filters: HashMap<String, QueryValue>,
) -> BridgeResult<()> {
    validate_identifier(table_name)?;
    let dialect_type = SqlDialect::from_url(url);
    let dialect = dialect_type.to_dialect();

    let mut sql = format!("DELETE FROM {}", dialect.quote_identifier(table_name));
    let mut values = Vec::new();

    if !filters.is_empty() {
        let filters_vec: Vec<(String, QueryValue)> = filters.into_iter().collect();
        for (col, val) in &filters_vec {
            validate_identifier(col)?;
            validate_filter_value(val)?;
        }
        validate_query_filters(table_name, &filters_vec)?;

        sql.push_str(" WHERE ");
        let mut where_clauses = Vec::new();
        for (col, val) in filters_vec {
            match val {
                #[cfg(feature = "allow-raw-sql")]
                QueryValue::Raw(_) => {
                    return Err(BridgeError::Validation(
                        "Raw SQL expressions are not allowed in WHERE clauses. Use `raw_filter()` for explicit opt-in.".to_string(),
                        DiagnosticInfo::default(),
                    ));
                }
                _ => {
                    where_clauses.push(format!(
                        "{} = {}",
                        dialect.quote_identifier(&col),
                        dialect.get_placeholder(values.len())
                    ));
                    values.push(val);
                }
            }
        }
        sql.push_str(&where_clauses.join(" AND "));
    }

    let start = Instant::now();
    let mut query = sqlx::query(&sql);
    for val in &values {
        query = bind_query_value(query, val);
    }

    if let Some(tx_mutex) = tx {
        let mut tx_guard = tx_mutex.lock().await;
        let tx_conn = tx_guard.as_mut().ok_or_else(|| {
            BridgeError::Validation(
                "Transaction already closed".to_string(),
                DiagnosticInfo::default(),
            )
        })?;
        query.execute(&mut **tx_conn).await.map_err(|e| {
            BridgeError::from(e)
                .with_sql(sql.clone(), None)
                .add_breadcrumb("generic_delete")
        })?;
    } else {
        query.execute(pool).await.map_err(|e| {
            BridgeError::from(e)
                .with_sql(sql.clone(), None)
                .add_breadcrumb("generic_delete")
        })?;
    }

    let duration = start.elapsed();
    logger::emit_telemetry(TelemetryEvent {
        sql: sql.clone(),
        duration_micros: duration.as_micros() as u64,
        operation: "DELETE".to_string(),
        table: table_name.to_string(),
    });

    Ok(())
}

/// Pure Rust implementation of execute_raw.
#[must_use]
#[tracing::instrument(skip(pool))]
pub async fn execute_raw(pool: &AnyPool, sql: &str) -> BridgeResult<()> {
    sqlx::query(sql).execute(pool).await.map_err(|e| {
        BridgeError::from(e)
            .with_sql(sql.to_string(), None)
            .add_breadcrumb("execute_raw")
    })?;
    Ok(())
}

/// Pure Rust generic insert.
#[must_use]
#[tracing::instrument(skip(pool, tx))]
pub async fn generic_insert(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    table_name: &str,
    data: HashMap<String, QueryValue>,
) -> BridgeResult<HashMap<String, QueryValue>> {
    validate_identifier(table_name)?;
    let dialect_type = SqlDialect::from_url(url);
    let dialect = dialect_type.to_dialect();

    let data_vec: Vec<(String, QueryValue)> = data.into_iter().collect();
    for (col, _) in &data_vec {
        validate_identifier(col)?;
    }
    validate_query_filters(table_name, &data_vec)?;

    let mut columns = Vec::new();
    let mut values = Vec::new();
    let mut placeholders = Vec::new();

    for (col, val) in &data_vec {
        validate_identifier(col)?;
        columns.push(col.clone());
        match val {
            #[cfg(feature = "allow-raw-sql")]
            QueryValue::Raw(_) => {
                return Err(BridgeError::Validation(
                    "Raw SQL expressions are not allowed in VALUES clauses. Use `execute_raw()` instead.".to_string(),
                    DiagnosticInfo::default(),
                ));
            }
            _ => {
                placeholders.push(dialect.get_placeholder(values.len()));
                values.push(val.clone());
            }
        }
    }

    let quoted_cols: Vec<String> = columns.iter().map(|c| dialect.quote_identifier(c)).collect();
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        dialect.quote_identifier(table_name),
        quoted_cols.join(", "),
        placeholders.join(", ")
    );

    let start = Instant::now();
    let mut query = sqlx::query(&sql);
    for val in &values {
        query = bind_query_value(query, val);
    }

    if let Some(tx_mutex) = tx {
        let mut tx_guard = tx_mutex.lock().await;
        let tx_conn = tx_guard.as_mut().ok_or_else(|| {
            BridgeError::Validation(
                "Transaction already closed".to_string(),
                DiagnosticInfo::default(),
            )
        })?;
        query.execute(&mut **tx_conn).await.map_err(|e| {
            BridgeError::from(e)
                .with_sql(sql.clone(), None)
                .add_breadcrumb("generic_insert")
        })?;
    } else {
        query.execute(pool).await.map_err(|e| {
            BridgeError::from(e)
                .with_sql(sql.clone(), None)
                .add_breadcrumb("generic_insert")
        })?;
    }

    let duration = start.elapsed();
    logger::emit_telemetry(TelemetryEvent {
        sql: sql.clone(),
        duration_micros: duration.as_micros() as u64,
        operation: "INSERT".to_string(),
        table: table_name.to_string(),
    });

    Ok(data_vec.into_iter().collect())
}

/// Pure Rust generic bulk insert.
#[must_use]
#[tracing::instrument(skip(pool, tx))]
pub async fn generic_insert_bulk(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    table_name: &str,
    items: Vec<HashMap<String, QueryValue>>,
) -> BridgeResult<Vec<HashMap<String, QueryValue>>> {
    validate_identifier(table_name)?;
    let dialect_type = SqlDialect::from_url(url);
    let dialect = dialect_type.to_dialect();

    if items.is_empty() {
        return Ok(Vec::new());
    }

    // Assume all items have the same keys as the first item for bulk construction
    let first_item = &items[0];
    let (columns, _, _) = prepare_statement(dialect.as_ref(), first_item)?;

    for item in &items {
        let item_vec: Vec<(String, QueryValue)> = item.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        validate_query_filters(table_name, &item_vec)?;
    }

    let quoted_cols: Vec<String> = columns.iter().map(|c| dialect.quote_identifier(c)).collect();
    let mut sql = format!(
        "INSERT INTO {} ({}) VALUES ",
        dialect.quote_identifier(table_name),
        quoted_cols.join(", ")
    );

    let mut placeholders = Vec::new();
    let mut all_values = Vec::new();

    for item in items.iter() {
        let mut row_placeholders = Vec::new();
        for col in &columns {
            let val = item.get(col).cloned().unwrap_or(QueryValue::Null);
            match val {
                #[cfg(feature = "allow-raw-sql")]
                QueryValue::Raw(_) => {
                    return Err(BridgeError::Validation(
                        "Raw SQL expressions are not allowed in VALUES clauses. Use `execute_raw()` instead.".to_string(),
                        DiagnosticInfo::default(),
                    ));
                }
                _ => {
                    row_placeholders.push(dialect.get_placeholder(all_values.len()));
                    all_values.push(val);
                }
            }
        }
        placeholders.push(format!("({})", row_placeholders.join(", ")));
    }

    sql.push_str(&placeholders.join(", "));

    let start = Instant::now();
    let mut query = sqlx::query(&sql);
    for val in &all_values {
        query = bind_query_value(query, val);
    }

    if let Some(tx_mutex) = tx {
        let mut tx_guard = tx_mutex.lock().await;
        let tx_conn = tx_guard.as_mut().ok_or_else(|| {
            BridgeError::Validation(
                "Transaction already closed".to_string(),
                DiagnosticInfo::default(),
            )
        })?;
        query.execute(&mut **tx_conn).await.map_err(|e| {
            BridgeError::from(e)
                .with_sql(sql.clone(), None)
                .add_breadcrumb("generic_insert_bulk")
        })?;
    } else {
        query.execute(pool).await.map_err(|e| {
            BridgeError::from(e)
                .with_sql(sql.clone(), None)
                .add_breadcrumb("generic_insert_bulk")
        })?;
    }

    let duration = start.elapsed();
    logger::emit_telemetry(TelemetryEvent {
        sql: sql.clone(),
        duration_micros: duration.as_micros() as u64,
        operation: "BULK_INSERT".to_string(),
        table: table_name.to_string(),
    });

    Ok(items)
}

/// Pure Rust generic query.
#[must_use]
#[tracing::instrument(skip(pool, tx))]
pub async fn generic_query(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    table_name: &str,
    filters: HashMap<String, QueryValue>,
    limit: Option<i64>,
    fields: Option<Vec<String>>,
) -> BridgeResult<Vec<AnyRow>> {
    validate_identifier(table_name)?;
    let dialect_type = SqlDialect::from_url(url);
    let dialect = dialect_type.to_dialect();

    let columns = fields.unwrap_or_default();
    for col in &columns {
        validate_identifier(col)?;
    }

    let filter_vec: Vec<(String, QueryValue)> = filters.into_iter().collect();
    for (col, val) in &filter_vec {
        validate_identifier(col)?;
        validate_filter_value(val)?;
    }
    validate_query_filters(table_name, &filter_vec)?;

    let (sql, values) = dialect.build_select(table_name, &columns, &filter_vec, limit)?;

    CIRCUIT_BREAKER
        .call(|| async {
            let start = Instant::now();
            let mut query = sqlx::query(&sql);
            for val in &values {
                query = bind_query_value(query, val);
            }

            let rows = if let Some(tx_mutex) = tx {
                let mut tx_guard = tx_mutex.lock().await;
                let tx_conn = tx_guard.as_mut().ok_or_else(|| {
                    BridgeError::Validation(
                        "Transaction already closed".to_string(),
                        DiagnosticInfo::default(),
                    )
                })?;
                query.fetch_all(&mut **tx_conn).await.map_err(|e| {
                    BridgeError::from(e)
                        .with_sql(sql.clone(), None)
                        .add_breadcrumb("generic_query")
                })?
            } else {
                query.fetch_all(pool).await.map_err(|e| {
                    BridgeError::from(e)
                        .with_sql(sql.clone(), None)
                        .add_breadcrumb("generic_query")
                })?
            };

            let duration = start.elapsed();
            logger::emit_telemetry(TelemetryEvent {
                sql: sql.clone(),
                duration_micros: duration.as_micros() as u64,
                operation: "SELECT".to_string(),
                table: table_name.to_string(),
            });

            Ok(rows)
        })
        .await
}

/// Executes SELECT * FROM table WHERE column IN (id1, id2, ...)
/// with dialect-appropriate placeholders.
#[must_use]
#[tracing::instrument(skip(pool, tx))]
pub async fn generic_select_in(
    pool: &AnyPool,
    tx: Option<&Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    table_name: &str,
    column: &str,
    ids: &[String],
) -> BridgeResult<Vec<AnyRow>> {
    validate_identifier(table_name)?;
    validate_identifier(column)?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let dialect_type = SqlDialect::from_url(url);
    let dialect = dialect_type.to_dialect();
    let (sql, _) = dialect.build_select_in(table_name, column, ids.len())?;
    let start = Instant::now();
    let mut query = sqlx::query(&sql);
    for id in ids {
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
        query.fetch_all(&mut **tx_conn).await.map_err(|e| {
            BridgeError::from(e)
                .with_sql(sql.clone(), None)
                .add_breadcrumb("generic_select_in")
        })?
    } else {
        query.fetch_all(pool).await.map_err(|e| {
            BridgeError::from(e)
                .with_sql(sql.clone(), None)
                .add_breadcrumb("generic_select_in")
        })?
    };
    let duration = start.elapsed();
    logger::emit_telemetry(TelemetryEvent {
        sql: sql.clone(),
        duration_micros: duration.as_micros() as u64,
        operation: "SELECT_IN".to_string(),
        table: table_name.to_string(),
    });
    Ok(rows)
}

/// Rust implementation of lazy query.
#[must_use]
#[tracing::instrument(skip(pool, tx))]
pub fn query_lazy(
    pool: &AnyPool,
    tx: Option<Arc<Mutex<Option<sqlx::Transaction<'static, sqlx::Any>>>>>,
    url: &str,
    table_name: &str,
    filters: HashMap<String, QueryValue>,
    limit: Option<i64>,
    fields: Option<Vec<String>>,
) -> BridgeResult<BoxStream<'static, BridgeResult<AnyRow>>> {
    validate_identifier(table_name)?;
    let dialect_type = SqlDialect::from_url(url);
    let dialect = dialect_type.to_dialect();

    let columns = fields.unwrap_or_default();
    let filter_vec: Vec<(String, QueryValue)> = filters.into_iter().collect();

    let (sql, values) = dialect.build_select(table_name, &columns, &filter_vec, limit)?;

    let pool_clone = pool.clone();
    let stream = futures::stream::once(async move {
        let mut query = sqlx::query(&sql);
        for val in &values {
            query = bind_query_value(query, val);
        }

        if let Some(tx_mutex) = tx {
            let mut tx_guard = tx_mutex.lock().await;
            let tx_conn = tx_guard.as_mut().ok_or_else(|| {
                BridgeError::Validation(
                    "Transaction already closed".to_string(),
                    DiagnosticInfo::default(),
                )
            })?;
            query.fetch_all(&mut **tx_conn).await.map_err(|e| {
                BridgeError::from(e)
                    .with_sql(sql.clone(), None)
                    .add_breadcrumb("query_lazy")
            })
        } else {
            query.fetch_all(&pool_clone).await.map_err(|e| {
                BridgeError::from(e)
                    .with_sql(sql.clone(), None)
                    .add_breadcrumb("query_lazy")
            })
        }
    })
    .flat_map(|res| match res {
        Ok(rows) => futures::stream::iter(rows.into_iter().map(Ok)).left_stream(),
        Err(e) => futures::stream::once(async move { Err(e) }).right_stream(),
    })
    .boxed();

    Ok(stream)
}

/// Resolves a Python type name to its corresponding SQL type for a given dialect.
#[must_use]
pub fn resolve_python_type_to_sql(py_type: &str, dialect: &str) -> BridgeResult<String> {
    let is_optional = py_type.starts_with("Optional[") || py_type.contains("None");
    let base_type = if is_optional {
        py_type
            .replace("Optional[", "")
            .replace("]", "")
            .replace("None", "")
            .trim()
            .to_string()
    } else {
        py_type.to_string()
    };

    let sql_type = match (base_type.as_str(), dialect.to_lowercase().as_str()) {
        ("str", _) => "TEXT".to_string(),
        ("int", d) if d.contains("postgres") => "BIGINT".to_string(),
        ("int", d) if d.contains("mysql") => "BIGINT".to_string(),
        ("int", "sqlite") => "INTEGER".to_string(),
        ("int", d) if d.contains("mssql") => "BIGINT".to_string(),

        ("float", d) if d.contains("postgres") => "DOUBLE PRECISION".to_string(),
        ("float", "sqlite") => "REAL".to_string(),
        ("float", d) if d.contains("mysql") => "DOUBLE".to_string(),

        ("bool", d) if d.contains("postgres") => "BOOLEAN".to_string(),
        ("bool", "sqlite") | ("bool", _) if dialect.contains("mysql") => "INTEGER".to_string(),

        ("datetime", d) if d.contains("postgres") => "TIMESTAMP WITH TIME ZONE".to_string(),
        ("datetime", _) => "TEXT".to_string(),

        ("UUID", _) | ("uuid", _) => {
            if dialect == "sqlite" {
                "TEXT".to_string()
            } else {
                "UUID".to_string()
            }
        }
        (t, _) if t.contains("Enum") => "TEXT".to_string(),
        (unknown, _) => {
            return Err(BridgeError::Validation(
                format!("Unsupported Python type '{}'", unknown),
                DiagnosticInfo::default(),
            ))
        }
    };

    if !is_optional {
        Ok(format!("{} NOT NULL", sql_type))
    } else {
        Ok(sql_type)
    }
}
