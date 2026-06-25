use crate::engine::metadata::REGISTRY;
use crate::error::BridgeResult;
use arrow::array::{ArrayRef, BooleanBuilder, Float64Builder, Int64Builder, StringBuilder};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use arrow_ipc::writer::StreamWriter;
use sqlx::{any::AnyRow, Column, Row};
use std::sync::Arc;

pub fn rows_to_arrow_ipc(table_name: &str, rows: &[AnyRow]) -> BridgeResult<Vec<u8>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let registry_guard = REGISTRY.read().unwrap();
    let mapping = registry_guard.mappings.get(table_name);

    let first_row = &rows[0];
    let mut fields = Vec::new();
    let mut builders: Vec<Box<dyn arrow::array::ArrayBuilder>> = Vec::new();

    for column in first_row.columns() {
        let name = column.name();
        let meta = mapping.and_then(|m| m.columns.get(name));

        let data_type = if let Some(m) = meta {
            match m.data_type.to_lowercase().as_str() {
                "int" | "bigint" | "integer" => DataType::Int64,
                "bool" | "boolean" => DataType::Boolean,
                "float" | "double" | "real" => DataType::Float64,
                "uuid" | "datetime" | "timestamp" => DataType::Utf8,
                _ => DataType::Utf8,
            }
        } else {
            DataType::Utf8
        };

        fields.push(Field::new(name, data_type.clone(), true));
        builders.push(match data_type {
            DataType::Int64 => Box::new(Int64Builder::with_capacity(rows.len())),
            DataType::Boolean => Box::new(BooleanBuilder::with_capacity(rows.len())),
            DataType::Float64 => Box::new(Float64Builder::with_capacity(rows.len())),
            _ => Box::new(StringBuilder::with_capacity(rows.len(), rows.len() * 32)),
        });
    }

    let schema = Arc::new(Schema::new(fields));

    for row in rows {
        for (i, column) in row.columns().iter().enumerate() {
            let name = column.name();
            let builder = &mut builders[i];

            if let Some(meta) = mapping.and_then(|m| m.columns.get(name)) {
                match meta.data_type.to_lowercase().as_str() {
                    "int" | "bigint" | "integer" => {
                        let b = builder.as_any_mut().downcast_mut::<Int64Builder>().unwrap();
                        if let Ok(val) = row.try_get::<i64, _>(name) {
                            b.append_value(val);
                        } else {
                            b.append_null();
                        }
                    }
                    "bool" | "boolean" => {
                        let b = builder
                            .as_any_mut()
                            .downcast_mut::<BooleanBuilder>()
                            .unwrap();
                        if let Ok(val) = row.try_get::<bool, _>(name) {
                            b.append_value(val);
                        } else {
                            b.append_null();
                        }
                    }
                    "float" | "double" | "real" => {
                        let b = builder
                            .as_any_mut()
                            .downcast_mut::<Float64Builder>()
                            .unwrap();
                        if let Ok(val) = row.try_get::<f64, _>(name) {
                            b.append_value(val);
                        } else {
                            b.append_null();
                        }
                    }
                    "uuid" | "datetime" | "timestamp" => {
                        let b = builder
                            .as_any_mut()
                            .downcast_mut::<StringBuilder>()
                            .unwrap();
                        if let Ok(val) = row.try_get::<String, _>(name) {
                            b.append_value(val);
                        } else {
                            b.append_null();
                        }
                    }
                    _ => {
                        let b = builder
                            .as_any_mut()
                            .downcast_mut::<StringBuilder>()
                            .unwrap();
                        if let Ok(val) = row.try_get::<String, _>(name) {
                            b.append_value(val);
                        } else {
                            b.append_null();
                        }
                    }
                }
            } else {
                let b = builder
                    .as_any_mut()
                    .downcast_mut::<StringBuilder>()
                    .unwrap();
                if let Ok(val) = row.try_get::<String, _>(name) {
                    b.append_value(val);
                } else {
                    b.append_null();
                }
            }
        }
    }

    let arrays: Vec<ArrayRef> = builders.into_iter().map(|mut b| b.finish()).collect();
    let batch = RecordBatch::try_new(schema.clone(), arrays).unwrap();

    let mut buffer = Vec::new();
    {
        let mut writer = StreamWriter::try_new(&mut buffer, &schema).unwrap();
        writer.write(&batch).unwrap();
        writer.finish().unwrap();
    }

    Ok(buffer)
}
