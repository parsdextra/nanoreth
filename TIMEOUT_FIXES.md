# Database Transaction Timeout Fixes for 2B Gas Operations

This document describes the comprehensive fixes implemented to handle massive gas caps (up to 2 billion gas) without causing database transaction timeouts and process hangs.

## Problem Description

When `max.gascap` (rpc_gas_cap) is set too high, the process hangs with database read transaction timeout warnings:

```
WARN The database read transaction has been open for too long
```

This occurs because:
1. High gas cap allows complex RPC calls (eth_call, eth_estimateGas) to run for extended periods
2. Database read transactions remain open during the entire EVM simulation
3. Long-running operations exhaust database connection resources

## Implemented Solutions

### 1. Progressive Timeout Scaling

**Files Modified:**
- `src/node/rpc/timeout.rs`
- `src/node/cli.rs`

**Features:**
- Dynamic timeout scaling based on gas limit
- Supports operations up to 2B gas with appropriate timeouts
- Configurable scaling parameters and maximum timeout limits

**Timeout Scaling Formula:**
- Up to 1M gas: Base timeout (default 30s)
- 1M to 100M gas: Linear scale to 10x base timeout
- 100M to 2B gas: Scale up to maximum timeout (default 1 hour)

### 2. Chunked Execution for Massive Operations

**Files Modified:**
- `src/node/rpc/chunked_execution.rs` (new)
- `src/node/rpc/timeout.rs`
- `src/node/rpc/mod.rs`

**Features:**
- Automatic chunking for operations above configurable threshold (default 100M gas)
- Configurable chunk size (default 50M gas per chunk)
- Progress tracking and reporting
- Automatic delays between chunks for database cleanup
- Support for both eth_call and eth_estimateGas

### 3. RPC Call Timeout Mechanisms

**Files Modified:**
- `src/node/rpc/timeout.rs` (new)
- `src/node/rpc/mod.rs`
- `src/call_forwarder.rs`

**Features:**
- Configurable RPC call timeouts (default: 30 seconds)
- Gas limit-based forwarding decisions
- Automatic timeout handling for eth_call and eth_estimateGas
- Integration with existing call forwarding system

**Configuration:**
```bash
--rpc-call-timeout 30                    # Base RPC timeout in seconds
--enable-progressive-timeout true        # Enable dynamic timeout scaling
--max-timeout 3600                       # Maximum timeout for 2B gas operations
--chunk-gas-limit 50000000               # Gas per chunk for massive operations
--chunking-threshold 100000000           # Start chunking above this gas limit
```

### 4. Database Transaction Timeout Handling

**Files Modified:**
- `src/node/storage/timeout.rs` (new)
- `src/node/storage/mod.rs`
- `src/node/rpc/mod.rs`

**Features:**
- Configurable database read timeouts (default: 60 seconds)
- Automatic timeout detection and cleanup
- Provider operation wrapping with timeout handling
- Comprehensive error reporting

**Configuration:**
```bash
--db-read-timeout 60                     # Database timeout in seconds
```

### 5. Database Connection Management

**Files Modified:**
- `src/node/storage/timeout.rs`
- `src/node/rpc/mod.rs`

**Features:**
- Connection pool management with configurable limits
- Concurrent operation tracking and limiting
- Detailed statistics collection (active connections, timeouts, success rates)
- Automatic rejection of operations when limits are exceeded
- Resource cleanup and monitoring

**Configuration:**
```bash
--max-concurrent-db-ops 100             # Max concurrent database operations
```

### 6. Configuration System

**Files Modified:**
- `src/node/cli.rs`
- `src/node/mod.rs`
- `src/main.rs`

**New CLI Arguments:**
```bash
--rpc-call-timeout <SECONDS>            # Base RPC call timeout (default: 30)
--db-read-timeout <SECONDS>             # Database read timeout (default: 60)
--max-local-gas-limit <GAS>             # Max gas for local execution
--max-concurrent-db-ops <COUNT>         # Max concurrent DB ops (default: 100)
--enable-progressive-timeout <BOOL>     # Enable dynamic timeout scaling (default: true)
--max-timeout <SECONDS>                 # Maximum timeout for largest operations (default: 3600)
--chunk-gas-limit <GAS>                 # Gas per chunk for massive operations (default: 50M)
--chunking-threshold <GAS>              # Start chunking above this gas (default: 100M)
```

## Usage Examples

