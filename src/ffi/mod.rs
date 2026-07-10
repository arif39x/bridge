pub fn catch_panic<F, T>(f: F) -> Result<T, PyErr>
where
    F: FnOnce() -> Result<T, PyErr>,
{
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|e| {
        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown Rust panic".to_string()
        };
        Err(pyo3::exceptions::PyRuntimeError::new_err(format!(
            "Rust panic: {}",
            msg
        )))
    })
}

#[macro_export]
macro_rules! ffi_guard {
    ($py:expr, $body:expr) => {
        $crate::ffi::catch_panic(|| $body)
    };
    ($body:expr) => {
        $crate::ffi::catch_panic(|| $body)
    };
}

use crate::engine;
use crate::error::{BridgeError, BridgeResult, DiagnosticInfo};
use crate::schema;
use crate::telemetry;
pub mod java;
pub mod pool_config;
pub mod type_coercion;
use futures::stream::BoxStream;
use futures::StreamExt;
use pyo3::exceptions::{
    PyException, PyKeyError, PyRuntimeError, PyStopAsyncIteration, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use sqlx::{any::AnyRow, AnyPool, Transaction};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Holds pool, url, and optional transaction extracted from a tx parameter.
struct PoolTx {
    pool: AnyPool,
    url: String,
    tx_mutex: Option<Arc<Mutex<Option<Transaction<'static, sqlx::Any>>>>>,
}

/// Extracts (pool, url, optional tx_mutex) from an optional tx parameter.
/// If tx is None, falls back to the PoolManager's default pool.
fn extract_pool_tx(py: Python<'_>, tx: Option<&PyObject>) -> PyResult<PoolTx> {
    if let Some(tx_obj) = tx {
        if let Ok(session) = tx_obj.extract::<engine::session::Session>(py) {
            return Ok(PoolTx {
                pool: session.pool,
                url: session.url,
                tx_mutex: Some(session.transaction),
            });
        }
        if let Ok(tx_handle) = tx_obj.extract::<engine::transaction::TxHandle>(py) {
            return Ok(PoolTx {
                pool: tx_handle.pool,
                url: tx_handle.url,
                tx_mutex: Some(tx_handle.inner),
            });
        }
        return Err(PyValueError::new_err(
            "Invalid transaction or session object",
        ));
    }

    let mgr = crate::engine::pool_manager::pool_manager();
    let (pool, url) = mgr
        .get(None)
        .map_err(bridge_error_to_py)?
        .ok_or_else(|| PyException::new_err("Connection pool not initialized"))?;
    Ok(PoolTx {
        pool,
        url,
        tx_mutex: None,
    })
}

/// Converts a `BridgeError` to a `PyErr`.
fn bridge_error_to_py(err: BridgeError) -> PyErr {
    let err_str = err.to_string();
    match err {
        BridgeError::NotFound(_, _) => PyKeyError::new_err(err_str),
        BridgeError::Validation(_, _) => PyValueError::new_err(err_str),
        BridgeError::Database(ref sqlx_err, _) => match sqlx_err {
            sqlx::Error::RowNotFound => PyKeyError::new_err("Resource not found"),
            _ => PyRuntimeError::new_err(err_str),
        },
        _ => PyRuntimeError::new_err(err_str),
    }
}

use crate::engine::query::QueryValue;

fn query_value_to_py(py: Python<'_>, v: QueryValue) -> PyResult<PyObject> {
    match v {
        QueryValue::String(s) => Ok(s.to_object(py)),
        QueryValue::Int(i) => Ok(i.to_object(py)),
        QueryValue::Float(f) => Ok(f.to_object(py)),
        QueryValue::Bool(b) => Ok(b.to_object(py)),
        QueryValue::Uuid(u) => {
            let uuid_module = py.import_bound("uuid")?;
            let uuid_obj = uuid_module.call_method1("UUID", (u.to_string(),))?;
            Ok(uuid_obj.to_object(py))
        }
        QueryValue::DateTime(dt) => {
            let datetime_module = py.import_bound("datetime")?;
            let datetime_cls = datetime_module.getattr("datetime")?;
            let dt_obj = datetime_cls.call_method1("fromisoformat", (dt.to_rfc3339(),))?;
            Ok(dt_obj.to_object(py))
        }
        QueryValue::Json(j) => {
            let s = j.to_string();
            let json_module = py.import_bound("json")?;
            let json_obj = json_module.call_method1("loads", (s,))?;
            Ok(json_obj.to_object(py))
        }
        QueryValue::Bytes(b) => Ok(b.to_object(py)),
        #[cfg(feature = "allow-raw-sql")]
        QueryValue::Raw(raw) => {
            let dict = PyDict::new_bound(py);
            dict.set_item("sql", raw.sql)?;
            let params: Vec<PyObject> = raw
                .params
                .into_iter()
                .map(|p| query_value_to_py(py, p))
                .collect::<PyResult<Vec<_>>>()?;
            dict.set_item("params", params)?;
            Ok(dict.to_object(py))
        }
        QueryValue::Null => Ok(py.None()),
    }
}

fn py_to_query_value(
    py: Python<'_>,
    obj: &Bound<'_, PyAny>,
    table_name: &str,
    column_name: &str,
) -> BridgeResult<QueryValue> {
    if obj.is_none() {
        return Ok(QueryValue::Null);
    }

    let registry = engine::metadata::get_registry().ok_or_else(|| {
        BridgeError::Internal(
            "Registry not initialized".to_string(),
            DiagnosticInfo::default(),
        )
    })?;
    let meta = registry
        .mappings
        .get(table_name)
        .and_then(|m| m.columns.get(column_name));

    if let Some(m) = meta {
        return crate::ffi::type_coercion::coerce_py_value(obj, m, table_name);
    }

    // Default heuristics if meta is missing or doesn't match
    if let Ok(b) = obj.extract::<bool>() {
        // In Python, bool is a subclass of int, so check bool first.
        if obj.is_instance_of::<pyo3::types::PyBool>() {
            return Ok(QueryValue::Bool(b));
        }
    }

    if let Ok(i) = obj.extract::<i64>() {
        return Ok(QueryValue::Int(i));
    }

    if let Ok(f) = obj.extract::<f64>() {
        return Ok(QueryValue::Float(f));
    }

    // Heuristics for UUID/DateTime if meta is missing
    if let Ok(u) = uuid::Uuid::parse_str(&obj.to_string()) {
        // Basic check to avoid false positives with random strings
        if !obj.is_instance_of::<pyo3::types::PyString>() {
            return Ok(QueryValue::Uuid(u));
        }
    }

    if let Ok(s) = obj
        .call_method0("isoformat")
        .and_then(|r| r.extract::<String>())
    {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&s) {
            return Ok(QueryValue::DateTime(dt.with_timezone(&chrono::Utc)));
        }
    }

    if let Ok(b) = obj.extract::<Vec<u8>>() {
        return Ok(QueryValue::Bytes(b));
    }

    // Check for Raw expression
    if let Ok(sql_attr) = obj.getattr("sql") {
        if let Ok(sql) = sql_attr.extract::<String>() {
            if let Ok(params_attr) = obj.getattr("params") {
                if let Ok(params_py) = params_attr.extract::<Vec<Bound<'_, PyAny>>>() {
                    #[cfg(feature = "allow-raw-sql")]
                    {
                        let mut params = Vec::new();
                        for p in params_py {
                            params.push(py_to_query_value(py, &p, table_name, column_name)?);
                        }
                        return Ok(QueryValue::Raw(crate::engine::query::RawExpression {
                            sql,
                            params,
                        }));
                    }
                    #[cfg(not(feature = "allow-raw-sql"))]
                    {
                        return Err(BridgeError::Validation(
                            "Raw SQL expressions are not supported. Rebuild with `--features allow-raw-sql` to enable.".to_string(),
                            DiagnosticInfo::default(),
                        ));
                    }
                }
            }
        }
    }

    // Default to string representation
    if let Ok(val) = obj.extract::<String>() {
        Ok(QueryValue::String(val))
    } else {
        Ok(QueryValue::String(obj.to_string()))
    }
}

