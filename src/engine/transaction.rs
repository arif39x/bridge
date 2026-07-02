use once_cell::sync::Lazy;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use regex::Regex;
use sqlx::{Any, Transaction};
use std::sync::Arc;
use tokio::sync::Mutex;

#[pyclass]
#[derive(Clone)]
pub struct TxHandle {
    pub inner: Arc<Mutex<Option<Transaction<'static, Any>>>>,
    pub pool: sqlx::AnyPool,
    pub url: String,
    pub savepoint_depth: u32,
}

#[pymethods]
impl TxHandle {
    fn commit<'p>(&self, py: Python<'p>) -> PyResult<Bound<'p, PyAny>> {
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = inner.lock().await;
            let tx = guard
                .take()
                .ok_or_else(|| PyValueError::new_err("Transaction already closed"))?;
            tx.commit()
                .await
                .map_err(|e| PyValueError::new_err(format!("Commit failed: {}", e)))?;
            Ok(())
        })
    }

    fn rollback<'p>(&self, py: Python<'p>) -> PyResult<Bound<'p, PyAny>> {
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = inner.lock().await;
            let tx = guard
                .take()
                .ok_or_else(|| PyValueError::new_err("Transaction already closed"))?;
            tx.rollback()
                .await
                .map_err(|e| PyValueError::new_err(format!("Rollback failed: {}", e)))?;
            Ok(())
        })
    }
}

pub async fn begin_transaction(pool: &sqlx::AnyPool, url: &str) -> Result<TxHandle, sqlx::Error> {
    let tx = pool.begin().await?;
    Ok(TxHandle {
        inner: Arc::new(Mutex::new(Some(tx))),
        pool: pool.clone(),
        url: url.to_string(),
        savepoint_depth: 0,
    })
}

static SAVEPOINT_NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").expect("Invalid hardcoded savepoint regex"));

pub fn validate_savepoint_name(name: &str) -> PyResult<()> {
    if !SAVEPOINT_NAME_RE.is_match(name) {
        return Err(PyValueError::new_err("Invalid savepoint name"));
    }
    Ok(())
}
