use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{info, debug, warn};
use alloy_primitives::{Bytes, U256};
use alloy_rpc_types::TransactionRequest;
use alloy_eips::BlockId;
use reth_rpc_eth_api::helpers::{EthCall, EstimateCall};
use reth_rpc_types::EvmOverrides;
use alloy_rpc_types_eth::state::StateOverride;
use jsonrpsee::types::ErrorObject;

/// Configuration for chunked execution
#[derive(Debug, Clone)]
pub struct ChunkedExecutionConfig {
    /// Gas limit per chunk
    pub chunk_gas_limit: u64,
    /// Minimum gas limit to enable chunking
    pub chunking_threshold: u64,
    /// Delay between chunks to allow database cleanup
    pub chunk_delay_ms: u64,
    /// Maximum number of chunks to prevent infinite loops
    pub max_chunks: u32,
    /// Enable progress reporting
    pub enable_progress_reporting: bool,
}

impl Default for ChunkedExecutionConfig {
    fn default() -> Self {
        Self {
            chunk_gas_limit: 50_000_000,      // 50M gas per chunk
            chunking_threshold: 100_000_000,   // Start chunking at 100M gas
            chunk_delay_ms: 100,               // 100ms delay between chunks
            max_chunks: 100,                   // Max 100 chunks (5B gas total)
            enable_progress_reporting: true,
        }
    }
}

/// Progress information for chunked execution
#[derive(Debug, Clone)]
pub struct ExecutionProgress {
    pub total_gas: u64,
    pub processed_gas: u64,
    pub current_chunk: u32,
    pub total_chunks: u32,
    pub elapsed_time: Duration,
    pub estimated_remaining: Duration,
}

impl ExecutionProgress {
    pub fn progress_percentage(&self) -> f64 {
        if self.total_gas == 0 {
            0.0
        } else {
            (self.processed_gas as f64 / self.total_gas as f64) * 100.0
        }
    }
}

/// Chunked execution engine for massive gas operations
pub struct ChunkedExecutionEngine {
    config: ChunkedExecutionConfig,
}

impl ChunkedExecutionEngine {
    pub fn new(config: ChunkedExecutionConfig) -> Self {
        Self { config }
    }

    /// Check if an operation should use chunked execution
    pub fn should_use_chunking(&self, gas_limit: Option<u64>) -> bool {
        gas_limit.map_or(false, |gas| gas >= self.config.chunking_threshold)
    }

    /// Calculate the number of chunks needed
    fn calculate_chunks(&self, gas_limit: u64) -> u32 {
        let chunks = (gas_limit + self.config.chunk_gas_limit - 1) / self.config.chunk_gas_limit;
        chunks.min(self.config.max_chunks as u64) as u32
    }

