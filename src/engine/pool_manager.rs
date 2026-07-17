use crate::error::{BridgeError, BridgeResult, DiagnosticInfo};
use sqlx::AnyPool;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

#[derive(Debug)]
struct PoolManagerInner {
    pools: HashMap<String, AnyPool>,
    urls: HashMap<String, String>,
    default_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PoolManager {
    inner: Arc<RwLock<PoolManagerInner>>,
}

impl PoolManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(PoolManagerInner {
                pools: HashMap::new(),
                urls: HashMap::new(),
                default_key: None,
            })),
        }
    }

    fn poison_err() -> BridgeError {
        BridgeError::Internal(
            "Pool manager lock poisoned".into(),
            DiagnosticInfo::default(),
        )
    }

    pub fn register(&self, key: String, pool: AnyPool, url: String) -> BridgeResult<()> {
        let mut guard = self.inner.write().map_err(|_| Self::poison_err())?;
        guard.pools.insert(key.clone(), pool);
        guard.urls.insert(key, url);
        Ok(())
    }

    pub fn get(&self, key: Option<&str>) -> BridgeResult<Option<(AnyPool, String)>> {
        let guard = self.inner.read().map_err(|_| Self::poison_err())?;
        let actual_key = match key {
            Some(k) => k.to_string(),
            None => match &guard.default_key {
                Some(k) => k.clone(),
                None => return Ok(None),
            },
        };
        match (guard.pools.get(&actual_key), guard.urls.get(&actual_key)) {
            (Some(pool), Some(url)) => Ok(Some((pool.clone(), url.clone()))),
            _ => Ok(None),
        }
    }

    pub fn set_default(&self, key: String) -> BridgeResult<()> {
        let mut guard = self.inner.write().map_err(|_| Self::poison_err())?;
        guard.default_key = Some(key);
        Ok(())
    }

    pub fn get_default_key(&self) -> BridgeResult<Option<String>> {
        let guard = self.inner.read().map_err(|_| Self::poison_err())?;
        Ok(guard.default_key.clone())
    }

    pub fn remove(&self, key: &str) -> BridgeResult<()> {
        let mut guard = self.inner.write().map_err(|_| Self::poison_err())?;
        guard.pools.remove(key);
        guard.urls.remove(key);
        if guard.default_key.as_deref() == Some(key) {
            guard.default_key = None;
        }
        Ok(())
    }

    pub fn contains(&self, key: &str) -> BridgeResult<bool> {
        let guard = self.inner.read().map_err(|_| Self::poison_err())?;
        Ok(guard.pools.contains_key(key))
    }
}



static POOL_MANAGER: OnceLock<PoolManager> = OnceLock::new();

pub fn init_pool_manager() -> &'static PoolManager {
    POOL_MANAGER.get_or_init(|| {
        tracing::info!("PoolManager initialized via init_pool_manager() fallback");
        PoolManager::new()
    })
}

pub fn pool_manager() -> &'static PoolManager {
    POOL_MANAGER.get().expect("BUG: PoolManager not initialized. Call init_pool_manager() during module setup.")
}

pub fn set_pool_manager(pm: PoolManager) -> Result<(), PoolManager> {
    POOL_MANAGER.set(pm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::any::AnyPoolOptions;

    async fn make_pool() -> AnyPool {
        sqlx::any::install_default_drivers();
        AnyPoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn register_and_get_by_key() {
        let pm = PoolManager::new();
        let pool = make_pool().await;
        pm.register("test_db".into(), pool.clone(), "sqlite::memory:".into()).unwrap();

        let result = pm.get(Some("test_db")).unwrap();
        assert!(result.is_some());
        let (retrieved, url) = result.unwrap();
        assert_eq!(url, "sqlite::memory:");
    }

    #[tokio::test]
    async fn get_nonexistent_key() {
        let pm = PoolManager::new();
        let result = pm.get(Some("no_such_db")).unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_without_key_and_no_default() {
        let pm = PoolManager::new();
        let result = pm.get(None).unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn default_key_operations() {
        let pm = PoolManager::new();
        let pool = make_pool().await;
        pm.register("primary".into(), pool.clone(), "sqlite::memory:".into()).unwrap();
        pm.set_default("primary".into()).unwrap();

        assert_eq!(pm.get_default_key().unwrap(), Some("primary".into()));

        let result = pm.get(None).unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn remove_key() {
        let pm = PoolManager::new();
        let pool = make_pool().await;
        pm.register("k".into(), pool.clone(), "sqlite::memory:".into()).unwrap();
        pm.set_default("k".into()).unwrap();

        assert!(pm.contains("k").unwrap());
        pm.remove("k").unwrap();
        assert!(!pm.contains("k").unwrap());
        assert!(pm.get_default_key().unwrap().is_none());
    }

    #[tokio::test]
    async fn contains_key() {
        let pm = PoolManager::new();
        assert!(!pm.contains("x").unwrap());
        let pool = make_pool().await;
        pm.register("x".into(), pool.clone(), "sqlite::memory:".into()).unwrap();
        assert!(pm.contains("x").unwrap());
    }
}
