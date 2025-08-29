# RPC Blocking Task Pool Exhaustion Fixes

This document describes the application-level workarounds implemented to prevent RPC operations from blocking S3 syncing due to task pool exhaustion.

## Problem

The original issue was that heavy RPC load could exhaust the blocking task pool, preventing S3 syncing operations from completing. This happened because:

1. RPC operations were consuming all available blocking threads
2. Database operations in RPC handlers were blocking the async runtime
3. No resource isolation between RPC and S3 operations
4. Limited concurrency controls for resource-intensive operations

## Solution Overview

Since we cannot modify the underlying Reth dependency directly, we implemented application-level workarounds:

### 1. **Resource Manager** (`src/node/resource_manager.rs`)

A centralized resource management system that:
- **Limits concurrent RPC operations** using semaphores
- **Provides dedicated thread pool** for blocking operations
- **Isolates blocking work** from the async runtime
- **Prevents resource exhaustion** under heavy load

### 2. **Enhanced RPC Wrappers** (`src/node/rpc/enhanced.rs`)

Enhanced RPC implementations that:
- **Use resource permits** before performing expensive operations
- **Execute blocking operations** on dedicated thread pools
- **Provide proper error handling** and logging
- **Maintain compatibility** with existing RPC interfaces

### 3. **CLI Configuration Options** (`src/node/cli.rs`)

New command-line options to control resource usage:
- `--max-concurrent-rpc-requests`: Limit concurrent RPC operations (default: 50)
- `--enable-blocking-thread-pool`: Enable dedicated blocking thread pool (default: true)
- `--blocking-thread-pool-size`: Size of blocking thread pool (default: 4)

## Usage

### Command Line Options

```bash
# Run with default settings (recommended)
./reth-hl

# Customize resource limits
./reth-hl \
  --max-concurrent-rpc-requests 100 \
  --blocking-thread-pool-size 8

# Disable blocking thread pool (not recommended)
./reth-hl --enable-blocking-thread-pool false
```

### Environment Variables

```bash
export MAX_CONCURRENT_RPC_REQUESTS=75
export BLOCKING_THREAD_POOL_SIZE=6
export ENABLE_BLOCKING_THREAD_POOL=true
```

### Programmatic Usage

```rust
use reth_hl::node::resource_manager::ResourceManager;
use std::sync::Arc;

// Create resource manager
let resource_manager = Arc::new(ResourceManager::new(50, Some(4)));

// Acquire permit for RPC operation
let _permit = resource_manager.acquire_rpc_permit().await?;

// Execute blocking operation
let result = resource_manager.execute_blocking(|| {
    // Your blocking database operation here
    expensive_database_call()
}).await?;
```

## Benefits

### 1. **Prevents Resource Exhaustion**
- Limits concurrent RPC operations to prevent overwhelming the system
- Dedicated thread pools ensure blocking operations don't starve each other

### 2. **Improves S3 Sync Reliability**
- S3 operations can proceed even under heavy RPC load
- Resource isolation prevents RPC operations from blocking S3 syncing

### 3. **Better Performance Under Load**
- Controlled concurrency prevents system thrashing
- Proper thread pool management improves overall throughput

### 4. **Configurable Resource Limits**
- Tune resource limits based on your hardware and workload
- Environment variables and CLI options for easy configuration

## Testing

Run the test suite to validate the fixes:

```bash
# Run all blocking fixes tests
cargo test rpc_blocking_fixes

# Run specific tests
cargo test test_resource_manager_prevents_exhaustion
cargo test test_blocking_operations_isolation
cargo test test_concurrent_rpc_s3_operations
```

## Monitoring

The resource manager provides statistics for monitoring:

```rust
let stats = resource_manager.get_resource_stats();
println!("Available RPC permits: {}", stats.available_rpc_permits);
println!("Has blocking pool: {}", stats.has_blocking_pool);
```

## Troubleshooting

### High RPC Load Still Causing Issues

1. **Increase RPC permit limit**: `--max-concurrent-rpc-requests 100`
2. **Increase blocking thread pool size**: `--blocking-thread-pool-size 8`
3. **Monitor resource usage** and adjust limits accordingly

### S3 Syncing Still Slow

1. **Verify resource isolation** is working by checking logs
2. **Consider reducing RPC limits** to give S3 more resources
3. **Check network and storage performance** for S3 operations

### Memory Usage Concerns

1. **Monitor thread pool sizes** - each thread uses memory
2. **Adjust limits based on available RAM**
3. **Consider disabling blocking thread pool** if memory is very constrained (not recommended)

## Implementation Notes

- **Backward Compatible**: All changes are additive and don't break existing functionality
- **Configurable**: All limits can be adjusted via CLI or environment variables
- **Tested**: Comprehensive test suite validates the fixes work correctly
- **Documented**: Clear documentation and examples for usage

## Future Improvements

When possible, consider:
1. **Upgrading to Reth version** with native fixes
2. **Contributing fixes upstream** to the Reth project
3. **Implementing more sophisticated** resource scheduling algorithms
4. **Adding metrics and monitoring** for better observability