#[pyclass]
#[derive(Clone)]
pub struct ColumnMetaProxy {
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub data_type: String,
    #[pyo3(get)]
    pub is_nullable: bool,
    #[pyo3(get)]
    pub is_primary_key: bool,
    #[pyo3(get)]
    pub default_value: Option<String>,
}

#[pyclass]
pub struct LazyRowStream {
    pub stream: Arc<Mutex<BoxStream<'static, BridgeResult<AnyRow>>>>,
    pub table_name: String,
}

#[pymethods]
impl LazyRowStream {
    fn __aiter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __anext__<'p>(&self, py: Python<'p>) -> PyResult<Bound<'p, PyAny>> {
        let stream = self.stream.clone();
        let table_name = self.table_name.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut stream = stream.lock().await;
            match stream.next().await {
                Some(Ok(row)) => Python::with_gil(|py| {
                    let dict = engine::hydrator::hydrate_row(py, &table_name, &row)?;
                    Ok(dict.to_object(py))
                }),
                Some(Err(e)) => Err(bridge_error_to_py(e)),
                None => Err(PyStopAsyncIteration::new_err("Stream exhausted")),
            }
        })
    }
}

#[pyfunction]
fn configure_logging(level: String, slow_query_ms: u64) -> PyResult<()> {
    ffi_guard!({
        telemetry::logger::configure_logging(&level, slow_query_ms);
        Ok(())
    })
}

