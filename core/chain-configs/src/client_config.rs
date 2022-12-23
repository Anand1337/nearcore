//! Chain Client Configuration
use std::cmp::max;
use std::cmp::min;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::MutableConfigValue;
use near_primitives::types::{
    AccountId, BlockHeight, BlockHeightDelta, Gas, NumBlocks, NumSeats, ShardId,
};
use near_primitives::version::Version;

pub const TEST_STATE_SYNC_TIMEOUT: u64 = 5;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum LogSummaryStyle {
    #[serde(rename = "plain")]
    Plain,
    #[serde(rename = "colored")]
    Colored,
}

/// Minimum number of epochs for which we keep store data
pub const MIN_GC_NUM_EPOCHS_TO_KEEP: u64 = 3;

/// Default number of epochs for which we keep store data
pub const DEFAULT_GC_NUM_EPOCHS_TO_KEEP: u64 = 5;

/// Configuration for garbage collection.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GCConfig {
    /// Maximum number of blocks to garbage collect at every garbage collection
    /// call.
    #[serde(default = "default_gc_blocks_limit")]
    pub gc_blocks_limit: NumBlocks,

    /// Maximum number of height to go through at each garbage collection step
    /// when cleaning forks during garbage collection.
    #[serde(default = "default_gc_fork_clean_step")]
    pub gc_fork_clean_step: u64,

    /// Number of epochs for which we keep store data.
    #[serde(default = "default_gc_num_epochs_to_keep")]
    pub gc_num_epochs_to_keep: u64,
}

impl Default for GCConfig {
    fn default() -> Self {
        Self {
            gc_blocks_limit: 2,
            gc_fork_clean_step: 100,
            gc_num_epochs_to_keep: DEFAULT_GC_NUM_EPOCHS_TO_KEEP,
        }
    }
}

fn default_gc_blocks_limit() -> NumBlocks {
    GCConfig::default().gc_blocks_limit
}

fn default_gc_fork_clean_step() -> u64 {
    GCConfig::default().gc_fork_clean_step
}

fn default_gc_num_epochs_to_keep() -> u64 {
    GCConfig::default().gc_num_epochs_to_keep()
}

impl GCConfig {
    pub fn gc_num_epochs_to_keep(&self) -> u64 {
        max(MIN_GC_NUM_EPOCHS_TO_KEEP, self.gc_num_epochs_to_keep)
    }
}

