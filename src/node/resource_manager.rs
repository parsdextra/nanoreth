//! Resource management for RPC operations to prevent blocking S3 syncing

use std::sync::Arc;
use tokio::sync::{Semaphore, OwnedSemaphorePermit};
use tracing::{debug, warn};

/// Resource manager to control concurrent RPC operations and prevent resource exhaustion
#[derive(Debug, Clone)]
pub struct ResourceManager {
    /// Semaphore to limit concurrent RPC operations
    rpc_semaphore: Arc<Semaphore>,
    /// Dedicated thread pool for blocking operations
    blocking_pool: Option<Arc<rayon::ThreadPool>>,
}

impl ResourceManager {
    /// Create a new resource manager with the specified limits
    pub fn new(max_concurrent_rpc: usize, blocking_pool_size: Option<usize>) -> Self {
        let rpc_semaphore = Arc::new(Semaphore::new(max_concurrent_rpc));
        
        let blocking_pool = blocking_pool_size.map(|size| {
            Arc::new(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(size)
                    .thread_name(|i| format!("rpc-blocking-{}", i))
                    .build()
                    .expect("Failed to create blocking thread pool")
            )
        });

        debug!(
            max_concurrent_rpc = max_concurrent_rpc,
            blocking_pool_size = blocking_pool_size,
            "Created resource manager"
        );

        Self {
            rpc_semaphore,
            blocking_pool,
        }
    }

    /// Acquire a permit for RPC operations
    /// 
    /// This should be called before performing resource-intensive RPC operations
    /// to prevent system overload
    pub async fn acquire_rpc_permit(&self) -> Result<OwnedSemaphorePermit, tokio::sync::AcquireError> {
        debug!("Acquiring RPC permit, available: {}", self.rpc_semaphore.available_permits());
        
        match self.rpc_semaphore.clone().acquire_owned().await {
            Ok(permit) => {
                debug!("RPC permit acquired, remaining: {}", self.rpc_semaphore.available_permits());
                Ok(permit)
            }
            Err(e) => {
                warn!("Failed to acquire RPC permit: {}", e);
                Err(e)
            }
        }
    }

    /// Execute a blocking operation on the dedicated thread pool
    /// 
    /// If no dedicated thread pool is configured, falls back to tokio::task::spawn_blocking
    pub async fn execute_blocking<F, R>(&self, f: F) -> Result<R, tokio::task::JoinError>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        if let Some(pool) = &self.blocking_pool {
            let pool = pool.clone();
            tokio::task::spawn_blocking(move || {
                let (tx, rx) = std::sync::mpsc::channel();
                pool.spawn(move || {
                    let result = f();
                    let _ = tx.send(result);
                });
                rx.recv().expect("Thread pool task failed")
            }).await
        } else {
            tokio::task::spawn_blocking(f).await
        }
    }

    /// Get the number of available RPC permits
    pub fn available_rpc_permits(&self) -> usize {
        self.rpc_semaphore.available_permits()
    }

    /// Check if the blocking thread pool is enabled
    pub fn has_blocking_pool(&self) -> bool {
        self.blocking_pool.is_some()
    }
}

impl Default for ResourceManager {
    fn default() -> Self {
        Self::new(50, Some(4))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_resource_manager_permits() {
        let manager = ResourceManager::new(2, None);
        
        // Should be able to acquire permits up to the limit
        let _permit1 = manager.acquire_rpc_permit().await.unwrap();
        let _permit2 = manager.acquire_rpc_permit().await.unwrap();
        
        // Third permit should be pending
        let permit3_future = manager.acquire_rpc_permit();
        tokio::select! {
            _ = permit3_future => panic!("Should not acquire third permit immediately"),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
        
        // After dropping a permit, should be able to acquire again
        drop(_permit1);
        let _permit3 = manager.acquire_rpc_permit().await.unwrap();
    }

    #[tokio::test]
    async fn test_blocking_execution() {
        let manager = ResourceManager::new(10, Some(2));
        
        let result = manager.execute_blocking(|| {
            std::thread::sleep(Duration::from_millis(10));
            42
        }).await.unwrap();
        
        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn test_blocking_execution_without_pool() {
        let manager = ResourceManager::new(10, None);
        
        let result = manager.execute_blocking(|| {
            std::thread::sleep(Duration::from_millis(10));
            42
        }).await.unwrap();
        
        assert_eq!(result, 42);
    }
}
