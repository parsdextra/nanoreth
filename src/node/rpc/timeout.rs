use std::time::Duration;
use tokio::time::timeout;
use reth_rpc_eth_api::helpers::{EthCall, EstimateCall};
use alloy_rpc_types::{TransactionRequest, BlockId, StateOverride, BlockOverrides};
use alloy_primitives::{Bytes, U256};
use reth::rpc::server_types::eth::EthApiError;
use jsonrpsee::types::ErrorObject;
use reth_rpc_types::EvmOverrides;
use super::chunked_execution::{ChunkedExecutionEngine, ChunkedExecutionConfig};

/// Error codes for timeout-related errors
const RPC_TIMEOUT_ERROR_CODE: i32 = -32603;

/// Wrapper for RPC calls with timeout handling
pub struct TimeoutWrapper {
    pub rpc_timeout: Duration,
    pub max_local_gas_limit: Option<u64>,
    pub enable_progressive_timeout: bool,
    pub max_timeout: Duration,
    pub chunked_execution: ChunkedExecutionEngine,
}

impl TimeoutWrapper {
    pub fn new(rpc_timeout_secs: u64, max_local_gas_limit: Option<u64>) -> Self {
        Self {
            rpc_timeout: Duration::from_secs(rpc_timeout_secs),
            max_local_gas_limit,
            enable_progressive_timeout: true,
            max_timeout: Duration::from_secs(3600), // 1 hour max for 2B gas operations
            chunked_execution: ChunkedExecutionEngine::new(ChunkedExecutionConfig::default()),
        }
    }

    pub fn with_progressive_timeout(mut self, enable: bool, max_timeout_secs: u64) -> Self {
        self.enable_progressive_timeout = enable;
        self.max_timeout = Duration::from_secs(max_timeout_secs);
        self
    }

    pub fn with_chunked_execution(mut self, config: ChunkedExecutionConfig) -> Self {
        self.chunked_execution = ChunkedExecutionEngine::new(config);
        self
    }

    /// Calculate timeout based on gas limit
    /// For 2B gas operations, we need much longer timeouts
    fn calculate_timeout(&self, gas_limit: Option<u64>) -> Duration {
        if !self.enable_progressive_timeout {
            return self.rpc_timeout;
        }

        let gas = gas_limit.unwrap_or(21000); // Default gas for simple transfer

        // Progressive scaling:
        // - Up to 1M gas: base timeout
        // - 1M to 100M gas: scale linearly up to 10x base timeout
        // - 100M to 2B gas: scale up to max timeout
        let timeout_secs = if gas <= 1_000_000 {
            self.rpc_timeout.as_secs()
        } else if gas <= 100_000_000 {
            // Scale from base to 10x base for 1M-100M gas
            let scale_factor = (gas as f64 - 1_000_000.0) / 99_000_000.0; // 0.0 to 1.0
            let multiplier = 1.0 + (scale_factor * 9.0); // 1.0 to 10.0
            (self.rpc_timeout.as_secs() as f64 * multiplier) as u64
        } else {
            // Scale from 10x base to max timeout for 100M-2B gas
            let base_scaled = self.rpc_timeout.as_secs() * 10;
            let scale_factor = (gas as f64 - 100_000_000.0) / 1_900_000_000.0; // 0.0 to 1.0
            let additional = ((self.max_timeout.as_secs() - base_scaled) as f64 * scale_factor) as u64;
            base_scaled + additional
        };

        Duration::from_secs(timeout_secs.min(self.max_timeout.as_secs()))
    }

    /// Check if a gas limit exceeds the maximum allowed for local execution
    pub fn should_forward_call(&self, gas_limit: Option<u64>) -> bool {
        if let (Some(max_local), Some(gas)) = (self.max_local_gas_limit, gas_limit) {
            gas > max_local
        } else {
            false
        }
    }