#[derive(Clone, Serialize, Deserialize)]
/// ClientConfig with all fields immutable.
/// Can be serialized/deserialized, making it possible to read and write it from a file.
pub struct StaticClientConfig {
    /// Version of the binary.
    pub version: Version,
    /// Chain id for status.
    pub chain_id: String,
    /// Listening rpc port for status.
    pub rpc_addr: Option<String>,
    /// Graceful shutdown at expected block height.
    pub expected_shutdown: Option<BlockHeight>,
    /// Duration to check for producing / skipping block.
    pub block_production_tracking_delay: Duration,
    /// Minimum duration before producing block.
    pub min_block_production_delay: Duration,
    /// Maximum wait for approvals before producing block.
    pub max_block_production_delay: Duration,
    /// Maximum duration before skipping given height.
    pub max_block_wait_delay: Duration,
    /// Duration to reduce the wait for each missed block by validator.
    pub reduce_wait_for_missing_block: Duration,
    /// Skip waiting for sync (for testing or single node testnet).
    pub skip_sync_wait: bool,
    /// How often to check that we are not out of sync.
    pub sync_check_period: Duration,
    /// While syncing, how long to check for each step.
    pub sync_step_period: Duration,
    /// Sync height threshold: below this difference in height don't start syncing.
    pub sync_height_threshold: BlockHeightDelta,
    /// How much time to wait after initial header sync
    pub header_sync_initial_timeout: Duration,
    /// How much time to wait after some progress is made in header sync
    pub header_sync_progress_timeout: Duration,
    /// How much time to wait before banning a peer in header sync if sync is too slow
    pub header_sync_stall_ban_timeout: Duration,
    /// Expected increase of header head weight per second during header sync
    pub header_sync_expected_height_per_second: u64,
    /// How long to wait for a response during state sync
    pub state_sync_timeout: Duration,
    /// Minimum number of peers to start syncing.
    pub min_num_peers: usize,
    /// Period between logging summary information.
    pub log_summary_period: Duration,
    /// Enable coloring of the logs
    pub log_summary_style: LogSummaryStyle,
    /// Produce empty blocks, use `false` for testing.
    pub produce_empty_blocks: bool,
    /// Epoch length.
    pub epoch_length: BlockHeightDelta,
    /// Number of block producer seats
    pub num_block_producer_seats: NumSeats,
    /// Maximum blocks ahead of us before becoming validators to announce account.
    pub announce_account_horizon: BlockHeightDelta,
    /// Time to persist Accounts Id in the router without removing them.
    pub ttl_account_id_router: Duration,
    /// Horizon at which instead of fetching block, fetch full state.
    pub block_fetch_horizon: BlockHeightDelta,
    /// Horizon to step from the latest block when fetching state.
    pub state_fetch_horizon: NumBlocks,
    /// Time between check to perform catchup.
    pub catchup_step_period: Duration,
    /// Time between checking to re-request chunks.
    pub chunk_request_retry_period: Duration,
    /// Time between running doomslug timer.
    pub doosmslug_step_period: Duration,
    /// Behind this horizon header fetch kicks in.
    pub block_header_fetch_horizon: BlockHeightDelta,
    /// Garbage collection configuration.
    pub gc: GCConfig,
    /// Accounts that this client tracks
    pub tracked_accounts: Vec<AccountId>,
    /// Shards that this client tracks
    pub tracked_shards: Vec<ShardId>,
    /// Not clear old data, set `true` for archive nodes.
    pub archive: bool,
    /// Save trie changes. Must be set to true if either of the following is true
    /// - archive is false - non archival nodes need trie changes for garbage collection
    /// - the node will be migrated to split storage in the near future - split storage nodes need trie changes for hot storage garbage collection
    pub save_trie_changes: bool,
    /// Number of threads for ViewClientActor pool.
    pub view_client_threads: usize,
    /// Run Epoch Sync on the start.
    pub epoch_sync_enabled: bool,
    /// Number of seconds between state requests for view client.
    pub view_client_throttle_period: Duration,
    /// Upper bound of the byte size of contract state that is still viewable. None is no limit
    pub trie_viewer_state_size_limit: Option<u64>,
    /// Max burnt gas per view method.  If present, overrides value stored in
    /// genesis file.  The value only affects the RPCs without influencing the
    /// protocol thus changing it per-node doesn’t affect the blockchain.
    pub max_gas_burnt_view: Option<Gas>,
    /// Re-export storage layer statistics as prometheus metrics.
    pub enable_statistics_export: bool,
    /// Number of threads to execute background migration work in client.
    pub client_background_migration_threads: usize,
}

