//! Integration tests for RPC blocking fixes

use reth_hl::node::resource_manager::ResourceManager;
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;

/// Test that resource manager prevents resource exhaustion
#[tokio::test]
async fn test_resource_manager_prevents_exhaustion() {
    let resource_manager = Arc::new(ResourceManager::new(2, Some(2)));
    
    // Should be able to acquire permits up to the limit
    let _permit1 = resource_manager.acquire_rpc_permit().await.unwrap();
    let _permit2 = resource_manager.acquire_rpc_permit().await.unwrap();
    
    // Third permit should be pending (not immediately available)
    let permit3_future = resource_manager.acquire_rpc_permit();
    let result = timeout(Duration::from_millis(50), permit3_future).await;
    assert!(result.is_err(), "Third permit should not be immediately available");
    
    // After dropping a permit, should be able to acquire again
    drop(_permit1);
    let _permit3 = resource_manager.acquire_rpc_permit().await.unwrap();
}

/// Test that blocking operations don't block the async runtime
#[tokio::test]
async fn test_blocking_operations_isolation() {
    let resource_manager = Arc::new(ResourceManager::new(10, Some(4)));
    
    // Start multiple blocking operations concurrently
    let mut handles = Vec::new();
    
    for i in 0..8 {
        let rm = resource_manager.clone();
        let handle = tokio::spawn(async move {
            rm.execute_blocking(move || {
                // Simulate blocking work
                std::thread::sleep(Duration::from_millis(10));
                i * 2
            }).await.unwrap()
        });
        handles.push(handle);
    }
    
    // All operations should complete within reasonable time
    let results = timeout(Duration::from_secs(2), async {
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.unwrap());
        }
        results
    }).await.unwrap();
    
    assert_eq!(results.len(), 8);
    assert_eq!(results, vec![0, 2, 4, 6, 8, 10, 12, 14]);
}

/// Test concurrent RPC and "S3" operations don't block each other
#[tokio::test]
async fn test_concurrent_rpc_s3_operations() {
    let resource_manager = Arc::new(ResourceManager::new(5, Some(4)));
    
    // Simulate RPC operations
    let rpc_task = {
        let rm = resource_manager.clone();
        tokio::spawn(async move {
            let mut results = Vec::new();
            for i in 0..3 {
                let _permit = rm.acquire_rpc_permit().await.unwrap();
                let result = rm.execute_blocking(move || {
                    // Simulate RPC database operation
                    std::thread::sleep(Duration::from_millis(20));
                    format!("rpc-{}", i)
                }).await.unwrap();
                results.push(result);
            }
            results
        })
    };
    
    // Simulate S3 operations (these would normally run on separate runtime)
    let s3_task = tokio::spawn(async {
        let mut results = Vec::new();
        for i in 0..3 {
            // Simulate S3 network operation
            tokio::time::sleep(Duration::from_millis(15)).await;
            results.push(format!("s3-{}", i));
        }
        results
    });
    
    // Both should complete within reasonable time
    let (rpc_results, s3_results) = timeout(Duration::from_secs(3), async {
        tokio::join!(rpc_task, s3_task)
    }).await.unwrap();
    
    let rpc_results = rpc_results.unwrap();
    let s3_results = s3_results.unwrap();
    
    assert_eq!(rpc_results, vec!["rpc-0", "rpc-1", "rpc-2"]);
    assert_eq!(s3_results, vec!["s3-0", "s3-1", "s3-2"]);
}

/// Test resource manager statistics
#[tokio::test]
async fn test_resource_manager_stats() {
    let resource_manager = Arc::new(ResourceManager::new(10, Some(4)));
    
    assert_eq!(resource_manager.available_rpc_permits(), 10);
    assert!(resource_manager.has_blocking_pool());
    
    // Acquire some permits
    let _permit1 = resource_manager.acquire_rpc_permit().await.unwrap();
    let _permit2 = resource_manager.acquire_rpc_permit().await.unwrap();
    
    assert_eq!(resource_manager.available_rpc_permits(), 8);
    
    // Test without blocking pool
    let resource_manager_no_pool = Arc::new(ResourceManager::new(5, None));
    assert!(!resource_manager_no_pool.has_blocking_pool());
}

/// Test that resource manager handles errors gracefully
#[tokio::test]
async fn test_resource_manager_error_handling() {
    let resource_manager = Arc::new(ResourceManager::new(10, Some(2)));
    
    // Test that panics in blocking operations are handled
    let result = resource_manager.execute_blocking(|| {
        panic!("Test panic");
    }).await;
    
    assert!(result.is_err());
}

/// Benchmark-style test to ensure performance is reasonable
#[tokio::test]
async fn test_performance_under_load() {
    let resource_manager = Arc::new(ResourceManager::new(20, Some(8)));
    
    let start = std::time::Instant::now();
    
    // Run many concurrent operations
    let mut handles = Vec::new();
    for i in 0..50 {
        let rm = resource_manager.clone();
        let handle = tokio::spawn(async move {
            let _permit = rm.acquire_rpc_permit().await.unwrap();
            rm.execute_blocking(move || {
                // Very light work to test throughput
                i + 1
            }).await.unwrap()
        });
        handles.push(handle);
    }
    
    let results: Vec<_> = futures::future::join_all(handles).await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    
    let duration = start.elapsed();
    
    assert_eq!(results.len(), 50);
    assert!(duration < Duration::from_secs(5), "Operations took too long: {:?}", duration);
}
