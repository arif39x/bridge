use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "allow-raw-sql")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawExpression {
    pub sql: String,
    pub params: Vec<QueryValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum QueryValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Uuid(Uuid),
    DateTime(DateTime<Utc>),
    Json(serde_json::Value),
    Bytes(Vec<u8>),
    #[cfg(feature = "allow-raw-sql")]
    Raw(RawExpression),
    Null,
}

pub struct Query {
    pub table: String,
    pub selection: Option<Vec<String>>,
    pub filters: Vec<(String, QueryValue)>,
    pub limit: Option<i64>,
}
