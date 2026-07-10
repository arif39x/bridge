use crate::engine::metadata::{get_registry, ColumnMetadata};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use sqlx::{any::AnyRow, Column, Row};
use std::collections::HashMap;

pub fn hydrate_row<'py>(
    py: Python<'py>,
    table_name: &str,
    row: &AnyRow,
) -> PyResult<Bound<'py, PyDict>> {
    let registry = get_registry().ok_or_else(|| {
        pyo3::exceptions::PyRuntimeError::new_err("Registry not initialized")
    })?;
    let mapping = registry.mappings.get(table_name);

    let dict = PyDict::new_bound(py);

    for column in row.columns() {
        let name = column.name();
        let meta = mapping.and_then(|m| m.columns.get(name));

        let val = if let Some(meta) = meta {
            coerce_value(py, row, name, meta)?
        } else {
            let raw_val: String = row.try_get(name).unwrap_or_default();
            raw_val.to_object(py)
        };

        dict.set_item(name, val)?;
    }

    Ok(dict)
}

fn coerce_value(
    py: Python<'_>,
    row: &AnyRow,
    name: &str,
    meta: &ColumnMetadata,
) -> PyResult<PyObject> {
    // Basic type coercion based on metadata data_type.

    match meta.data_type.to_lowercase().as_str() {
        "text" | "str" | "string" => {
            if let Ok(val) = row.try_get::<String, _>(name) {
                Ok(val.to_object(py))
            } else {
                Ok(py.None())
            }
        }
        "uuid" => {
            if let Ok(val_str) = row.try_get::<String, _>(name) {
                let uuid_module = py.import_bound("uuid")?;
                if let Ok(uuid_obj) = uuid_module.call_method1("UUID", (val_str.clone(),)) {
                    Ok(uuid_obj.to_object(py))
                } else {
                    Ok(val_str.to_object(py))
                }
            } else {
                Ok(py.None())
            }
        }
        "datetime" | "timestamp" => {
            if let Ok(val_str) = row.try_get::<String, _>(name) {
                let datetime_module = py.import_bound("datetime")?;
                let datetime_cls = datetime_module.getattr("datetime")?;
                if let Ok(dt_obj) = datetime_cls.call_method1("fromisoformat", (val_str.clone(),)) {
                    Ok(dt_obj.to_object(py))
                } else {
                    Ok(val_str.to_object(py))
                }
            } else {
                Ok(py.None())
            }
        }
        "int" | "bigint" | "integer" => {
            if let Ok(val) = row.try_get::<i64, _>(name) {
                Ok(val.to_object(py))
            } else if let Ok(val_str) = row.try_get::<String, _>(name) {
                let val: i64 = val_str.parse().unwrap_or(0);
                Ok(val.to_object(py))
            } else {
                Ok(py.None())
            }
        }
        "bool" | "boolean" => {
            if let Ok(val) = row.try_get::<bool, _>(name) {
                Ok(val.to_object(py))
            } else if let Ok(val_i) = row.try_get::<i32, _>(name) {
                Ok((val_i != 0).to_object(py))
            } else if let Ok(val_i) = row.try_get::<i64, _>(name) {
                Ok((val_i != 0).to_object(py))
            } else if let Ok(val_s) = row.try_get::<String, _>(name) {
                let normalized = val_s.to_lowercase();
                let val = normalized == "true" || normalized == "1" || normalized == "t";
                Ok(val.to_object(py))
            } else {
                Ok(py.None())
            }
        }
        "float" | "double" | "real" => {
            if let Ok(val) = row.try_get::<f64, _>(name) {
                Ok(val.to_object(py))
            } else if let Ok(val_str) = row.try_get::<String, _>(name) {
                let val: f64 = val_str.parse().unwrap_or(0.0);
                Ok(val.to_object(py))
            } else {
                Ok(py.None())
            }
        }
        "json" | "jsonb" => {
            if let Ok(val_str) = row.try_get::<String, _>(name) {
                let json_module = py.import_bound("json")?;
                let parsed = json_module
                    .call_method1("loads", (val_str,))?;
                Ok(parsed.to_object(py))
            } else {
                Ok(py.None())
            }
        }
        "bytes" | "blob" | "bytea" => {
            if let Ok(val) = row.try_get::<Vec<u8>, _>(name) {
                Ok(val.to_object(py))
            } else {
                Ok(py.None())
            }
        }
        _ => {
            // Default to string representation
            if let Ok(val) = row.try_get::<String, _>(name) {
                Ok(val.to_object(py))
            } else {
                Ok(py.None())
            }
        }
    }
}
