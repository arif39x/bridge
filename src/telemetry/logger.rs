// every query in this file uses bound parameters.
use once_cell::sync::Lazy;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::sync::{Once, RwLock};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

static INIT: Once = Once::new();
static SLOW_QUERY_THRESHOLD: RwLock<u64> = RwLock::new(100);
static PYTHON_LOGGER: RwLock<Option<PyObject>> = RwLock::new(None);

pub struct TelemetryEvent {
    pub sql: String,
    pub duration_micros: u64,
    pub operation: String,
    pub table: String,
}

pub fn configure_logging(level: &str, slow_query_ms: u64) {
    INIT.call_once(|| {
        let tracing_level = match level.to_lowercase().as_str() {
            "debug" => Level::DEBUG,
            "warn" => Level::WARN,
            "error" => Level::ERROR,
            _ => Level::INFO,
        };

        let subscriber = FmtSubscriber::builder()
            .with_max_level(tracing_level)
            .finish();

        let _ = tracing::subscriber::set_global_default(subscriber);
    });

    if let Ok(mut threshold) = SLOW_QUERY_THRESHOLD.write() {
        *threshold = slow_query_ms;
    }
}

pub fn set_python_logger(logger: PyObject) {
    if let Ok(mut guard) = PYTHON_LOGGER.write() {
        *guard = Some(logger);
    }
}

/// Dispatches a telemetry event to the registered Python logger.
/// Rust Spans into Python Telemetry.
pub fn emit_telemetry(event: TelemetryEvent) {
    let slow_threshold = SLOW_QUERY_THRESHOLD.read().map(|g| *g).unwrap_or(100);

    // Rust-side structured logging
    if event.duration_micros > slow_threshold * 1000 {
        info!(
            "[Bridge SLOW QUERY] {} | {}μs | table={}",
            event.sql, event.duration_micros, event.table
        );
    }

    // Python-side bridge
    Python::with_gil(|py| {
        if let Ok(guard) = PYTHON_LOGGER.read() {
        if let Some(logger) = guard.as_ref() {
            let dict = PyDict::new_bound(py);
            let _ = dict.set_item("sql", &event.sql);
            let _ = dict.set_item("duration_micros", event.duration_micros);
            let _ = dict.set_item("operation", &event.operation);
            let _ = dict.set_item("table", &event.table);

            // Call the Python logger's 'handle_telemetry' method
            let _ = logger.call_method_bound(py, "handle_telemetry", (dict,), None);
        }
        }
    });
}