/// ClientConfig where some fields can be updated at runtime.
#[derive(Clone)]
pub struct ClientConfig {
    /// Version of the binary.
    pub version: Version,
    /// Chain id for status.
    pub chain_id: String,
    /// Listening rpc port for status.
    pub rpc_addr: Option<String>,
    /// Graceful shutdown at expected block height.
    pub expected_shutdown: MutableConfigValue<Option<BlockHeight>>,
    /// Duration to check for producing / skipping block.
    pub block_production_tracking_delay: MutableConfigValue<Duration>,
    /// Minimum duration before producing block.
    pub min_block_production_delay: MutableConfigValue<Duration>,
    /// Maximum wait for approvals before producing block.
    pub max_block_production_delay: MutableConfigValue<Duration>,
    /// Maximum duration before skipping given height.
    pub max_block_wait_delay: MutableConfigValue<Duration>,
    /// Duration to reduce the wait for each missed block by validator.
    pub reduce_wait_for_missing_block: Duration,
    /// Skip waiting for sync (for testing or single node testnet).
    pub skip_sync_wait: bool,
    /// How often to check that we are not out of sync.
    pub sync_check_period: Duration,
    /// While syncing, how long to check for each step.
    pub sync_step_period: Duration,
    /// Sync height threshold: below this difference in height don't start syncing.
    pub sync_height_threshold: BlockHeightDelta,
    /// How much time to wait after initial header sync
    pub header_sync_initial_timeout: Duration,
    /// How much time to wait after some progress is made in header sync
    pub header_sync_progress_timeout: Duration,
    /// How much time to wait before banning a peer in header sync if sync is too slow
    pub header_sync_stall_ban_timeout: Duration,
    /// Expected increase of header head weight per second during header sync
    pub header_sync_expected_height_per_second: u64,
    /// How long to wait for a response during state sync
    pub state_sync_timeout: Duration,
    /// Minimum number of peers to start syncing.
    pub min_num_peers: usize,
    /// Period between logging summary information.
    pub log_summary_period: Duration,
    /// Enable coloring of the logs
    pub log_summary_style: LogSummaryStyle,
    /// Produce empty blocks, use `false` for testing.
    pub produce_empty_blocks: bool,
    /// Epoch length.
    pub epoch_length: BlockHeightDelta,
    /// Number of block producer seats
    pub num_block_producer_seats: NumSeats,
    /// Maximum blocks ahead of us before becoming validators to announce account.
    pub announce_account_horizon: BlockHeightDelta,
    /// Time to persist Accounts Id in the router without removing them.
    pub ttl_account_id_router: Duration,
    /// Horizon at which instead of fetching block, fetch full state.
    pub block_fetch_horizon: BlockHeightDelta,
    /// Horizon to step from the latest block when fetching state.
    pub state_fetch_horizon: NumBlocks,
    /// Time between check to perform catchup.
    pub catchup_step_period: Duration,
    /// Time between checking to re-request chunks.
    pub chunk_request_retry_period: Duration,
    /// Time between running doomslug timer.
    pub doosmslug_step_period: Duration,
    /// Behind this horizon header fetch kicks in.
    pub block_header_fetch_horizon: BlockHeightDelta,
    /// Garbage collection configuration.
    pub gc: GCConfig,
    /// Accounts that this client tracks
    pub tracked_accounts: Vec<AccountId>,
    /// Shards that this client tracks
    pub tracked_shards: Vec<ShardId>,
    /// Not clear old data, set `true` for archive nodes.
    pub archive: bool,
    /// Save trie changes. Must be set to true if either of the following is true
    /// - archive is false - non archival nodes need trie changes for garbage collection
    /// - the node will be migrated to split storage in the near future - split storage nodes need trie changes for hot storage garbage collection
    pub save_trie_changes: bool,
    /// Number of threads for ViewClientActor pool.
    pub view_client_threads: usize,
    /// Run Epoch Sync on the start.
    pub epoch_sync_enabled: bool,
    /// Number of seconds between state requests for view client.
    pub view_client_throttle_period: Duration,
    /// Upper bound of the byte size of contract state that is still viewable. None is no limit
    pub trie_viewer_state_size_limit: Option<u64>,
    /// Max burnt gas per view method.  If present, overrides value stored in
    /// genesis file.  The value only affects the RPCs without influencing the
    /// protocol thus changing it per-node doesn’t affect the blockchain.
    pub max_gas_burnt_view: Option<Gas>,
    /// Re-export storage layer statistics as prometheus metrics.
    pub enable_statistics_export: bool,
    /// Number of threads to execute background migration work in client.
    pub client_background_migration_threads: usize,
}

