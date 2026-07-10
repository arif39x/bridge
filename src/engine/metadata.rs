use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use pyo3::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMetadata {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary_key: bool,
}

#[derive(Debug, Clone)]
pub struct EntityMapping {
    pub table_name: String,
    pub columns: HashMap<String, ColumnMetadata>,
}

pub struct MetadataRegistry {
    pub mappings: HashMap<String, EntityMapping>,
}

// Build-phase storage: accessed only during registration (before locking)
static BUILD_MAPPINGS: Lazy<Mutex<HashMap<String, EntityMapping>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// Final frozen registry: set once by lock_registry, then read-only.
// No lock contention or poison risk for readers.
static REGISTRY: OnceCell<MetadataRegistry> = OnceCell::new();

/// Returns a reference to the frozen registry, or None if not yet locked.
pub fn get_registry() -> Option<&'static MetadataRegistry> {
    REGISTRY.get()
}

#[pyfunction]
pub fn register_entity(
    table_name: String,
    columns: Vec<(String, String, bool, bool)>,
) -> PyResult<()> {
    if REGISTRY.get().is_some() {
        return Err(pyo3::exceptions::PyRuntimeError::new_err(
            "Metadata registry is locked. Cannot register new entities after initialization.",
        ));
    }

    let mut mappings = BUILD_MAPPINGS.lock().map_err(|_| {
        pyo3::exceptions::PyRuntimeError::new_err("Build mappings lock poisoned")
    })?;

    let mut col_map = HashMap::new();
    for (name, data_type, is_nullable, is_primary_key) in columns {
        col_map.insert(
            name.clone(),
            ColumnMetadata {
                name,
                data_type,
                is_nullable,
                is_primary_key,
            },
        );
    }

    mappings.insert(
        table_name.clone(),
        EntityMapping {
            table_name,
            columns: col_map,
        },
    );
    Ok(())
}

#[pyfunction]
pub fn lock_registry() -> PyResult<()> {
    let mut mappings = BUILD_MAPPINGS.lock().map_err(|_| {
        pyo3::exceptions::PyRuntimeError::new_err("Build mappings lock poisoned")
    })?;

    let registry = MetadataRegistry {
        mappings: std::mem::take(&mut *mappings),
    };

    REGISTRY.set(registry).map_err(|_| {
        pyo3::exceptions::PyRuntimeError::new_err("Registry is already locked")
    })
}
