pub mod engine;
pub mod error;
pub mod ffi;
pub mod schema;
pub mod telemetry;

pub use error::{BridgeError, BridgeResult};

use pyo3::prelude::*;

#[pymodule]
fn bridge_rs(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    engine::pool_manager::set_pool_manager(engine::pool_manager::PoolManager::new()).expect(
        "bug: pool manager already set",
    );

    ffi::register_module(m)
}