#[pyfunction]
#[pyo3(signature = (url, config=None))]
fn connect(
    py: Python<'_>,
    url: String,
    config: Option<pool_config::PoolConfig>,
) -> PyResult<Bound<'_, PyAny>> {
    ffi_guard!(py, {
        let url_clone = url.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let pool = engine::db::connect(&url_clone, config)
                .await
                .map_err(bridge_error_to_py)?;

            let mgr = engine::pool_manager::pool_manager();
            mgr.register("primary".to_string(), pool, url_clone)
                .map_err(bridge_error_to_py)?;
            mgr.set_default("primary".to_string())
                .map_err(bridge_error_to_py)?;

            Ok(())
        })
    })
}

#[pyclass]
#[derive(Clone)]
pub struct TableMetaProxy {
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub columns: Vec<ColumnMetaProxy>,
}

#[pyfunction]
fn reflect_schema(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, None)?;
        let pool = pt.pool;
        let url = pt.url;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tables = schema::introspect::reflect_schema(&pool, &url)
                .await
                .map_err(bridge_error_to_py)?;

            let proxies: Vec<TableMetaProxy> = tables
                .into_iter()
                .map(|t| TableMetaProxy {
                    name: t.name,
                    columns: t
                        .columns
                        .into_iter()
                        .map(|c| ColumnMetaProxy {
                            name: c.name,
                            data_type: c.data_type,
                            is_nullable: c.is_nullable,
                            is_primary_key: c.is_primary_key,
                            default_value: c.default_value,
                        })
                        .collect(),
                })
                .collect();

            Ok(proxies)
        })
    })
}

#[pyfunction]
fn reflect_table(py: Python<'_>, table_name: String) -> PyResult<Bound<'_, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, None)?;
        let pool = pt.pool;
        let url = pt.url;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let columns = schema::introspect::reflect_table(&pool, &url, &table_name)
                .await
                .map_err(bridge_error_to_py)?;

            let proxies: Vec<ColumnMetaProxy> = columns
                .into_iter()
                .map(|c| ColumnMetaProxy {
                    name: c.name,
                    data_type: c.data_type,
                    is_nullable: c.is_nullable,
                    is_primary_key: c.is_primary_key,
                    default_value: c.default_value,
                })
                .collect();

            Ok(proxies)
        })
    })
}

#[pyfunction]
#[pyo3(signature = (table, data, tx=None))]
fn insert_row<'py>(
    py: Python<'py>,
    table: String,
    data: Bound<'py, PyDict>,
    tx: Option<PyObject>,
) -> PyResult<Bound<'py, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;

        let table_clone = table.clone();
        let mut query_data: HashMap<String, QueryValue> = HashMap::new();
        for (k, v) in data {
            let key = k.extract::<String>()?;
            query_data.insert(
                key.clone(),
                py_to_query_value(py, &v, &table_clone, &key).map_err(bridge_error_to_py)?,
            );
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let res = engine::db::generic_insert(&pool, tx_mutex.as_ref(), &url, &table, query_data)
                .await
                .map_err(bridge_error_to_py)?;

            Python::with_gil(|py| {
                let dict = PyDict::new_bound(py);
                for (k, v) in res {
                    dict.set_item(k, query_value_to_py(py, v)?)?;
                }
                Ok(dict.to_object(py))
            })
        })
    })
}

