use crate::error::{BridgeError, DiagnosticInfo};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Closed,
    Open,
    HalfOpen,
}

pub struct CircuitBreaker {
    max_failures: u32,
    reset_timeout: Duration,
    state: Mutex<CircuitBreakerState>,
}

struct CircuitBreakerState {
    current_state: State,
    failures: u32,
    last_failure_time: Option<Instant>,
}

impl CircuitBreaker {
    pub fn new(max_failures: u32, reset_timeout: Duration) -> Self {
        Self {
            max_failures,
            reset_timeout,
            state: Mutex::new(CircuitBreakerState {
                current_state: State::Closed,
                failures: 0,
                last_failure_time: None,
            }),
        }
    }

    pub async fn call<F, Fut, R>(&self, f: F) -> Result<R, BridgeError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<R, BridgeError>>,
    {
        self.before_call()?;
        let result = f().await;
        self.after_call(&result)?;
        result
    }

    fn before_call(&self) -> Result<(), BridgeError> {
        let mut state = self.state.lock().map_err(|e| {
            BridgeError::Internal(
                format!("Circuit breaker lock poisoned: {}", e),
                DiagnosticInfo::default(),
            )
        })?;
        match state.current_state {
            State::Closed => Ok(()),
            State::Open => {
                if let Some(last_failure) = state.last_failure_time {
                    if last_failure.elapsed() >= self.reset_timeout {
                        state.current_state = State::HalfOpen;
                        return Ok(());
                    }
                }
                Err(BridgeError::Internal(
                    "Circuit breaker is OPEN. Database calls are temporarily blocked to allow recovery.".to_string(),
                    DiagnosticInfo::default(),
                ))
            }
            State::HalfOpen => Ok(()),
        }
    }

    pub fn max_failures(&self) -> u32 {
        self.max_failures
    }

    pub fn reset_timeout(&self) -> Duration {
        self.reset_timeout
    }

    fn after_call<R>(&self, result: &Result<R, BridgeError>) -> Result<(), BridgeError> {
        let mut state = self.state.lock().map_err(|e| {
            BridgeError::Internal(
                format!("Circuit breaker lock poisoned: {}", e),
                DiagnosticInfo::default(),
            )
        })?;
        match result {
            Ok(_) => {
                state.failures = 0;
                state.current_state = State::Closed;
                state.last_failure_time = None;
            }
            Err(e) => match e {
                BridgeError::Database(_, _) | BridgeError::Internal(_, _) => {
                    state.failures += 1;
                    state.last_failure_time = Some(Instant::now());
                    if state.failures >= self.max_failures {
                        state.current_state = State::Open;
                    }
                }
                _ => {}
            },
        }
        Ok(())
    }
}

/// A registry of circuit breakers keyed by pool identifier (e.g., database URL).
/// Each pool gets its own circuit breaker so a failure in one database
/// does not affect queries against other databases.
pub struct CircuitBreakerRegistry {
    breakers: Mutex<HashMap<String, Arc<CircuitBreaker>>>,
    max_failures: u32,
    reset_timeout: Duration,
}

impl CircuitBreakerRegistry {
    pub fn new(max_failures: u32, reset_timeout: Duration) -> Self {
        Self {
            breakers: Mutex::new(HashMap::new()),
            max_failures,
            reset_timeout,
        }
    }

    fn poison_err() -> BridgeError {
        BridgeError::Internal(
            "Circuit breaker registry lock poisoned".into(),
            DiagnosticInfo::default(),
        )
    }

    pub fn get_or_create(&self, key: &str) -> Result<Arc<CircuitBreaker>, BridgeError> {
        let mut breakers = self.breakers.lock().map_err(|_| Self::poison_err())?;
        Ok(breakers
            .entry(key.to_string())
            .or_insert_with(|| {
                Arc::new(CircuitBreaker::new(self.max_failures, self.reset_timeout))
            })
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_breaker_initial_state() {
        let cb = CircuitBreaker::new(5, Duration::from_secs(30));
        assert_eq!(cb.max_failures(), 5);
        assert_eq!(cb.reset_timeout(), Duration::from_secs(30));
        assert!(cb.before_call().is_ok());
    }

    #[test]
    fn circuit_breaker_opens_after_max_failures() {
        let cb = CircuitBreaker::new(2, Duration::from_secs(60));
        assert!(cb.before_call().is_ok());

        let err_db = BridgeError::Database(
            sqlx::Error::Protocol("fail".into()),
            DiagnosticInfo::default(),
        );
        assert!(cb.after_call::<()>(&Err(err_db)).is_ok());
        assert!(cb.before_call().is_ok());

        let err_internal = BridgeError::Internal("oops".into(), DiagnosticInfo::default());
        assert!(cb.after_call::<()>(&Err(err_internal)).is_ok());

        let result = cb.before_call();
        assert!(result.is_err());
        if let Err(BridgeError::Internal(msg, _)) = result {
            assert!(msg.contains("OPEN"));
        } else {
            panic!("expected Internal error with OPEN message");
        }
    }

    #[test]
    fn circuit_breaker_resets_on_success() {
        let cb = CircuitBreaker::new(1, Duration::from_secs(60));
        let err = BridgeError::Database(
            sqlx::Error::Protocol("fail".into()),
            DiagnosticInfo::default(),
        );
        assert!(cb.after_call::<()>(&Err(err)).is_ok());
        assert!(cb.before_call().is_err());

        assert!(cb.after_call::<()>(&Ok(())).is_ok());
        assert!(cb.before_call().is_ok());
    }

    #[test]
    fn circuit_breaker_ignores_validation_errors() {
        let cb = CircuitBreaker::new(1, Duration::from_secs(60));
        let err = BridgeError::Validation("bad input".into(), DiagnosticInfo::default());
        assert!(cb.after_call::<()>(&Err(err)).is_ok());
        assert!(cb.before_call().is_ok());
    }

    #[test]
    fn circuit_breaker_half_open_transition() {
        let cb = CircuitBreaker::new(1, Duration::from_secs(0));
        let err = BridgeError::Database(
            sqlx::Error::Protocol("fail".into()),
            DiagnosticInfo::default(),
        );
        assert!(cb.after_call::<()>(&Err(err)).is_ok());

        std::thread::sleep(Duration::from_millis(1));
        assert!(cb.before_call().is_ok());
    }

    #[test]
    fn circuit_breaker_registry_get_or_create() {
        let reg = CircuitBreakerRegistry::new(3, Duration::from_secs(30));
        let cb1 = reg.get_or_create("db1").unwrap();
        let cb2 = reg.get_or_create("db1").unwrap();
        assert!(Arc::ptr_eq(&cb1, &cb2));

        let cb3 = reg.get_or_create("db2").unwrap();
        assert!(!Arc::ptr_eq(&cb1, &cb3));
    }
}
