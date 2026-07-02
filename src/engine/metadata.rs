use once_cell::sync::Lazy;
use pyo3::prelude::*;
use std::collections::HashMap;
use std::sync::RwLock;

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
    pub locked: bool,
}

impl MetadataRegistry {
    pub fn new() -> Self {
        Self {
            mappings: HashMap::new(),
            locked: false,
        }
    }
}

pub static REGISTRY: Lazy<RwLock<MetadataRegistry>> =
    Lazy::new(|| RwLock::new(MetadataRegistry::new()));

fn poison_err() -> PyErr {
    pyo3::exceptions::PyRuntimeError::new_err("Metadata registry lock poisoned")
}

#[pyfunction]
pub fn register_entity(
    table_name: String,
    columns: Vec<(String, String, bool, bool)>,
) -> PyResult<()> {
    let mut registry = REGISTRY.write().map_err(|_| poison_err())?;
    if registry.locked {
        return Err(pyo3::exceptions::PyRuntimeError::new_err(
            "Metadata registry is locked. Cannot register new entities after initialization.",
        ));
    }

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

    registry.mappings.insert(
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
    let mut registry = REGISTRY.write().map_err(|_| poison_err())?;
    registry.locked = true;
    Ok(())
}