#[pyfunction]
#[pyo3(signature = (table, filters, fields=None, tx=None))]
fn find_one<'py>(
    py: Python<'py>,
    table: String,
    filters: Bound<'py, PyDict>,
    fields: Option<Vec<String>>,
    tx: Option<PyObject>,
) -> PyResult<Bound<'py, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;

        let table_clone = table.clone();
        let mut query_filters: HashMap<String, QueryValue> = HashMap::new();
        for (k, v) in filters {
            let key = k.extract::<String>()?;
            query_filters.insert(
                key.clone(),
                py_to_query_value(py, &v, &table_clone, &key).map_err(bridge_error_to_py)?,
            );
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let rows = engine::db::generic_query(
                &pool,
                tx_mutex.as_ref(),
                &url,
                &table,
                query_filters,
                Some(1),
                fields,
            )
            .await
            .map_err(bridge_error_to_py)?;

            if rows.is_empty() {
                Ok(Python::with_gil(|py| py.None()))
            } else {
                Python::with_gil(|py| {
                    let dict = engine::hydrator::hydrate_row(py, &table, &rows[0])?;
                    Ok(dict.to_object(py))
                })
            }
        })
    })
}

#[cfg(feature = "data-science")]
#[pyfunction]
#[pyo3(signature = (table, filters, limit=None, fields=None, tx=None))]
fn fetch_all_arrow<'py>(
    py: Python<'py>,
    table: String,
    filters: Bound<'py, PyDict>,
    limit: Option<i64>,
    fields: Option<Vec<String>>,
    tx: Option<PyObject>,
) -> PyResult<Bound<'py, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;

        let table_clone = table.clone();
        let mut query_filters: HashMap<String, QueryValue> = HashMap::new();
        for (k, v) in filters {
            let key = k.extract::<String>()?;
            query_filters.insert(
                key.clone(),
                py_to_query_value(py, &v, &table_clone, &key).map_err(bridge_error_to_py)?,
            );
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let rows = engine::db::generic_query(
                &pool,
                tx_mutex.as_ref(),
                &url,
                &table,
                query_filters,
                limit,
                fields,
            )
            .await
            .map_err(bridge_error_to_py)?;

            let buffer = engine::arrow::rows_to_arrow_ipc(&table, &rows).map_err(bridge_error_to_py)?;

            Python::with_gil(|py| {
                let bytes = PyBytes::new_bound(py, &buffer);
                Ok(bytes.to_object(py))
            })
        })
    })
}

#[pyfunction]
#[pyo3(signature = (table, filters, limit=None, fields=None, eager_loads=None, tx=None))]
fn fetch_all<'py>(
    py: Python<'py>,
    table: String,
    filters: Bound<'py, PyDict>,
    limit: Option<i64>,
    fields: Option<Vec<String>>,
    eager_loads: Option<Vec<Bound<'py, PyDict>>>,
    tx: Option<PyObject>,
) -> PyResult<Bound<'py, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;

        let table_clone = table.clone();
        let mut query_filters: HashMap<String, QueryValue> = HashMap::new();
        for (k, v) in filters {
            let key = k.extract::<String>()?;
            query_filters.insert(
                key.clone(),
                py_to_query_value(py, &v, &table_clone, &key).map_err(bridge_error_to_py)?,
            );
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let rows = engine::db::generic_query(
                &pool,
                tx_mutex.as_ref(),
                &url,
                &table,
                query_filters,
                limit,
                fields,
            )
            .await
            .map_err(bridge_error_to_py)?;

            Python::with_gil(|py| {
                let mut results = Vec::new();
                for row in rows {
                    let dict = engine::hydrator::hydrate_row(py, &table, &row)?;
                    results.push(dict.to_object(py));
                }
                Ok(results)
            })
        })
    })
}

