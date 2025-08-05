use crate::{
    chainspec::HlChainSpec,
    node::{
        pool::HlPoolBuilder,
        primitives::{BlockBody, HlBlock, HlBlockBody, HlPrimitives, TransactionSigned},
        rpc::{
            chunked_execution::ChunkedExecutionConfig,
            engine_api::{
                builder::HlEngineApiBuilder, payload::HlPayloadTypes,
                validator::HlEngineValidatorBuilder,
            },
            HlEthApiBuilder,
        },
        storage::HlStorage,
    },
    pseudo_peer::BlockSourceConfig,
};
use consensus::HlConsensusBuilder;
use engine::HlPayloadServiceBuilder;
use evm::HlExecutorBuilder;
use network::HlNetworkBuilder;
use reth::{
    api::{FullNodeComponents, FullNodeTypes, NodeTypes},
    builder::{
        components::ComponentsBuilder, rpc::RpcAddOns, DebugNode, Node, NodeAdapter,
        NodeComponentsBuilder,
    },
};
use reth_engine_primitives::BeaconConsensusEngineHandle;
use reth_trie_db::MerklePatriciaTrie;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

pub mod cli;
pub mod consensus;
pub mod engine;
pub mod evm;
pub mod network;
pub mod primitives;
pub mod rpc;
pub mod spot_meta;
pub mod storage;
pub mod types;

/// Hl addons configuring RPC types
pub type HlNodeAddOns<N> =
    RpcAddOns<N, HlEthApiBuilder, HlEngineValidatorBuilder, HlEngineApiBuilder>;

/// Type configuration for a regular Hl node.
#[derive(Debug, Clone)]
pub struct HlNode {
    engine_handle_rx:
        Arc<Mutex<Option<oneshot::Receiver<BeaconConsensusEngineHandle<HlPayloadTypes>>>>>,
    block_source_config: BlockSourceConfig,
    hl_node_compliant: bool,
    rpc_call_timeout: u64,
    max_local_gas_limit: Option<u64>,
    db_read_timeout: u64,
    max_concurrent_db_ops: u64,
    enable_progressive_timeout: bool,
    max_timeout_secs: u64,
    chunk_gas_limit: u64,
    chunking_threshold: u64,
}

impl HlNode {
    pub fn new(
        block_source_config: BlockSourceConfig,
        hl_node_compliant: bool,
    ) -> (Self, oneshot::Sender<BeaconConsensusEngineHandle<HlPayloadTypes>>) {
        let (tx, rx) = oneshot::channel();
        (
            Self {
                engine_handle_rx: Arc::new(Mutex::new(Some(rx))),
                block_source_config,
                hl_node_compliant,
                rpc_call_timeout: 30,
                max_local_gas_limit: None,
                db_read_timeout: 60,
                max_concurrent_db_ops: 100,
                enable_progressive_timeout: true,
                max_timeout_secs: 3600,
                chunk_gas_limit: 50_000_000,
                chunking_threshold: 100_000_000,
            },
            tx,
        )
    }

    pub fn with_rpc_timeout_config(
        mut self,
        rpc_call_timeout: u64,
        max_local_gas_limit: Option<u64>,
        db_read_timeout: u64,
        max_concurrent_db_ops: u64,
        enable_progressive_timeout: bool,
        max_timeout_secs: u64,
        chunk_gas_limit: u64,
        chunking_threshold: u64,
    ) -> Self {
        self.rpc_call_timeout = rpc_call_timeout;
        self.max_local_gas_limit = max_local_gas_limit;
        self.db_read_timeout = db_read_timeout;
        self.max_concurrent_db_ops = max_concurrent_db_ops;
        self.enable_progressive_timeout = enable_progressive_timeout;
        self.max_timeout_secs = max_timeout_secs;
        self.chunk_gas_limit = chunk_gas_limit;
        self.chunking_threshold = chunking_threshold;
        self
    }
}

mod pool;

impl HlNode {
    pub fn components<Node>(
        &self,
    ) -> ComponentsBuilder<
        Node,
        HlPoolBuilder,
        HlPayloadServiceBuilder,
        HlNetworkBuilder,
        HlExecutorBuilder,
        HlConsensusBuilder,
    >
    where
        Node: FullNodeTypes<Types = Self>,
    {
        ComponentsBuilder::default()
            .node_types::<Node>()
            .pool(HlPoolBuilder)
            .executor(HlExecutorBuilder::default())
            .payload(HlPayloadServiceBuilder::default())
            .network(HlNetworkBuilder {
                engine_handle_rx: self.engine_handle_rx.clone(),
                block_source_config: self.block_source_config.clone(),
            })
            .consensus(HlConsensusBuilder::default())
    }
}

impl NodeTypes for HlNode {
    type Primitives = HlPrimitives;
    type ChainSpec = HlChainSpec;
    type StateCommitment = MerklePatriciaTrie;
    type Storage = HlStorage;
    type Payload = HlPayloadTypes;
}

impl<N> Node<N> for HlNode
where
    N: FullNodeTypes<Types = Self>,
{
    type ComponentsBuilder = ComponentsBuilder<
        N,
        HlPoolBuilder,
        HlPayloadServiceBuilder,
        HlNetworkBuilder,
        HlExecutorBuilder,
        HlConsensusBuilder,
    >;

    type AddOns = HlNodeAddOns<
        NodeAdapter<N, <Self::ComponentsBuilder as NodeComponentsBuilder<N>>::Components>,
    >;

    fn components_builder(&self) -> Self::ComponentsBuilder {
        Self::components(self)
    }

    fn add_ons(&self) -> Self::AddOns {
        let chunked_config = ChunkedExecutionConfig {
            chunk_gas_limit: self.chunk_gas_limit,
            chunking_threshold: self.chunking_threshold,
            ..Default::default()
        };

        HlNodeAddOns::new(
            HlEthApiBuilder::default()
                .with_hl_node_compliant(self.hl_node_compliant)
                .with_rpc_call_timeout(self.rpc_call_timeout)
                .with_max_local_gas_limit(self.max_local_gas_limit)
                .with_db_read_timeout(self.db_read_timeout)
                .with_max_concurrent_db_ops(self.max_concurrent_db_ops)
                .with_progressive_timeout(self.enable_progressive_timeout, self.max_timeout_secs)
                .with_chunked_execution_config(chunked_config),
            Default::default(),
            Default::default(),
            Default::default(),
        )
    }
}

impl<N> DebugNode<N> for HlNode
where
    N: FullNodeComponents<Types = Self>,
{
    type RpcBlock = alloy_rpc_types::Block;

    fn rpc_to_primitive_block(rpc_block: Self::RpcBlock) -> HlBlock {
        let alloy_rpc_types::Block { header, transactions, withdrawals, .. } = rpc_block;
        HlBlock {
            header: header.inner,
            body: HlBlockBody {
                inner: BlockBody {
                    transactions: transactions
                        .into_transactions()
                        .map(|tx| TransactionSigned::Default(tx.inner.into_inner().into()))
                        .collect(),
                    ommers: Default::default(),
                    withdrawals,
                },
                sidecars: None,
                read_precompile_calls: None,
                highest_precompile_address: None,
            },
        }
    }
}