    /// Execute eth_call with timeout
    pub async fn call_with_timeout<T>(
        &self,
        eth_api: &T,
        request: TransactionRequest,
        block_number: Option<BlockId>,
        overrides: EvmOverrides,
    ) -> Result<Bytes, ErrorObject<'static>>
    where
        T: EthCall + Send + Sync,
    {
        // Check if we should forward based on gas limit
        if self.should_forward_call(request.gas) {
            return Err(ErrorObject::owned(
                RPC_TIMEOUT_ERROR_CODE,
                "Gas limit too high for local execution. Enable --forward-call to handle high gas limit calls.",
                Some(()),
            ));
        }

        let timeout_duration = self.calculate_timeout(request.gas);
        let gas_info = request.gas.map(|g| format!(" (gas: {})", g)).unwrap_or_default();

        tracing::info!(
            "Starting eth_call with timeout: {:?}{}",
            timeout_duration,
            gas_info
        );

        // Check if we should use chunked execution for massive gas operations
        let call_future = if self.chunked_execution.should_use_chunking(request.gas) {
            tracing::info!("Using chunked execution for massive gas operation{}", gas_info);
            self.chunked_execution.chunked_call(eth_api, request, block_number, overrides)
        } else {
            async move {
                EthCall::call(eth_api, request, block_number, overrides).await
                    .map_err(|e| ErrorObject::owned(RPC_TIMEOUT_ERROR_CODE, format!("Call execution failed: {e:?}"), Some(())))
            }
        };

        match timeout(timeout_duration, call_future).await {
            Ok(result) => {
                tracing::debug!("eth_call completed successfully{}", gas_info);
                result
            },
            Err(_) => {
                tracing::warn!(
                    "eth_call timed out after {:?}{}",
                    timeout_duration,
                    gas_info
                );
                Err(ErrorObject::owned(
                    RPC_TIMEOUT_ERROR_CODE,
                    format!("Call execution timed out after {:?}{}", timeout_duration, gas_info),
                    Some(()),
                ))
            },
        }
    }

    /// Execute eth_estimateGas with timeout
    pub async fn estimate_gas_with_timeout<T>(
        &self,
        eth_api: &T,
        request: TransactionRequest,
        block_number: BlockId,
        state_override: Option<StateOverride>,
    ) -> Result<U256, ErrorObject<'static>>
    where
        T: EstimateCall + Send + Sync,
    {
        // Check if we should forward based on gas limit
        if self.should_forward_call(request.gas) {
            return Err(ErrorObject::owned(
                RPC_TIMEOUT_ERROR_CODE,
                "Gas limit too high for local execution. Enable --forward-call to handle high gas limit calls.",
                Some(()),
            ));
        }

        let timeout_duration = self.calculate_timeout(request.gas);
        let gas_info = request.gas.map(|g| format!(" (gas: {})", g)).unwrap_or_default();

        tracing::info!(
            "Starting eth_estimateGas with timeout: {:?}{}",
            timeout_duration,
            gas_info
        );

        // Check if we should use chunked execution for massive gas operations
        let estimate_future = if self.chunked_execution.should_use_chunking(request.gas) {
            tracing::info!("Using chunked gas estimation for massive gas operation{}", gas_info);
            self.chunked_execution.chunked_estimate_gas(eth_api, request, block_number, state_override)
        } else {
            async move {
                EstimateCall::estimate_gas_at(eth_api, request, block_number, state_override).await
                    .map_err(|e| ErrorObject::owned(RPC_TIMEOUT_ERROR_CODE, format!("Gas estimation failed: {e:?}"), Some(())))
            }
        };

        match timeout(timeout_duration, estimate_future).await {
            Ok(result) => {
                tracing::debug!("eth_estimateGas completed successfully{}", gas_info);
                result
            },
            Err(_) => {
                tracing::warn!(
                    "eth_estimateGas timed out after {:?}{}",
                    timeout_duration,
                    gas_info
                );
                Err(ErrorObject::owned(
                    RPC_TIMEOUT_ERROR_CODE,
                    format!("Gas estimation timed out after {:?}{}", timeout_duration, gas_info),
                    Some(()),
                ))
            },
        }
    }
}

/// Extension trait for adding timeout functionality to RPC APIs
pub trait RpcTimeoutExt {
    fn timeout_wrapper(&self) -> &TimeoutWrapper;
}