impl StaticClientConfig {
    pub fn test(
        skip_sync_wait: bool,
        min_block_prod_time: u64,
        max_block_prod_time: u64,
        num_block_producer_seats: NumSeats,
        archive: bool,
        save_trie_changes: bool,
        epoch_sync_enabled: bool,
    ) -> Self {
        Self {
        assert!(
            archive || save_trie_changes,
            "Configuration with archive = false and save_trie_changes = false is not supported \
            because non-archival nodes must save trie changes in order to do do garbage collection."
        );

        Self {
            version: Default::default(),
            chain_id: "unittest".to_string(),
            rpc_addr: Some("0.0.0.0:3030".to_string()),
            expected_shutdown: None,
            block_production_tracking_delay: Duration::from_millis(std::cmp::max(
                10,
                min_block_prod_time / 5,
            )),
            min_block_production_delay: Duration::from_millis(min_block_prod_time),
            max_block_production_delay: Duration::from_millis(max_block_prod_time),
            max_block_wait_delay: Duration::from_millis(3 * min_block_prod_time),
            reduce_wait_for_missing_block: Duration::from_millis(0),
            skip_sync_wait,
            sync_check_period: Duration::from_millis(100),
            sync_step_period: Duration::from_millis(10),
            sync_height_threshold: 1,
            header_sync_initial_timeout: Duration::from_secs(10),
            header_sync_progress_timeout: Duration::from_secs(2),
            header_sync_stall_ban_timeout: Duration::from_secs(30),
            state_sync_timeout: Duration::from_secs(TEST_STATE_SYNC_TIMEOUT),
            header_sync_expected_height_per_second: 1,
            min_num_peers: 1,
            log_summary_period: Duration::from_secs(10),
            produce_empty_blocks: true,
            epoch_length: 10,
            num_block_producer_seats,
            announce_account_horizon: 5,
            ttl_account_id_router: Duration::from_secs(60 * 60),
            block_fetch_horizon: 50,
            state_fetch_horizon: 5,
            catchup_step_period: Duration::from_millis(1),
            chunk_request_retry_period: min(
                Duration::from_millis(100),
                Duration::from_millis(min_block_prod_time / 5),
            ),
            doosmslug_step_period: Duration::from_millis(100),
            block_header_fetch_horizon: 50,
            gc: GCConfig { gc_blocks_limit: 100, ..GCConfig::default() },
            tracked_accounts: vec![],
            tracked_shards: vec![],
            archive,
            save_trie_changes,
            log_summary_style: LogSummaryStyle::Colored,
            view_client_threads: 1,
            epoch_sync_enabled,
            view_client_throttle_period: Duration::from_secs(1),
            trie_viewer_state_size_limit: None,
            max_gas_burnt_view: None,
            enable_statistics_export: true,
            client_background_migration_threads: 1,
        }
    }
}

impl ClientConfig {
    pub fn new(config: StaticClientConfig) -> Self {
        Self {
            version: config.version,
            chain_id: config.chain_id,
            rpc_addr: config.rpc_addr,
            expected_shutdown: MutableConfigValue::new(config.expected_shutdown),
            block_production_tracking_delay: MutableConfigValue::new(
                config.block_production_tracking_delay,
            ),
            min_block_production_delay: MutableConfigValue::new(config.min_block_production_delay),
            max_block_production_delay: MutableConfigValue::new(config.max_block_production_delay),
            max_block_wait_delay: MutableConfigValue::new(config.max_block_wait_delay),
            reduce_wait_for_missing_block: config.reduce_wait_for_missing_block,
            skip_sync_wait: config.skip_sync_wait,
            sync_check_period: config.sync_check_period,
            sync_step_period: config.sync_step_period,
            sync_height_threshold: config.sync_height_threshold,
            header_sync_initial_timeout: config.header_sync_initial_timeout,
            header_sync_progress_timeout: config.header_sync_progress_timeout,
            header_sync_stall_ban_timeout: config.header_sync_stall_ban_timeout,
            state_sync_timeout: config.state_sync_timeout,
            header_sync_expected_height_per_second: config.header_sync_expected_height_per_second,
            min_num_peers: config.min_num_peers,
            log_summary_period: config.log_summary_period,
            produce_empty_blocks: config.produce_empty_blocks,
            epoch_length: config.epoch_length,
            num_block_producer_seats: config.num_block_producer_seats,
            announce_account_horizon: config.announce_account_horizon,
            ttl_account_id_router: config.ttl_account_id_router,
            block_fetch_horizon: config.block_fetch_horizon,
            state_fetch_horizon: config.state_fetch_horizon,
            catchup_step_period: config.catchup_step_period,
            chunk_request_retry_period: config.chunk_request_retry_period,
            doosmslug_step_period: config.doosmslug_step_period,
            block_header_fetch_horizon: config.block_header_fetch_horizon,
            gc: config.gc,
            tracked_accounts: config.tracked_accounts,
            tracked_shards: config.tracked_shards,
            archive: config.archive,
            save_trie_changes: config.save_trie_changes,
            log_summary_style: config.log_summary_style,
            view_client_threads: config.view_client_threads,
            epoch_sync_enabled: config.epoch_sync_enabled,
            view_client_throttle_period: config.view_client_throttle_period,
            trie_viewer_state_size_limit: config.trie_viewer_state_size_limit,
            max_gas_burnt_view: config.max_gas_burnt_view,
            enable_statistics_export: config.enable_statistics_export,
            client_background_migration_threads: config.client_background_migration_threads,
        }
    }
}
