use std::fmt;
use thiserror::Error;

#[derive(Debug, Clone, Default)]
pub struct DiagnosticInfo {
    pub breadcrumbs: Vec<String>,
    pub sql: Option<String>,
    pub params: Option<String>,
    pub trace_id: Option<String>,
}

impl fmt::Display for DiagnosticInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.breadcrumbs.is_empty() {
            writeln!(f, "Breadcrumbs: {}", self.breadcrumbs.join(" -> "))?;
        }
        if let Some(sql) = &self.sql {
            writeln!(f, "SQL: {}", sql)?;
        }
        if let Some(params) = &self.params {
            writeln!(f, "Params: {}", params)?;
        }
        if let Some(trace_id) = &self.trace_id {
            writeln!(f, "Trace ID: {}", trace_id)?;
        }
        Ok(())
    }
}

/// Unified error enum for the entire Bridge library.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BridgeError {
    #[error("Database error: {0}\n{1}")]
    Database(sqlx::Error, DiagnosticInfo),

    #[error("Serialization error: {0}\n{1}")]
    Serialization(serde_json::Error, DiagnosticInfo),

    #[error("Validation error: {0}\n{1}")]
    Validation(String, DiagnosticInfo),

    #[error("Resource not found: {0}\n{1}")]
    NotFound(String, DiagnosticInfo),

    #[error("Configuration error: {0}\n{1}")]
    Configuration(String, DiagnosticInfo),

    #[error("Internal error: {0}\n{1}")]
    Internal(String, DiagnosticInfo),

    #[error("Type mismatch error: field {field}, expected {expected}, got {got}\n{info}")]
    TypeMismatch {
        field: String,
        expected: String,
        got: String,
        info: DiagnosticInfo,
    },
}

impl From<sqlx::Error> for BridgeError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err, DiagnosticInfo::default())
    }
}

impl From<serde_json::Error> for BridgeError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err, DiagnosticInfo::default())
    }
}

impl BridgeError {
    pub fn add_breadcrumb(mut self, crumb: &str) -> Self {
        let info = match self {
            Self::Database(_, ref mut info) => info,
            Self::Serialization(_, ref mut info) => info,
            Self::Validation(_, ref mut info) => info,
            Self::NotFound(_, ref mut info) => info,
            Self::Configuration(_, ref mut info) => info,
            Self::Internal(_, ref mut info) => info,
            Self::TypeMismatch { ref mut info, .. } => info,
        };
        info.breadcrumbs.push(crumb.to_string());
        self
    }

    pub fn with_sql(mut self, sql: String, params: Option<String>) -> Self {
        let info = match self {
            Self::Database(_, ref mut info) => info,
            Self::Serialization(_, ref mut info) => info,
            Self::Validation(_, ref mut info) => info,
            Self::NotFound(_, ref mut info) => info,
            Self::Configuration(_, ref mut info) => info,
            Self::Internal(_, ref mut info) => info,
            Self::TypeMismatch { ref mut info, .. } => info,
        };
        info.sql = Some(sql);
        info.params = params;
        self
    }
}

/// Type alias for Results returned by Bridge functions.
pub type BridgeResult<T> = Result<T, BridgeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_info_default() {
        let info = DiagnosticInfo::default();
        assert!(info.breadcrumbs.is_empty());
        assert!(info.sql.is_none());
        assert!(info.params.is_none());
        assert!(info.trace_id.is_none());
    }

    #[test]
    fn diagnostic_info_display_empty() {
        let info = DiagnosticInfo::default();
        let s = info.to_string();
        assert_eq!(s, "");
    }

    #[test]
    fn diagnostic_info_display_with_breadcrumbs() {
        let mut info = DiagnosticInfo::default();
        info.breadcrumbs.push("step1".into());
        info.breadcrumbs.push("step2".into());
        let s = info.to_string();
        assert!(s.contains("step1"));
        assert!(s.contains("step2"));
        assert!(s.contains("->"));
    }

    #[test]
    fn diagnostic_info_display_with_sql() {
        let mut info = DiagnosticInfo::default();
        info.sql = Some("SELECT * FROM t".into());
        let s = info.to_string();
        assert!(s.contains("SELECT * FROM t"));
    }

    #[test]
    fn bridge_error_add_breadcrumb() {
        let err = BridgeError::Internal("test".into(), DiagnosticInfo::default())
            .add_breadcrumb("crumb1")
            .add_breadcrumb("crumb2");
        match err {
            BridgeError::Internal(_, ref info) => {
                assert_eq!(info.breadcrumbs, vec!["crumb1", "crumb2"]);
            }
            _ => panic!("expected Internal variant"),
        }
    }

    #[test]
    fn bridge_error_add_breadcrumb_on_all_variants() {
        let make = |e: BridgeError| e.add_breadcrumb("x");
        assert!(matches!(make(BridgeError::Database(
            sqlx::Error::Protocol("".into()),
            DiagnosticInfo::default()
        )), BridgeError::Database(_, _)));
        assert!(matches!(
            make(BridgeError::Validation("".into(), DiagnosticInfo::default())),
            BridgeError::Validation(_, _)
        ));
        assert!(matches!(
            make(BridgeError::NotFound("".into(), DiagnosticInfo::default())),
            BridgeError::NotFound(_, _)
        ));
        assert!(matches!(
            make(BridgeError::Configuration("".into(), DiagnosticInfo::default())),
            BridgeError::Configuration(_, _)
        ));
        assert!(matches!(
            make(BridgeError::Internal("".into(), DiagnosticInfo::default())),
            BridgeError::Internal(_, _)
        ));
    }

    #[test]
    fn bridge_error_with_sql() {
        let err = BridgeError::Validation("bad".into(), DiagnosticInfo::default())
            .with_sql("SELECT *".into(), Some("[]".into()));
        match err {
            BridgeError::Validation(_, ref info) => {
                assert_eq!(info.sql.as_deref(), Some("SELECT *"));
                assert_eq!(info.params.as_deref(), Some("[]"));
            }
            _ => panic!("expected Validation variant"),
        }
    }

    #[test]
    fn bridge_error_from_sqlx() {
        let sqlx_err = sqlx::Error::Protocol("conn lost".into());
        let bridge_err: BridgeError = sqlx_err.into();
        assert!(matches!(bridge_err, BridgeError::Database(_, _)));
    }

    #[test]
    fn bridge_error_display() {
        let err = BridgeError::NotFound("user_42".into(), DiagnosticInfo::default());
        let s = err.to_string();
        assert!(s.contains("user_42"));
        assert!(s.contains("not found"));
    }

    #[test]
    fn bridge_error_typemismatch_display() {
        let err = BridgeError::TypeMismatch {
            field: "age".into(),
            expected: "int".into(),
            got: "string".into(),
            info: DiagnosticInfo::default(),
        };
        let s = err.to_string();
        assert!(s.contains("age"));
        assert!(s.contains("int"));
        assert!(s.contains("string"));
    }
}