#[pyfunction]
#[pyo3(signature = (table, filters, limit=None, fields=None, tx=None))]
fn fetch_lazy(
    py: Python<'_>,
    table: String,
    filters: Bound<'_, PyDict>,
    limit: Option<i64>,
    fields: Option<Vec<String>>,
    tx: Option<PyObject>,
) -> PyResult<LazyRowStream> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;

        let table_clone = table.clone();
        let mut query_filters: HashMap<String, QueryValue> = HashMap::new();
        for (k, v) in filters {
            let key = k.extract::<String>()?;
            query_filters.insert(
                key.clone(),
                py_to_query_value(py, &v, &table_clone, &key).map_err(bridge_error_to_py)?,
            );
        }

        let stream =
            engine::db::query_lazy(&pool, tx_mutex, &url, &table, query_filters, limit, fields)
                .map_err(bridge_error_to_py)?;

        Ok(LazyRowStream {
            stream: Arc::new(Mutex::new(stream)),
            table_name: table,
        })
    })
}

#[pyfunction]
#[pyo3(signature = (table, filters, tx=None))]
fn delete_row<'py>(
    py: Python<'py>,
    table: String,
    filters: Bound<'py, PyDict>,
    tx: Option<PyObject>,
) -> PyResult<Bound<'py, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;

        let table_clone = table.clone();
        let mut query_filters: HashMap<String, QueryValue> = HashMap::new();
        for (k, v) in filters {
            let key = k.extract::<String>()?;
            query_filters.insert(
                key.clone(),
                py_to_query_value(py, &v, &table_clone, &key).map_err(bridge_error_to_py)?,
            );
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            engine::db::generic_delete(&pool, tx_mutex.as_ref(), &url, &table, query_filters)
                .await
                .map_err(bridge_error_to_py)
        })
    })
}

#[cfg(feature = "allow-raw-sql")]
#[pyfunction]
fn execute_raw(py: Python<'_>, sql: String) -> PyResult<Bound<'_, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, None)?;
        let pool = pt.pool;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            engine::db::execute_raw(&pool, &sql)
                .await
                .map_err(bridge_error_to_py)?;
            Ok(())
        })
    })
}

#[pyfunction]
fn resolve_type(py_type: String, dialect: String) -> PyResult<String> {
    ffi_guard!({
        engine::db::resolve_python_type_to_sql(&py_type, &dialect).map_err(bridge_error_to_py)
    })
}

#[pyfunction]
fn set_telemetry_logger(logger: PyObject) -> PyResult<()> {
    ffi_guard!({
        telemetry::logger::set_python_logger(logger);
        Ok(())
    })
}

#[pyfunction]
#[pyo3(signature = (table, items, tx=None))]
fn insert_rows_bulk<'py>(
    py: Python<'py>,
    table: String,
    items: Vec<Bound<'py, PyDict>>,
    tx: Option<PyObject>,
) -> PyResult<Bound<'py, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;

        let table_clone = table.clone();
        let mut query_items: Vec<HashMap<String, QueryValue>> = Vec::new();
        for item in items {
            let mut query_item = HashMap::new();
            for (k, v) in item {
                let key = k.extract::<String>()?;
                query_item.insert(
                    key.clone(),
                    py_to_query_value(py, &v, &table_clone, &key).map_err(bridge_error_to_py)?,
                );
            }
            query_items.push(query_item);
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let res =
                engine::db::generic_insert_bulk(&pool, tx_mutex.as_ref(), &url, &table, query_items)
                    .await
                    .map_err(bridge_error_to_py)?;

            Python::with_gil(|py| {
                let mut results = Vec::new();
                for item in res {
                    let dict = PyDict::new_bound(py);
                    for (k, v) in item {
                        dict.set_item(k, query_value_to_py(py, v)?)?;
                    }
                    results.push(dict.to_object(py));
                }
                Ok(results)
            })
        })
    })
}

