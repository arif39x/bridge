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
