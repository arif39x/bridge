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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn query_value_variants() {
        let s = QueryValue::String("hello".into());
        assert_eq!(s, QueryValue::String("hello".into()));
        assert_ne!(s, QueryValue::String("world".into()));

        let i = QueryValue::Int(42);
        assert_eq!(i, QueryValue::Int(42));
        assert_ne!(i, QueryValue::Int(0));

        let f = QueryValue::Float(3.14);
        assert_eq!(f, QueryValue::Float(3.14));

        let b = QueryValue::Bool(true);
        assert_eq!(b, QueryValue::Bool(true));
        assert_ne!(b, QueryValue::Bool(false));

        let u = QueryValue::Uuid(uuid::Uuid::nil());
        assert_eq!(u, QueryValue::Uuid(uuid::Uuid::nil()));

        let dt = QueryValue::DateTime(Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap());
        assert_eq!(
            dt,
            QueryValue::DateTime(Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap())
        );

        let j = QueryValue::Json(serde_json::json!({"key": "value"}));
        assert_eq!(j, QueryValue::Json(serde_json::json!({"key": "value"})));

        let bytes = QueryValue::Bytes(vec![0, 1, 2]);
        assert_eq!(bytes, QueryValue::Bytes(vec![0, 1, 2]));

        assert_eq!(QueryValue::Null, QueryValue::Null);
    }

    #[test]
    fn query_value_debug() {
        let s = format!("{:?}", QueryValue::String("test".into()));
        assert!(s.contains("String") && s.contains("test"));

        let n = format!("{:?}", QueryValue::Null);
        assert_eq!(n, "Null");
    }
}

pub struct Query {
    pub table: String,
    pub selection: Option<Vec<String>>,
    pub filters: Vec<(String, QueryValue)>,
    pub limit: Option<i64>,
}
