//! Enhanced RPC implementations that prevent blocking S3 syncing

use crate::node::resource_manager::ResourceManager;
use alloy_primitives::B256;
use alloy_rpc_types_eth::BlockId;
use reth_rpc_eth_api::RpcNodeCore;
use std::sync::Arc;
use tracing::{debug, instrument, warn};

/// Enhanced RPC wrapper that uses resource management to prevent blocking
#[derive(Debug, Clone)]
pub struct EnhancedRpcWrapper<T> {
    inner: T,
    resource_manager: Arc<ResourceManager>,
}

impl<T> EnhancedRpcWrapper<T> {
    pub fn new(inner: T, resource_manager: Arc<ResourceManager>) -> Self {
        Self {
            inner,
            resource_manager,
        }
    }
}

impl<T> EnhancedRpcWrapper<T>
where
    T: RpcNodeCore + Clone + Send + Sync + 'static,
{
    /// Enhanced version of getting block header that uses resource management
    #[instrument(skip(self), fields(block_id = ?block_id))]
    pub async fn get_block_header_enhanced(
        &self,
        block_id: BlockId,
    ) -> Result<Option<T::Primitives>, Box<dyn std::error::Error + Send + Sync>> {
        // Acquire permit to prevent resource exhaustion
        let _permit = self.resource_manager.acquire_rpc_permit().await
            .map_err(|e| format!("Failed to acquire RPC permit: {}", e))?;

        debug!("Acquired RPC permit for get_block_header");

        let inner = self.inner.clone();
        let result = self.resource_manager.execute_blocking(move || {
            // This would normally be a direct database call that could block
            // In the actual implementation, this would call the provider
            debug!("Executing blocking database operation for block header");
            
            // Simulate the database operation
            // In real implementation: inner.provider().header_by_id(block_id)
            Ok(None) // Placeholder
        }).await;

        match result {
            Ok(header) => {
                debug!("Successfully retrieved block header");
                header
            }
            Err(e) => {
                warn!("Failed to retrieve block header: {}", e);
                Err(format!("Database operation failed: {}", e).into())
            }
        }
    }

    /// Enhanced version of getting raw transaction that uses resource management
    #[instrument(skip(self), fields(hash = ?hash))]
    pub async fn get_raw_transaction_enhanced(
        &self,
        hash: B256,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        // Acquire permit to prevent resource exhaustion
        let _permit = self.resource_manager.acquire_rpc_permit().await
            .map_err(|e| format!("Failed to acquire RPC permit: {}", e))?;

        debug!("Acquired RPC permit for get_raw_transaction");

        let inner = self.inner.clone();
        let result = self.resource_manager.execute_blocking(move || {
            debug!("Executing blocking database operation for raw transaction");
            
            // In real implementation: inner.provider().transaction_by_hash(hash)
            Ok(None) // Placeholder
        }).await;

        match result {
            Ok(tx) => {
                debug!("Successfully retrieved raw transaction");
                tx
            }
            Err(e) => {
                warn!("Failed to retrieve raw transaction: {}", e);
                Err(format!("Database operation failed: {}", e).into())
            }
        }
    }

    /// Enhanced version of getting block transactions that uses resource management
    #[instrument(skip(self), fields(block_id = ?block_id))]
    pub async fn get_block_transactions_enhanced(
        &self,
        block_id: BlockId,
    ) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        // Acquire permit to prevent resource exhaustion
        let _permit = self.resource_manager.acquire_rpc_permit().await
            .map_err(|e| format!("Failed to acquire RPC permit: {}", e))?;

        debug!("Acquired RPC permit for get_block_transactions");

        let inner = self.inner.clone();
        let result = self.resource_manager.execute_blocking(move || {
            debug!("Executing blocking database operation for block transactions");
            
            // In real implementation: inner.provider().block_with_senders_by_id(block_id)
            Ok(Vec::new()) // Placeholder
        }).await;

        match result {
            Ok(txs) => {
                debug!("Successfully retrieved block transactions");
                txs
            }
            Err(e) => {
                warn!("Failed to retrieve block transactions: {}", e);
                Err(format!("Database operation failed: {}", e).into())
            }
        }
    }

    /// Get resource manager statistics
    pub fn get_resource_stats(&self) -> ResourceStats {
        ResourceStats {
            available_rpc_permits: self.resource_manager.available_rpc_permits(),
            has_blocking_pool: self.resource_manager.has_blocking_pool(),
        }
    }
}

/// Statistics about resource usage
#[derive(Debug, Clone)]
pub struct ResourceStats {
    pub available_rpc_permits: usize,
    pub has_blocking_pool: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::resource_manager::ResourceManager;

    #[tokio::test]
    async fn test_resource_stats() {
        let resource_manager = Arc::new(ResourceManager::new(10, Some(2)));

        // Test resource manager directly
        assert_eq!(resource_manager.available_rpc_permits(), 10);
        assert!(resource_manager.has_blocking_pool());
    }
}
