use crate::engine::dirty_tracker::DirtyTracker;
use crate::engine::query::QueryValue;
use crate::error::BridgeResult;
use pyo3::prelude::*;
use sqlx::{Any, AnyPool, Transaction};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;

#[pyclass]
#[derive(Clone)]
pub struct Session {
    pub pool: AnyPool,
    pub transaction: Arc<TokioMutex<Option<Transaction<'static, Any>>>>,
    pub identity_map: Arc<Mutex<HashMap<String, PyObject>>>,
    pub dirty_tracker: Arc<Mutex<DirtyTracker>>,
}

#[pymethods]
impl Session {
    pub fn get_entity(&self, py: Python<'_>, key: String) -> PyResult<Option<PyObject>> {
        let map = self.identity_map.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Lock poisoned: {}", e))
        })?;
        Ok(map.get(&key).map(|obj| obj.clone_ref(py)))
    }

    pub fn set_entity(&self, py: Python<'_>, key: String, entity: PyObject) -> PyResult<()> {
        let mut map = self.identity_map.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Lock poisoned: {}", e))
        })?;
        map.insert(key.clone(), entity.clone_ref(py));
        Ok(())
    }

    pub fn remove_entity(&self, _py: Python<'_>, key: String) -> PyResult<()> {
        let mut map = self.identity_map.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Lock poisoned: {}", e))
        })?;
        map.remove(&key);

        // Also remove snapshot when entity is removed from identity map
        let mut tracker = self.dirty_tracker.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Lock poisoned: {}", e))
        })?;
        tracker.remove_snapshot(&key);
        Ok(())
    }

    pub fn clear_identity_map(&self) -> PyResult<()> {
        let mut map = self.identity_map.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Lock poisoned: {}", e))
        })?;
        map.clear();

        let mut tracker = self.dirty_tracker.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Lock poisoned: {}", e))
        })?;
        tracker.snapshots.clear();
        Ok(())
    }

    pub fn get_stats(&self) -> PyResult<HashMap<String, usize>> {
        let map = self.identity_map.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Lock poisoned: {}", e))
        })?;
        let tracker = self.dirty_tracker.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Lock poisoned: {}", e))
        })?;

        let mut stats = HashMap::new();
        stats.insert("identity_map_size".to_string(), map.len());
        stats.insert("snapshots_count".to_string(), tracker.snapshots.len());
        Ok(stats)
    }

    pub fn commit<'p>(&self, py: Python<'p>) -> PyResult<Bound<'p, PyAny>> {
        let transaction = self.transaction.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = transaction.lock().await;
            if let Some(tx) = guard.take() {
                tx.commit()
                    .await
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            }
            Ok(())
        })
    }

    pub fn rollback<'p>(&self, py: Python<'p>) -> PyResult<Bound<'p, PyAny>> {
        let transaction = self.transaction.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = transaction.lock().await;
            if let Some(tx) = guard.take() {
                tx.rollback()
                    .await
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
            }
            Ok(())
        })
    }
}

impl Session {
    pub fn snapshot_entity_internal(
        &self,
        key: String,
        table_name: String,
        values: HashMap<String, QueryValue>,
    ) -> PyResult<()> {
        let mut tracker = self.dirty_tracker.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Lock poisoned: {}", e))
        })?;
        tracker.take_snapshot(key, table_name, values);
        Ok(())
    }
}

pub async fn begin_session(pool: AnyPool) -> BridgeResult<Session> {
    let tx = pool.begin().await?;
    Ok(Session {
        pool,
        transaction: Arc::new(TokioMutex::new(Some(tx))),
        identity_map: Arc::new(Mutex::new(HashMap::new())),
        dirty_tracker: Arc::new(Mutex::new(DirtyTracker::new())),
    })
}