#[pyfunction]
#[pyo3(signature = (table, foreign_key, parent_id, tx=None))]
fn fetch_one_to_many(
    py: Python<'_>,
    table: String,
    foreign_key: String,
    parent_id: String,
    tx: Option<PyObject>,
) -> PyResult<Bound<'_, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let rows = engine::relations::fetch_one_to_many(
                &pool,
                tx_mutex.as_ref(),
                &url,
                &table,
                &foreign_key,
                &parent_id,
            )
            .await
            .map_err(bridge_error_to_py)?;

            Python::with_gil(|py| {
                let mut results = Vec::new();
                for row in rows {
                    let dict = engine::hydrator::hydrate_row(py, &table, &row)?;
                    results.push(dict.to_object(py));
                }
                Ok(results)
            })
        })
    })
}

#[pyfunction]
#[pyo3(signature = (target_table, junction_table, left_key, right_key, parent_id, tx=None))]
fn fetch_many_to_many(
    py: Python<'_>,
    target_table: String,
    junction_table: String,
    left_key: String,
    right_key: String,
    parent_id: String,
    tx: Option<PyObject>,
) -> PyResult<Bound<'_, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let rows = engine::relations::fetch_many_to_many(
                &pool,
                tx_mutex.as_ref(),
                &url,
                &target_table,
                &junction_table,
                &left_key,
                &right_key,
                &parent_id,
            )
            .await
            .map_err(bridge_error_to_py)?;

            Python::with_gil(|py| {
                let mut results = Vec::new();
                for row in rows {
                    let dict = engine::hydrator::hydrate_row(py, &target_table, &row)?;
                    results.push(dict.to_object(py));
                }
                Ok(results)
            })
        })
    })
}

#[pyfunction]
#[pyo3(signature = (table, parent_key, parent_id, tx=None))]
fn fetch_self_ref(
    py: Python<'_>,
    table: String,
    parent_key: String,
    parent_id: String,
    tx: Option<PyObject>,
) -> PyResult<Bound<'_, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let rows = engine::relations::fetch_self_ref(
                &pool,
                tx_mutex.as_ref(),
                &url,
                &table,
                &parent_key,
                &parent_id,
            )
            .await
            .map_err(bridge_error_to_py)?;

            Python::with_gil(|py| {
                let mut results = Vec::new();
                for row in rows {
                    let dict = engine::hydrator::hydrate_row(py, &table, &row)?;
                    results.push(dict.to_object(py));
                }
                Ok(results)
            })
        })
    })
}

#[pyfunction]
#[pyo3(signature = (child_table, foreign_key, parent_ids, tx=None))]
fn batch_fetch_one_to_many<'py>(
    py: Python<'py>,
    child_table: String,
    foreign_key: String,
    parent_ids: Vec<String>,
    tx: Option<PyObject>,
) -> PyResult<Bound<'py, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let grouped = engine::relations::batch_fetch_one_to_many(
                &pool,
                tx_mutex.as_ref(),
                &url,
                &child_table,
                &foreign_key,
                &parent_ids,
            )
            .await
            .map_err(bridge_error_to_py)?;
            Python::with_gil(|py| {
                let result_dict = PyDict::new_bound(py);
                for (parent_id, rows) in grouped {
                    let row_list: Vec<PyObject> = rows
                        .iter()
                        .map(|row| {
                            engine::hydrator::hydrate_row(py, &child_table, row)
                                .map(|d| d.to_object(py))
                        })
                        .collect::<PyResult<Vec<_>>>()?;
                    result_dict.set_item(&parent_id, row_list)?;
                }
                Ok(result_dict.to_object(py))
            })
        })
    })
}

#[pyfunction]
#[pyo3(signature = (target_table, junction_table, left_key, right_key, parent_ids, tx=None))]
fn batch_fetch_many_to_many<'py>(
    py: Python<'py>,
    target_table: String,
    junction_table: String,
    left_key: String,
    right_key: String,
    parent_ids: Vec<String>,
    tx: Option<PyObject>,
) -> PyResult<Bound<'py, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let grouped = engine::relations::batch_fetch_many_to_many(
                &pool,
                tx_mutex.as_ref(),
                &url,
                &target_table,
                &junction_table,
                &left_key,
                &right_key,
                &parent_ids,
            )
            .await
            .map_err(bridge_error_to_py)?;
            Python::with_gil(|py| {
                let result_dict = PyDict::new_bound(py);
                for (parent_id, rows) in grouped {
                    let row_list: Vec<PyObject> = rows
                        .iter()
                        .map(|row| {
                            engine::hydrator::hydrate_row(py, &target_table, row)
                                .map(|d| d.to_object(py))
                        })
                        .collect::<PyResult<Vec<_>>>()?;
                    result_dict.set_item(&parent_id, row_list)?;
                }
                Ok(result_dict.to_object(py))
            })
        })
    })
}

