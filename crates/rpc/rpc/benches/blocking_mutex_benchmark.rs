//! Benchmarks to measure the performance impact of blocking mutex fixes

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::time::Duration;
use tokio::runtime::Runtime;

/// Benchmark simulating the old blocking behavior
fn benchmark_blocking_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("blocking_database_operations", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Simulate the old way - direct database calls that could block
                let mut tasks = Vec::new();
                for i in 0..10 {
                    let task = async move {
                        // Simulate blocking database operation
                        std::thread::sleep(Duration::from_millis(1));
                        black_box(i)
                    };
                    tasks.push(task);
                }
                futures::future::join_all(tasks).await
            })
        })
    });
}

/// Benchmark simulating the new non-blocking behavior
fn benchmark_spawn_blocking_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("spawn_blocking_database_operations", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Simulate the new way - using spawn_blocking for database calls
                let mut tasks = Vec::new();
                for i in 0..10 {
                    let task = tokio::task::spawn_blocking(move || {
                        // Simulate database operation on blocking thread
                        std::thread::sleep(Duration::from_millis(1));
                        black_box(i)
                    });
                    tasks.push(task);
                }
                let results = futures::future::join_all(tasks).await;
                results.into_iter().map(|r| r.unwrap()).collect::<Vec<_>>()
            })
        })
    });
}

/// Benchmark concurrent RPC and S3 operations
fn benchmark_concurrent_rpc_s3(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("concurrent_rpc_s3_operations", |b| {
        b.iter(|| {
            rt.block_on(async {
                let rpc_task = tokio::spawn(async {
                    // Simulate RPC operations using spawn_blocking
                    let mut tasks = Vec::new();
                    for i in 0..5 {
                        let task = tokio::task::spawn_blocking(move || {
                            std::thread::sleep(Duration::from_millis(2));
                            black_box(i)
                        });
                        tasks.push(task);
                    }
                    futures::future::join_all(tasks).await
                });
                
                let s3_task = tokio::spawn(async {
                    // Simulate S3 operations on separate runtime
                    let mut results = Vec::new();
                    for i in 0..3 {
                        tokio::time::sleep(Duration::from_millis(3)).await;
                        results.push(black_box(i));
                    }
                    results
                });
                
                let (rpc_results, s3_results) = tokio::join!(rpc_task, s3_task);
                (rpc_results.unwrap(), s3_results.unwrap())
            })
        })
    });
}

/// Benchmark the impact of increased resource limits
fn benchmark_increased_limits(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("high_concurrency_operations", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Simulate many concurrent operations (up to new limits)
                let mut tasks = Vec::new();
                for i in 0..50 {
                    let task = tokio::task::spawn_blocking(move || {
                        // Very light work to test concurrency limits
                        black_box(i * 2)
                    });
                    tasks.push(task);
                }
                let results = futures::future::join_all(tasks).await;
                results.into_iter().map(|r| r.unwrap()).collect::<Vec<_>>()
            })
        })
    });
}

criterion_group!(
    benches,
    benchmark_blocking_operations,
    benchmark_spawn_blocking_operations,
    benchmark_concurrent_rpc_s3,
    benchmark_increased_limits
);
criterion_main!(benches);
