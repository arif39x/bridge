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
    Lazy::new(|| Regex::new(VALID_IDENTIFIER_REGEX).expect("Invalid hardcoded identifier regex"));

pub static CIRCUIT_BREAKER_REGISTRY: Lazy<
    crate::engine::circuit_breaker::CircuitBreakerRegistry,
> = Lazy::new(|| {
    crate::engine::circuit_breaker::CircuitBreakerRegistry::new(
        5,
        std::time::Duration::from_secs(30),
    )
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
    fn build_version_guarded_update(
        &self,
        table_name: &str,
        primary_key_column: &str,
        primary_key_value: &str,
        version_column_name: &str,
        known_version: u64,
        next_version: u64,
        column_value_pairs: &[(String, QueryValue)],
    ) -> BridgeResult<(String, Vec<QueryValue>)> {
        validate_identifier(table_name)?;
        validate_identifier(primary_key_column)?;
        validate_identifier(version_column_name)?;
        for (col, _) in column_value_pairs {
            validate_identifier(col)?;
        }

        let mut sql = format!("UPDATE {} SET ", self.quote_identifier(table_name));
        let mut values = Vec::new();
        let mut set_clauses = Vec::new();

        for (col, val) in column_value_pairs {
            set_clauses.push(format!(
                "{} = {}",
                self.quote_identifier(col),
                self.get_placeholder(values.len())
            ));
            values.push(val.clone());
        }

        set_clauses.push(format!(
            "{} = {}",
            self.quote_identifier(version_column_name),
            self.get_placeholder(values.len())
        ));
        values.push(QueryValue::Int(next_version as i64));

        sql.push_str(&set_clauses.join(", "));

        sql.push_str(&format!(
            " WHERE {} = {} AND {} = {}",
            self.quote_identifier(primary_key_column),
            self.get_placeholder(values.len()),
            self.quote_identifier(version_column_name),
            self.get_placeholder(values.len() + 1),
        ));
        values.push(QueryValue::String(primary_key_value.to_owned()));
        values.push(QueryValue::Int(known_version as i64));

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
        use crate::engine::metadata::get_registry;

        let registry = get_registry().ok_or_else(|| {
            BridgeError::Internal(
                "Registry not initialized".to_string(),
                DiagnosticInfo::default(),
            )
        })?;
        if let Some(mapping) = registry.mappings.get(table_name) {
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
) -> BridgeResult<sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>>> {
    match value {
        QueryValue::String(s) => Ok(query.bind(s)),
        QueryValue::Int(i) => Ok(query.bind(i)),
        QueryValue::Float(f) => Ok(query.bind(f)),
        QueryValue::Bool(b) => Ok(query.bind(b)),
        QueryValue::Uuid(u) => Ok(query.bind(u.to_string())),
        QueryValue::DateTime(dt) => Ok(query.bind(dt.to_rfc3339())),
        QueryValue::Json(j) => Ok(query.bind(j.to_string())),
        QueryValue::Bytes(b) => Ok(query.bind(b)),
        #[cfg(feature = "allow-raw-sql")]
        QueryValue::Raw(raw) => Err(BridgeError::Runtime(
            format!("RawExpression should have been expanded before binding: sql={}", raw.sql),
            DiagnosticInfo::default(),
        )),
        QueryValue::Null => Ok(query.bind(None::<String>)),
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
        query = bind_query_value(query, val)?;
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
        query = bind_query_value(query, val)?;
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
        query = bind_query_value(query, val)?;
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
        query = bind_query_value(query, val)?;
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

    let cb = CIRCUIT_BREAKER_REGISTRY.get_or_create(url)?;
    cb.call(|| async {
        let start = Instant::now();
        let mut query = sqlx::query(&sql);
        for val in &values {
            query = bind_query_value(query, val)?;
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
            query = bind_query_value(query, val)?;
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

    Ok(sql_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_dialect_from_url_postgres() {
        assert_eq!(SqlDialect::from_url("postgres://localhost/mydb"), SqlDialect::Postgres);
        assert_eq!(SqlDialect::from_url("postgresql://localhost/mydb"), SqlDialect::Postgres);
    }

    #[test]
    fn sql_dialect_from_url_sqlite() {
        assert_eq!(SqlDialect::from_url("sqlite:data.db"), SqlDialect::Sqlite);
        assert_eq!(SqlDialect::from_url("sqlite:///tmp/test.db"), SqlDialect::Sqlite);
    }

    #[test]
    fn sql_dialect_from_url_mysql() {
        assert_eq!(SqlDialect::from_url("mysql://localhost/mydb"), SqlDialect::MySql);
        assert_eq!(SqlDialect::from_url("mariadb://localhost/mydb"), SqlDialect::MariaDb);
    }

    #[test]
    fn sql_dialect_from_url_mssql() {
        assert_eq!(SqlDialect::from_url("mssql://localhost/mydb"), SqlDialect::MsSql);
        assert_eq!(SqlDialect::from_url("sqlserver://localhost/mydb"), SqlDialect::MsSql);
    }

    #[test]
    fn sql_dialect_from_url_oracle() {
        assert_eq!(SqlDialect::from_url("oracle://host/sid"), SqlDialect::Oracle);
        assert_eq!(SqlDialect::from_url("thin://host/service"), SqlDialect::Oracle);
    }

    #[test]
    fn sql_dialect_from_url_postgres_variants() {
        assert_eq!(
            SqlDialect::from_url("postgres://host.neon.tech/db"),
            SqlDialect::Neon
        );
        assert_eq!(
            SqlDialect::from_url("postgres://host:26257/db"),
            SqlDialect::CockroachDb
        );
        assert_eq!(
            SqlDialect::from_url("postgres://yugabyte.host/db"),
            SqlDialect::YugabyteDb
        );
    }

    #[test]
    fn sql_dialect_from_url_mysql_variants() {
        assert_eq!(
            SqlDialect::from_url("mysql://host.psdb.cloud/db"),
            SqlDialect::PlanetScale
        );
        assert_eq!(
            SqlDialect::from_url("mysql://host.dolt/db"),
            SqlDialect::Dolt
        );
    }

    #[test]
    fn sql_dialect_from_url_sqlite_variants() {
        assert_eq!(
            SqlDialect::from_url("https://d1.cloudflare.com/db"),
            SqlDialect::CloudflareD1
        );
    }

    #[test]
    fn sql_dialect_from_url_unknown_defaults_to_postgres() {
        assert_eq!(SqlDialect::from_url("unknown://host/db"), SqlDialect::Postgres);
    }

    #[test]
    fn sql_dialect_to_dialect() {
        assert!(SqlDialect::Postgres.to_dialect().get_placeholder(0).contains('$'));
        assert_eq!(SqlDialect::Sqlite.to_dialect().get_placeholder(0), "$1");
        assert_eq!(SqlDialect::MySql.to_dialect().get_placeholder(0), "?");
        assert_eq!(SqlDialect::MsSql.to_dialect().get_placeholder(0), "@p1");
        assert_eq!(SqlDialect::Oracle.to_dialect().get_placeholder(0), ":1");
        assert_eq!(SqlDialect::Neon.to_dialect().get_placeholder(0), "$1");
        assert_eq!(SqlDialect::CockroachDb.to_dialect().get_placeholder(0), "$1");
        assert_eq!(SqlDialect::YugabyteDb.to_dialect().get_placeholder(0), "$1");
        assert_eq!(SqlDialect::MariaDb.to_dialect().get_placeholder(0), "?");
        assert_eq!(SqlDialect::PlanetScale.to_dialect().get_placeholder(0), "?");
        assert_eq!(SqlDialect::Dolt.to_dialect().get_placeholder(0), "?");
        assert_eq!(SqlDialect::CloudflareD1.to_dialect().get_placeholder(0), "$1");
    }

    #[test]
    fn dialect_placeholders() {
        assert_eq!(SqliteDialect.get_placeholder(0), "$1");
        assert_eq!(SqliteDialect.get_placeholder(9), "$10");
        assert_eq!(PostgreSqlDialect.get_placeholder(0), "$1");
        assert_eq!(MySqlDialect.get_placeholder(5), "?");
        assert_eq!(MsSqlDialect.get_placeholder(0), "@p1");
        assert_eq!(MsSqlDialect.get_placeholder(2), "@p3");
        assert_eq!(OracleDialect.get_placeholder(0), ":1");
        assert_eq!(OracleDialect.get_placeholder(2), ":3");
    }

    #[test]
    fn dialect_quote_identifier() {
        assert_eq!(SqliteDialect.quote_identifier("col"), r#""col""#);
        assert_eq!(PostgreSqlDialect.quote_identifier("col"), r#""col""#);
        assert_eq!(MySqlDialect.quote_identifier("col"), "`col`");
        assert_eq!(MsSqlDialect.quote_identifier("col"), "[col]");
        assert_eq!(OracleDialect.quote_identifier("col"), r#""col""#);
    }

    #[test]
    fn validate_identifier_valid() {
        assert!(validate_identifier("valid_name").is_ok());
        assert!(validate_identifier("_leading_underscore").is_ok());
        assert!(validate_identifier("a1b2c3").is_ok());
    }

    #[test]
    fn validate_identifier_invalid() {
        assert!(validate_identifier("").is_err());
        assert!(validate_identifier("1number").is_err());
        assert!(validate_identifier("has spaces").is_err());
        assert!(validate_identifier("sql-injection").is_err());
        assert!(validate_identifier("table;").is_err());
    }

    #[test]
    fn validate_identifier_reserved_keywords() {
        assert!(validate_identifier("SELECT").is_err());
        assert!(validate_identifier("drop").is_err());
        assert!(validate_identifier("Union").is_err());
        assert!(validate_identifier("DELETE").is_err());
    }

    #[test]
    fn validate_filter_value_safe() {
        assert!(validate_filter_value(&QueryValue::String("hello".into())).is_ok());
        assert!(validate_filter_value(&QueryValue::Int(42)).is_ok());
        assert!(validate_filter_value(&QueryValue::Null).is_ok());
    }

    #[test]
    fn validate_filter_value_dangerous() {
        assert!(validate_filter_value(&QueryValue::String("'; DROP TABLE".into())).is_err());
        assert!(validate_filter_value(&QueryValue::String("value -- comment".into())).is_err());
        assert!(validate_filter_value(&QueryValue::String("/* inline */".into())).is_err());
    }

    #[test]
    fn build_select_no_filters() {
        let (sql, values) = PostgreSqlDialect.build_select("users", &[], &[], None).unwrap();
        assert_eq!(sql, r#"SELECT * FROM "users""#);
        assert!(values.is_empty());
    }

    #[test]
    fn build_select_with_filters() {
        let filters = vec![
            ("name".into(), QueryValue::String("alice".into())),
            ("age".into(), QueryValue::Int(30)),
        ];
        let (sql, values) = PostgreSqlDialect
            .build_select("users", &[], &filters, None)
            .unwrap();
        assert!(sql.contains(r#""name" = $1"#));
        assert!(sql.contains(r#""age" = $2"#));
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn build_select_with_columns() {
        let columns = vec!["id".into(), "email".into()];
        let (sql, _) = PostgreSqlDialect
            .build_select("users", &columns, &[], None)
            .unwrap();
        assert_eq!(sql, r#"SELECT "id", "email" FROM "users""#);
    }

    #[test]
    fn build_select_with_limit() {
        let (sql, _) = PostgreSqlDialect
            .build_select("users", &[], &[], Some(10))
            .unwrap();
        assert!(sql.contains("LIMIT 10"));
    }

    #[test]
    fn build_select_oracle_limit() {
        let (sql, _) = OracleDialect
            .build_select("users", &[], &[], Some(5))
            .unwrap();
        assert!(sql.contains("FETCH NEXT 5 ROWS ONLY"));
    }

    #[test]
    fn build_select_invalid_identifier() {
        let result = PostgreSqlDialect.build_select("bad;table", &[], &[], None);
        assert!(result.is_err());
    }

    #[test]
    fn build_select_in() {
        let (sql, _) = PostgreSqlDialect.build_select_in("users", "id", 3).unwrap();
        assert_eq!(sql, r#"SELECT * FROM "users" WHERE "id" IN ($1, $2, $3)"#);
    }

    #[test]
    fn build_select_in_mysql() {
        let (sql, _) = MySqlDialect.build_select_in("users", "id", 2).unwrap();
        assert_eq!(sql, "SELECT * FROM `users` WHERE `id` IN (?, ?)");
    }

    #[test]
    fn build_many_to_many_select_in() {
        let (sql, _) = PostgreSqlDialect
            .build_many_to_many_select_in("posts", "post_tags", "post_id", "tag_id", 2)
            .unwrap();
        assert!(sql.contains(r#"FROM "posts" t"#));
        assert!(sql.contains(r#"JOIN "post_tags" j"#));
        assert!(sql.contains(r#"t.id = j."tag_id""#));
        assert!(sql.contains(r#"j."post_id" IN ("#));
    }

    #[test]
    fn build_version_guarded_update() {
        let pairs = vec![
            ("title".into(), QueryValue::String("new title".into())),
        ];
        let (sql, values) = PostgreSqlDialect
            .build_version_guarded_update("posts", "id", "42", "_version", 1, 2, &pairs)
            .unwrap();
        assert!(sql.contains(r#"UPDATE "posts" SET"#));
        assert!(sql.contains(r#""title" = $1"#));
        assert!(sql.contains(r#""_version" = $2"#));
        assert!(sql.contains(r#""id" = $3"#));
        assert!(sql.contains(r#""_version" = $4"#));
        assert_eq!(values.len(), 4);
    }

    #[test]
    fn build_version_guarded_update_invalid_table() {
        let result = PostgreSqlDialect.build_version_guarded_update(
            "bad table", "id", "42", "_v", 1, 2, &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn resolve_python_type_to_sql_str() {
        let result = resolve_python_type_to_sql("str", "postgres").unwrap();
        assert_eq!(result, "TEXT");
    }

    #[test]
    fn resolve_python_type_to_sql_int() {
        assert_eq!(resolve_python_type_to_sql("int", "postgres").unwrap(), "BIGINT");
        assert_eq!(resolve_python_type_to_sql("int", "mysql").unwrap(), "BIGINT");
        assert_eq!(resolve_python_type_to_sql("int", "sqlite").unwrap(), "INTEGER");
        assert_eq!(resolve_python_type_to_sql("int", "mssql").unwrap(), "BIGINT");
    }

    #[test]
    fn resolve_python_type_to_sql_float() {
        assert_eq!(
            resolve_python_type_to_sql("float", "postgres").unwrap(),
            "DOUBLE PRECISION"
        );
        assert_eq!(resolve_python_type_to_sql("float", "sqlite").unwrap(), "REAL");
        assert_eq!(resolve_python_type_to_sql("float", "mysql").unwrap(), "DOUBLE");
    }

    #[test]
    fn resolve_python_type_to_sql_datetime() {
        assert_eq!(
            resolve_python_type_to_sql("datetime", "postgres").unwrap(),
            "TIMESTAMP WITH TIME ZONE"
        );
        assert_eq!(resolve_python_type_to_sql("datetime", "sqlite").unwrap(), "TEXT");
    }

    #[test]
    fn resolve_python_type_to_sql_uuid() {
        assert_eq!(resolve_python_type_to_sql("uuid", "postgres").unwrap(), "UUID");
        assert_eq!(resolve_python_type_to_sql("UUID", "postgres").unwrap(), "UUID");
        assert_eq!(resolve_python_type_to_sql("uuid", "sqlite").unwrap(), "TEXT");
    }

    #[test]
    fn resolve_python_type_to_sql_optional() {
        let result = resolve_python_type_to_sql("Optional[str]", "postgres").unwrap();
        assert_eq!(result, "TEXT");

        let result = resolve_python_type_to_sql("Optional[int]", "sqlite").unwrap();
        assert_eq!(result, "INTEGER");
    }

    #[test]
    fn resolve_python_type_to_sql_unsupported() {
        let result = resolve_python_type_to_sql("some_custom_type", "postgres");
        assert!(result.is_err());
    }

    #[test]
    fn prepare_statement_valid() {
        let mut data = HashMap::new();
        data.insert("name".into(), QueryValue::String("alice".into()));
        data.insert("age".into(), QueryValue::Int(30));

        let (columns, values, placeholders) =
            prepare_statement(&PostgreSqlDialect, &data).unwrap();
        assert_eq!(columns.len(), 2);
        assert_eq!(values.len(), 2);
        assert_eq!(placeholders.len(), 2);
        assert!(placeholders.iter().all(|p| p.starts_with('$')));
    }

    #[test]
    fn prepare_statement_invalid_column() {
        let mut data = HashMap::new();
        data.insert("bad column".into(), QueryValue::Int(1));
        let result = prepare_statement(&PostgreSqlDialect, &data);
        assert!(result.is_err());
    }
}