    /// Execute eth_call with chunking for massive gas operations
    pub async fn chunked_call<T>(
        &self,
        eth_api: &T,
        mut request: TransactionRequest,
        block_number: Option<BlockId>,
        overrides: EvmOverrides,
    ) -> Result<Bytes, ErrorObject<'static>>
    where
        T: EthCall + Send + Sync,
    {
        let original_gas = request.gas.unwrap_or(21000);
        
        if !self.should_use_chunking(Some(original_gas)) {
            // Use normal execution for smaller operations
            return EthCall::call(eth_api, request, block_number, overrides).await
                .map_err(|e| ErrorObject::owned(-32603, format!("Call failed: {e:?}"), Some(())));
        }

        let total_chunks = self.calculate_chunks(original_gas);
        let start_time = Instant::now();
        
        info!(
            "Starting chunked execution: {} gas in {} chunks of {} gas each",
            original_gas, total_chunks, self.config.chunk_gas_limit
        );

        let mut accumulated_result = Bytes::new();
        let mut processed_gas = 0u64;

        for chunk_idx in 0..total_chunks {
            let chunk_gas = if chunk_idx == total_chunks - 1 {
                // Last chunk gets remaining gas
                original_gas - processed_gas
            } else {
                self.config.chunk_gas_limit
            };

            // Update request for this chunk
            request.gas = Some(chunk_gas);
            
            let chunk_start = Instant::now();
            
            debug!(
                "Executing chunk {}/{}: {} gas",
                chunk_idx + 1, total_chunks, chunk_gas
            );

            // Execute the chunk
            let chunk_result = EthCall::call(eth_api, request.clone(), block_number, overrides.clone()).await
                .map_err(|e| ErrorObject::owned(-32603, format!("Chunk {} failed: {e:?}", chunk_idx + 1), Some(())))?;

            processed_gas += chunk_gas;
            
            // For simplicity, we'll use the result from the last chunk
            // In a real implementation, you might need to merge results differently
            accumulated_result = chunk_result;

            let chunk_duration = chunk_start.elapsed();
            
            if self.config.enable_progress_reporting {
                let progress = ExecutionProgress {
                    total_gas: original_gas,
                    processed_gas,
                    current_chunk: chunk_idx + 1,
                    total_chunks,
                    elapsed_time: start_time.elapsed(),
                    estimated_remaining: if chunk_idx > 0 {
                        let avg_chunk_time = start_time.elapsed() / (chunk_idx + 1);
                        avg_chunk_time * (total_chunks - chunk_idx - 1)
                    } else {
                        Duration::from_secs(0)
                    },
                };

                info!(
                    "Chunk {}/{} completed in {:?} - Progress: {:.1}% ({} / {} gas)",
                    chunk_idx + 1,
                    total_chunks,
                    chunk_duration,
                    progress.progress_percentage(),
                    processed_gas,
                    original_gas
                );
            }

            // Add delay between chunks to allow database cleanup
            if chunk_idx < total_chunks - 1 && self.config.chunk_delay_ms > 0 {
                sleep(Duration::from_millis(self.config.chunk_delay_ms)).await;
            }
        }

        let total_duration = start_time.elapsed();
        info!(
            "Chunked execution completed: {} gas in {} chunks, total time: {:?}",
            original_gas, total_chunks, total_duration
        );

        Ok(accumulated_result)
    }

    /// Execute eth_estimateGas with chunking
    pub async fn chunked_estimate_gas<T>(
        &self,
        eth_api: &T,
        mut request: TransactionRequest,
        block_number: BlockId,
        state_override: Option<StateOverride>,
    ) -> Result<U256, ErrorObject<'static>>
    where
        T: EstimateCall + Send + Sync,
    {
        let original_gas = request.gas.unwrap_or(21000);
        
        if !self.should_use_chunking(Some(original_gas)) {
            // Use normal execution for smaller operations
            return EstimateCall::estimate_gas_at(eth_api, request, block_number, state_override).await
                .map_err(|e| ErrorObject::owned(-32603, format!("Gas estimation failed: {e:?}"), Some(())));
        }

        // For gas estimation with chunking, we'll estimate each chunk and sum them
        let total_chunks = self.calculate_chunks(original_gas);
        let start_time = Instant::now();
        
        info!(
            "Starting chunked gas estimation: {} gas in {} chunks",
            original_gas, total_chunks
        );

        let mut total_estimated_gas = U256::ZERO;
        let mut processed_gas = 0u64;

        for chunk_idx in 0..total_chunks {
            let chunk_gas = if chunk_idx == total_chunks - 1 {
                original_gas - processed_gas
            } else {
                self.config.chunk_gas_limit
            };

            request.gas = Some(chunk_gas);
            
            debug!(
                "Estimating chunk {}/{}: {} gas",
                chunk_idx + 1, total_chunks, chunk_gas
            );

            let chunk_estimate = EstimateCall::estimate_gas_at(eth_api, request.clone(), block_number, state_override.clone()).await
                .map_err(|e| ErrorObject::owned(-32603, format!("Chunk {} estimation failed: {e:?}", chunk_idx + 1), Some(())))?;

            total_estimated_gas += chunk_estimate;
            processed_gas += chunk_gas;

            if self.config.enable_progress_reporting && chunk_idx % 10 == 0 {
                info!(
                    "Gas estimation progress: chunk {}/{} - estimated so far: {}",
                    chunk_idx + 1, total_chunks, total_estimated_gas
                );
            }

            // Small delay between estimation chunks
            if chunk_idx < total_chunks - 1 && self.config.chunk_delay_ms > 0 {
                sleep(Duration::from_millis(self.config.chunk_delay_ms / 2)).await;
            }
        }

        let total_duration = start_time.elapsed();
        info!(
            "Chunked gas estimation completed: {} total estimated gas in {:?}",
            total_estimated_gas, total_duration
        );

        Ok(total_estimated_gas)
    }
}