### 2 Billion Gas Configuration (No Upstream)
```bash
reth-hl node \
  --rpc.gascap 2000000000 \
  --enable-progressive-timeout true \
  --max-timeout 7200 \
  --chunk-gas-limit 50000000 \
  --chunking-threshold 100000000 \
  --rpc-call-timeout 60 \
  --db-read-timeout 300 \
  --max-concurrent-db-ops 25
```

### High Performance 2B Gas Setup
```bash
reth-hl node \
  --rpc.gascap 2000000000 \
  --enable-progressive-timeout true \
  --max-timeout 10800 \
  --chunk-gas-limit 100000000 \
  --chunking-threshold 200000000 \
  --rpc-call-timeout 120 \
  --db-read-timeout 600 \
  --max-concurrent-db-ops 10
```

### Conservative 2B Gas Setup
```bash
reth-hl node \
  --rpc.gascap 2000000000 \
  --enable-progressive-timeout true \
  --max-timeout 3600 \
  --chunk-gas-limit 25000000 \
  --chunking-threshold 50000000 \
  --rpc-call-timeout 30 \
  --db-read-timeout 120 \
  --max-concurrent-db-ops 50
```

## How It Works

### RPC Call Flow for 2B Gas Operations
1. **Request arrives** → Analyze gas limit and determine execution strategy
2. **Gas limit check** → If above chunking threshold, enable chunked execution
3. **Timeout calculation** → Progressive scaling based on gas limit (up to max timeout)
4. **Chunked execution** → Break operation into manageable chunks (e.g., 50M gas each)
5. **Progress tracking** → Monitor and report progress through chunks
6. **Database management** → Automatic cleanup between chunks
7. **Result aggregation** → Combine chunk results into final response

### Database Operation Flow
1. **Operation starts** → Check concurrent operation limit
2. **Limit exceeded** → Reject with "too many operations" error
3. **Operation allowed** → Track in connection pool and start timeout
4. **Timeout reached** → Cancel operation and log warning
5. **Operation completes** → Update statistics and cleanup resources

### Chunked Execution Flow
1. **Chunk calculation** → Determine number of chunks needed (gas_limit / chunk_size)
2. **Sequential execution** → Execute chunks one by one to prevent resource exhaustion
3. **Inter-chunk delays** → Configurable delays between chunks for database cleanup
4. **Progress reporting** → Real-time progress updates with time estimates
5. **Error handling** → Graceful failure handling with detailed error reporting

### Connection Management
- **Active tracking**: Real-time monitoring of concurrent operations
- **Automatic limits**: Prevent resource exhaustion with configurable limits
- **Statistics**: Track success rates, timeouts, and active connections
- **Cleanup**: Automatic resource cleanup on operation completion or timeout

## Benefits

1. **2B Gas Support**: Native support for 2 billion gas operations without upstream dependency
2. **Stability**: Prevents process hangs from long-running database transactions
3. **Progressive Scaling**: Intelligent timeout scaling based on operation complexity
4. **Chunked Execution**: Breaks massive operations into manageable pieces
5. **Progress Tracking**: Real-time monitoring of long-running operations
6. **Resource Management**: Optimized database connection and memory usage
7. **Flexibility**: Highly configurable for different workload requirements
8. **Monitoring**: Comprehensive statistics and logging for debugging

## Monitoring

The system provides detailed logging for:
- **Chunked execution progress**: Real-time updates on chunk completion and progress percentages
- **Timeout scaling decisions**: Dynamic timeout calculations based on gas limits
- **Database operation statistics**: Connection usage, timeouts, and success rates
- **Resource management**: Connection pool status and resource exhaustion warnings
- **Performance metrics**: Execution times, chunk processing rates, and throughput

### Example Log Output for 2B Gas Operation:
```
INFO Starting chunked execution: 2000000000 gas in 40 chunks of 50000000 gas each
INFO Chunk 1/40 completed in 2.3s - Progress: 2.5% (50000000 / 2000000000 gas)
INFO Chunk 10/40 completed in 2.1s - Progress: 25.0% (500000000 / 2000000000 gas)
INFO Chunk 40/40 completed in 2.4s - Progress: 100.0% (2000000000 / 2000000000 gas)
INFO Chunked execution completed: 2000000000 gas in 40 chunks, total time: 95.2s
```

Use these logs to tune the configuration parameters for your specific workload and monitor the health of 2B gas operations.