#[pyfunction]
#[pyo3(signature = (table, parent_key, parent_ids, tx=None))]
fn batch_fetch_self_ref<'py>(
    py: Python<'py>,
    table: String,
    parent_key: String,
    parent_ids: Vec<String>,
    tx: Option<PyObject>,
) -> PyResult<Bound<'py, PyAny>> {
    ffi_guard!(py, {
        let pt = extract_pool_tx(py, tx.as_ref())?;
        let pool = pt.pool;
        let url = pt.url;
        let tx_mutex = pt.tx_mutex;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let grouped = engine::relations::batch_fetch_self_ref(
                &pool,
                tx_mutex.as_ref(),
                &url,
                &table,
                &parent_key,
                &parent_ids,
            )
            .await
            .map_err(bridge_error_to_py)?;
            Python::with_gil(|py| {
                let result_dict = PyDict::new_bound(py);
                for (parent_id, rows) in grouped {
                    let row_list: Vec<PyObject> = rows
                        .iter()
                        .map(|row| {
                            engine::hydrator::hydrate_row(py, &table, row).map(|d| d.to_object(py))
                        })
                        .collect::<PyResult<Vec<_>>>()?;
                    result_dict.set_item(&parent_id, row_list)?;
                }
                Ok(result_dict.to_object(py))
            })
        })
    })
}

#[pyfunction]
#[pyo3(signature = (pool_key=None))]
fn begin_transaction(py: Python<'_>, pool_key: Option<String>) -> PyResult<Bound<'_, PyAny>> {
    ffi_guard!(py, {
        let mgr = engine::pool_manager::pool_manager();
        let (pool, url) = mgr
            .get(pool_key.as_deref())
            .map_err(bridge_error_to_py)?
            .ok_or_else(|| PyException::new_err("Connection pool not initialized"))?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx = engine::transaction::begin_transaction(&pool, &url)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            Ok(tx)
        })
    })
}

#[pyfunction]
#[pyo3(signature = (pool_key=None))]
fn begin_session(py: Python<'_>, pool_key: Option<String>) -> PyResult<Bound<'_, PyAny>> {
    ffi_guard!(py, {
        let mgr = engine::pool_manager::pool_manager();
        let (pool, url) = mgr
            .get(pool_key.as_deref())
            .map_err(bridge_error_to_py)?
            .ok_or_else(|| PyException::new_err("Connection pool not initialized"))?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let session = engine::session::begin_session(pool, url)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            Ok(session)
        })
    })
}

#[pyfunction]
fn snapshot_entity(
    py: Python<'_>,
    session: engine::session::Session,
    key: String,
    table_name: String,
    values: Bound<'_, PyDict>,
) -> PyResult<()> {
    ffi_guard!(py, {
        let mut query_values = HashMap::new();
        for (k, v) in values {
            let key = k.extract::<String>()?;
            query_values.insert(
                key.clone(),
                py_to_query_value(py, &v, &table_name, &key).map_err(bridge_error_to_py)?,
            );
        }
        session.snapshot_entity_internal(key, table_name, query_values)
    })
}

