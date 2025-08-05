use alloy_eips::BlockId;
use alloy_primitives::{Bytes, U256};
use alloy_rpc_types_eth::{
    state::{EvmOverrides, StateOverride},
    transaction::TransactionRequest,
    BlockOverrides,
};
use jsonrpsee::{
    http_client::{HttpClient, HttpClientBuilder},
    proc_macros::rpc,
    rpc_params,
    types::{error::INTERNAL_ERROR_CODE, ErrorObject},
};
use jsonrpsee_core::{async_trait, client::ClientT, ClientError, RpcResult};
use reth_rpc_eth_api::helpers::EthCall;
use crate::node::rpc::timeout::RpcTimeoutExt;

#[rpc(server, namespace = "eth")]
pub(crate) trait CallForwarderApi {
    /// Executes a new message call immediately without creating a transaction on the block chain.
    #[method(name = "call")]
    async fn call(
        &self,
        request: TransactionRequest,
        block_number: Option<BlockId>,
        state_overrides: Option<StateOverride>,
        block_overrides: Option<Box<BlockOverrides>>,
    ) -> RpcResult<Bytes>;

    /// Generates and returns an estimate of how much gas is necessary to allow the transaction to
    /// complete.
    #[method(name = "estimateGas")]
    async fn estimate_gas(
        &self,
        request: TransactionRequest,
        block_number: Option<BlockId>,
        state_override: Option<StateOverride>,
    ) -> RpcResult<U256>;
}

pub struct CallForwarderExt<EthApi> {
    upstream_client: HttpClient,
    eth_api: EthApi,
}

impl<EthApi> CallForwarderExt<EthApi> {
    pub fn new(upstream_rpc_url: String, eth_api: EthApi) -> Self {
        let upstream_client =
            HttpClientBuilder::default().build(upstream_rpc_url).expect("Failed to build client");

        Self { upstream_client, eth_api }
    }
}

#[async_trait]
impl<EthApi> CallForwarderApiServer for CallForwarderExt<EthApi>
where
    EthApi: EthCall + Send + Sync + 'static,
{
    async fn call(
        &self,
        request: TransactionRequest,
        block_number: Option<BlockId>,
        state_overrides: Option<StateOverride>,
        block_overrides: Option<Box<BlockOverrides>>,
    ) -> RpcResult<Bytes> {
        let is_latest = block_number.as_ref().map(|b| b.is_latest()).unwrap_or(true);
        let result = if is_latest {
            self.upstream_client
                .request(
                    "eth_call",
                    rpc_params![request, block_number, state_overrides, block_overrides],
                )
                .await
                .map_err(|e| match e {
                    ClientError::Call(e) => e,
                    _ => ErrorObject::owned(
                        INTERNAL_ERROR_CODE,
                        format!("Failed to call: {e:?}"),
                        Some(()),
                    ),
                })?
        } else {
            // Use timeout wrapper for local calls
            self.eth_api
                .timeout_wrapper()
                .call_with_timeout(
                    &self.eth_api,
                    request,
                    block_number,
                    EvmOverrides::new(state_overrides, block_overrides),
                )
                .await?
        };

        Ok(result)
    }

    async fn estimate_gas(
        &self,
        request: TransactionRequest,
        block_number: Option<BlockId>,
        state_override: Option<StateOverride>,
    ) -> RpcResult<U256> {
        let is_latest = block_number.as_ref().map(|b| b.is_latest()).unwrap_or(true);
        let result = if is_latest {
            self.upstream_client
                .request("eth_estimateGas", rpc_params![request, block_number, state_override])
                .await
                .map_err(|e| match e {
                    ClientError::Call(e) => e,
                    _ => ErrorObject::owned(
                        INTERNAL_ERROR_CODE,
                        format!("Failed to estimate gas: {e:?}"),
                        Some(()),
                    ),
                })?
        } else {
            // Use timeout wrapper for local gas estimation
            self.eth_api
                .timeout_wrapper()
                .estimate_gas_with_timeout(
                    &self.eth_api,
                    request,
                    block_number.unwrap_or_default(),
                    state_override,
                )
                .await?
        };

        Ok(result)
    }
}
