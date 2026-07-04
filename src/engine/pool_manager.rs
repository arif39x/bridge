use crate::error::{BridgeError, BridgeResult, DiagnosticInfo};
use sqlx::AnyPool;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

struct PoolManagerInner {
    pools: HashMap<String, AnyPool>,
    urls: HashMap<String, String>,
    default_key: Option<String>,
}

#[derive(Clone)]
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

static POOL_MANAGER: once_cell::sync::Lazy<PoolManager> =
    once_cell::sync::Lazy::new(PoolManager::new);

pub fn pool_manager() -> &'static PoolManager {
    &POOL_MANAGER
}