#[pyfunction]
fn flush<'py>(
    py: Python<'py>,
    session: engine::session::Session,
    dirty_entities: Vec<(String, String, Bound<'py, PyDict>, Bound<'py, PyDict>)>,
) -> PyResult<Bound<'py, PyAny>> {
    ffi_guard!(py, {
        let pool = session.pool.clone();
        let url = session.url.clone();

        // To make it Send-safe, compute diffs and prepare updates synchronously (with GIL)
        // and then only pass pure Rust data into the async block.

        struct UpdateJob {
            table_name: String,
            diff: HashMap<String, QueryValue>,
            pk_filters: HashMap<String, QueryValue>,
            key: String,
            full_values: HashMap<String, QueryValue>,
        }

        let mut jobs = Vec::new();
        {
            let tracker_guard = session
                .dirty_tracker
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock poisoned: {}", e)))?;
            for (key, table_name, current_values_py, pk_filters_py) in dirty_entities {
                let mut current_values = HashMap::new();
                for (k, v) in current_values_py {
                    let col_name = k.extract::<String>()?;
                    current_values.insert(
                        col_name.clone(),
                        py_to_query_value(py, &v, &table_name, &col_name)
                            .map_err(bridge_error_to_py)?,
                    );
                }

                if let Some(diff) = tracker_guard.compute_diff(&key, &current_values) {
                    let mut pk_filters = HashMap::new();
                    for (k, v) in pk_filters_py {
                        let col_name = k.extract::<String>()?;
                        pk_filters.insert(
                            col_name.clone(),
                            py_to_query_value(py, &v, &table_name, &col_name)
                                .map_err(bridge_error_to_py)?,
                        );
                    }

                    jobs.push(UpdateJob {
                        table_name,
                        diff,
                        pk_filters,
                        key,
                        full_values: current_values,
                    });
                }
            }
        }

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            for job in jobs {
                engine::db::generic_update(
                    &pool,
                    Some(&session.transaction),
                    &url,
                    &job.table_name,
                    job.diff,
                    job.pk_filters,
                )
                .await
                .map_err(bridge_error_to_py)?;

                // Re-acquire lock to update snapshot
                let mut tracker_guard = session
                    .dirty_tracker
                    .lock()
                    .map_err(|e| PyRuntimeError::new_err(format!("Lock poisoned: {}", e)))?;
                tracker_guard.take_snapshot(job.key, job.table_name, job.full_values);
            }
            Ok(())
        })
    })
}

pub fn register_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ColumnMetaProxy>()?;
    m.add_class::<TableMetaProxy>()?;
    m.add_class::<LazyRowStream>()?;
    m.add_class::<engine::transaction::TxHandle>()?;
    m.add_class::<engine::session::Session>()?;
    m.add_function(wrap_pyfunction!(set_telemetry_logger, m)?)?;
    m.add_function(wrap_pyfunction!(configure_logging, m)?)?;
    m.add_function(wrap_pyfunction!(connect, m)?)?;
    m.add_function(wrap_pyfunction!(reflect_schema, m)?)?;
    m.add_function(wrap_pyfunction!(reflect_table, m)?)?;

    m.add_function(wrap_pyfunction!(begin_session, m)?)?;
    m.add_function(wrap_pyfunction!(insert_row, m)?)?;
    m.add_function(wrap_pyfunction!(insert_rows_bulk, m)?)?;
    m.add_function(wrap_pyfunction!(find_one, m)?)?;
    m.add_function(wrap_pyfunction!(fetch_all, m)?)?;
    #[cfg(feature = "data-science")]
    m.add_function(wrap_pyfunction!(fetch_all_arrow, m)?)?;
    m.add_function(wrap_pyfunction!(fetch_lazy, m)?)?;
    m.add_function(wrap_pyfunction!(snapshot_entity, m)?)?;
    m.add_function(wrap_pyfunction!(flush, m)?)?;
    m.add_function(wrap_pyfunction!(fetch_one_to_many, m)?)?;
    m.add_function(wrap_pyfunction!(fetch_many_to_many, m)?)?;
    m.add_function(wrap_pyfunction!(fetch_self_ref, m)?)?;
    m.add_function(wrap_pyfunction!(batch_fetch_one_to_many, m)?)?;
    m.add_function(wrap_pyfunction!(batch_fetch_many_to_many, m)?)?;
    m.add_function(wrap_pyfunction!(batch_fetch_self_ref, m)?)?;
    m.add_function(wrap_pyfunction!(delete_row, m)?)?;
    #[cfg(feature = "allow-raw-sql")]
    m.add_function(wrap_pyfunction!(execute_raw, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_type, m)?)?;
    m.add_function(wrap_pyfunction!(engine::metadata::register_entity, m)?)?;
    m.add_function(wrap_pyfunction!(engine::metadata::lock_registry, m)?)?;
    Ok(())
}
