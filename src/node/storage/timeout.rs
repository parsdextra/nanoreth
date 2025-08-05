use std::time::Duration;
use tokio::time::timeout;
use reth_provider::{DatabaseProvider, ProviderResult};
use reth_db::transaction::DbTx;
use std::future::Future;
use tracing::{warn, debug, info};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Statistics for database connection management
#[derive(Debug, Default)]
pub struct DatabaseStats {
    pub active_connections: AtomicU64,
    pub total_operations: AtomicU64,
    pub timeout_operations: AtomicU64,
    pub successful_operations: AtomicU64,
}

impl DatabaseStats {
    pub fn increment_active(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn decrement_active(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn increment_total(&self) {
        self.total_operations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_timeout(&self) {
        self.timeout_operations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_successful(&self) {
        self.successful_operations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_active_count(&self) -> u64 {
        self.active_connections.load(Ordering::Relaxed)
    }

    pub fn log_stats(&self) {
        let active = self.active_connections.load(Ordering::Relaxed);
        let total = self.total_operations.load(Ordering::Relaxed);
        let timeouts = self.timeout_operations.load(Ordering::Relaxed);
        let successful = self.successful_operations.load(Ordering::Relaxed);

        info!(
            "Database stats - Active: {}, Total: {}, Timeouts: {}, Successful: {}",
            active, total, timeouts, successful
        );
    }
}

/// Wrapper for database operations with timeout handling and connection management
pub struct DatabaseTimeoutWrapper {
    pub read_timeout: Duration,
    pub stats: Arc<DatabaseStats>,
    pub max_concurrent_operations: u64,
}

impl DatabaseTimeoutWrapper {
    pub fn new(read_timeout_secs: u64) -> Self {
        Self {
            read_timeout: Duration::from_secs(read_timeout_secs),
            stats: Arc::new(DatabaseStats::default()),
            max_concurrent_operations: 100, // Default limit
        }
    }

    pub fn with_max_concurrent_operations(mut self, max: u64) -> Self {
        self.max_concurrent_operations = max;
        self
    }

    pub fn get_stats(&self) -> Arc<DatabaseStats> {
        self.stats.clone()
    }

    /// Check if we can start a new operation
    fn can_start_operation(&self) -> bool {
        self.stats.get_active_count() < self.max_concurrent_operations
    }

    /// Guard for tracking operation lifecycle
    struct OperationGuard {
        stats: Arc<DatabaseStats>,
    }

    impl OperationGuard {
        fn new(stats: Arc<DatabaseStats>) -> Self {
            stats.increment_active();
            stats.increment_total();
            Self { stats }
        }
    }

    impl Drop for OperationGuard {
        fn drop(&mut self) {
            self.stats.decrement_active();
        }
    }

    /// Execute a database operation with timeout and connection management
    pub async fn execute_with_timeout<F, T, E>(
        &self,
        operation: F,
        operation_name: &str,
    ) -> Result<T, E>
    where
        F: Future<Output = Result<T, E>>,
        E: From<std::io::Error>,
    {
        // Check if we can start a new operation
        if !self.can_start_operation() {
            warn!(
                "Database operation '{}' rejected - too many concurrent operations ({})",
                operation_name, self.stats.get_active_count()
            );
            return Err(E::from(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                format!("Too many concurrent database operations for '{}'", operation_name),
            )));
        }

        let _guard = Self::OperationGuard::new(self.stats.clone());

        debug!("Starting database operation: {}", operation_name);

        match timeout(self.read_timeout, operation).await {
            Ok(result) => {
                match &result {
                    Ok(_) => {
                        self.stats.increment_successful();
                        debug!("Database operation '{}' completed successfully", operation_name);
                    }
                    Err(_) => {
                        debug!("Database operation '{}' failed", operation_name);
                    }
                }
                result
            }
            Err(_) => {
                self.stats.increment_timeout();
                warn!(
                    "Database operation '{}' timed out after {:?}",
                    operation_name, self.read_timeout
                );
                Err(E::from(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("Database operation '{}' timed out", operation_name),
                )))
            }
        }
    }

    /// Execute a provider operation with timeout and connection management
    pub async fn execute_provider_with_timeout<TX, F, T>(
        &self,
        _provider: &DatabaseProvider<TX, impl reth_provider::providers::NodeTypesForProvider>,
        operation: F,
        operation_name: &str,
    ) -> ProviderResult<T>
    where
        TX: DbTx + 'static,
        F: Future<Output = ProviderResult<T>>,
    {
        // Check if we can start a new operation
        if !self.can_start_operation() {
            warn!(
                "Provider operation '{}' rejected - too many concurrent operations ({})",
                operation_name, self.stats.get_active_count()
            );
            return Err(reth_provider::ProviderError::Database(
                reth_storage_errors::db::DatabaseError::Other(format!(
                    "Too many concurrent provider operations for '{}'",
                    operation_name
                )),
            ));
        }

        let _guard = Self::OperationGuard::new(self.stats.clone());

        debug!("Starting provider operation: {}", operation_name);

        match timeout(self.read_timeout, operation).await {
            Ok(result) => {
                match &result {
                    Ok(_) => {
                        self.stats.increment_successful();
                        debug!("Provider operation '{}' completed successfully", operation_name);
                    }
                    Err(_) => {
                        debug!("Provider operation '{}' failed", operation_name);
                    }
                }
                result
            }
            Err(_) => {
                self.stats.increment_timeout();
                warn!(
                    "Provider operation '{}' timed out after {:?}",
                    operation_name, self.read_timeout
                );
                Err(reth_provider::ProviderError::Database(
                    reth_storage_errors::db::DatabaseError::Other(format!(
                        "Provider operation '{}' timed out after {:?}",
                        operation_name, self.read_timeout
                    )),
                ))
            }
        }
    }
}

/// Extension trait for adding timeout functionality to database providers
pub trait DatabaseTimeoutExt {
    fn db_timeout_wrapper(&self) -> &DatabaseTimeoutWrapper;
}

/// Macro to wrap database operations with timeout
#[macro_export]
macro_rules! with_db_timeout {
    ($wrapper:expr, $operation:expr, $name:expr) => {
        $wrapper.execute_with_timeout($operation, $name).await
    };
}

/// Macro to wrap provider operations with timeout
#[macro_export]
macro_rules! with_provider_timeout {
    ($wrapper:expr, $provider:expr, $operation:expr, $name:expr) => {
        $wrapper.execute_provider_with_timeout($provider, $operation, $name).await
    };
}
