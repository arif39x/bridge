use crate::engine::metadata::ColumnMetadata;
use crate::engine::query::QueryValue;
use crate::error::{BridgeError, DiagnosticInfo};
use pyo3::prelude::*;

pub fn coerce_py_value(
    py_val: &Bound<'_, PyAny>,
    meta: &ColumnMetadata,
    table_name: &str,
) -> Result<QueryValue, BridgeError> {
    if py_val.is_none() {
        if meta.is_nullable {
            return Ok(QueryValue::Null);
        }
        return Err(BridgeError::TypeMismatch {
            field: format!("{}.{}", table_name, meta.name),
            expected: meta.data_type.clone(),
            got: "None".to_string(),
            info: DiagnosticInfo::default(),
        });
    }

    let py = py_val.py();

    match meta.data_type.to_lowercase().as_str() {
        "text" | "str" | "varchar" => {
            if py_val.is_instance_of::<pyo3::types::PyBool>() {
                return Err(type_error(table_name, meta, "bool"));
            }
            py_val
                .extract::<String>()
                .map(QueryValue::String)
                .map_err(|_| {
                    type_error(
                        table_name,
                        meta,
                        py_val
                            .get_type()
                            .name()
                            .unwrap_or(std::borrow::Cow::Borrowed("unknown"))
                            .to_string()
                            .as_str(),
                    )
                })
        }
        "int" | "bigint" | "integer" => {
            if py_val.is_instance_of::<pyo3::types::PyBool>() {
                return Err(type_error(table_name, meta, "bool"));
            }
            py_val
                .extract::<i64>()
                .map(QueryValue::Int)
                .map_err(|_| type_error(table_name, meta, "non-integer"))
        }
        "bool" | "boolean" => py_val
            .extract::<bool>()
            .map(QueryValue::Bool)
            .map_err(|_| type_error(table_name, meta, "non-bool")),
        "float" | "double precision" | "real" | "double" => py_val
            .extract::<f64>()
            .map(QueryValue::Float)
            .map_err(|_| type_error(table_name, meta, "non-float")),
        "uuid" => {
            let s: String = if py_val.is_instance_of::<pyo3::types::PyString>() {
                py_val
                    .extract()
                    .map_err(|_| type_error(table_name, meta, "non-string"))?
            } else {
                py_val.to_string()
            };
            uuid::Uuid::parse_str(&s)
                .map(QueryValue::Uuid)
                .map_err(|_| BridgeError::TypeMismatch {
                    field: format!("{}.{}", table_name, meta.name),
                    expected: "UUID string".to_string(),
                    got: s,
                    info: DiagnosticInfo::default(),
                })
        }
        "datetime" | "timestamp" | "timestamptz" => {
            if let Ok(s) = py_val
                .call_method0("isoformat")
                .and_then(|r| r.extract::<String>())
            {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&s) {
                    return Ok(QueryValue::DateTime(dt.with_timezone(&chrono::Utc)));
                }
            }
            if let Ok(s) = py_val.extract::<String>() {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&s) {
                    return Ok(QueryValue::DateTime(dt.with_timezone(&chrono::Utc)));
                }
            }
            Err(type_error(table_name, meta, "invalid datetime"))
        }
        "json" | "jsonb" => {
            let json_module = py.import_bound("json").map_err(|_| {
                BridgeError::Internal(
                    "Failed to import json module".to_string(),
                    DiagnosticInfo::default(),
                )
            })?;
            if let Ok(s) = json_module
                .call_method1("dumps", (py_val,))
                .and_then(|r| r.extract::<String>())
            {
                if let Ok(v) = serde_json::from_str(&s) {
                    return Ok(QueryValue::Json(v));
                }
            }
            Err(type_error(table_name, meta, "invalid json"))
        }
        _ => {
            // Fallback to basic string conversion for unknown types
            // (Raw SQL expressions are intercepted at the py_to_query_value level instead)

            if let Ok(s) = py_val.extract::<String>() {
                Ok(QueryValue::String(s))
            } else {
                Ok(QueryValue::String(py_val.to_string()))
            }
        }
    }
}

fn type_error(table_name: &str, meta: &ColumnMetadata, got: &str) -> BridgeError {
    BridgeError::TypeMismatch {
        field: format!("{}.{}", table_name, meta.name),
        expected: meta.data_type.clone(),
        got: got.to_string(),
        info: DiagnosticInfo::default(),
    }
}
