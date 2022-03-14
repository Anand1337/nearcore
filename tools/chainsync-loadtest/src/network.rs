use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use anyhow::{anyhow};

use crate::concurrency::{Ctx, Once, RateLimiter, Scope, WeakMap};

use near_network_primitives::types::{
    NetworkViewClientMessages, NetworkViewClientResponses,
    PartialEncodedChunkRequestMsg, PartialEncodedChunkResponseMsg,
};

use actix::{Actor, Context, Handler};
use log::{info,warn};
use crate::peer_manager::types::{
    FullPeerInfo, NetworkClientMessages, NetworkClientResponses, NetworkInfo, NetworkRequests, NetworkResponses,
    PeerManagerAdapter, PeerManagerMessageRequest, PeerManagerMessageResponse,
};
use near_primitives::block::{Block, BlockHeader, GenesisId};
use near_primitives::hash::CryptoHash;
use near_primitives::sharding::{ChunkHash, ShardChunkHeader};
use near_primitives::network::PeerId;
use nearcore::config::NearConfig;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tokio::time;
use std::collections::HashMap;

fn genesis_hash(chain_id: &str) -> CryptoHash {
    return match chain_id {
        "mainnet" => "EPnLgE7iEq9s7yTkos96M3cWymH5avBAPm3qx3NXqR8H",
        "testnet" => "FWJ9kR6KFWoyMoNjpLXXGHeuiy7tEY6GmoFeCA5yuc6b",
        "betanet" => "6hy7VoEJhPEUaJr1d5ePBhKdgeDWKCjLoUAn7XS9YPj",
        _ => {
            return Default::default();
        }
    }
    .parse()
    .unwrap();
}

#[derive(Default)]
pub struct PeerStats {
    pub requests: u32, 
    pub responses: u32,
    pub total_latency: time::Duration,
}

#[derive(Default)]
pub struct RequestStats {
    pub requests: u64,
    pub total_sends: u64,
    pub total_latency: time::Duration,
}

#[derive(Default)]
pub struct PeerStatsMap {
    pub requests: Mutex<RequestStats>,
    pub peers: Mutex<HashMap<PeerId,PeerStats>>,
}

impl PeerStatsMap {
    fn add_response_time(&self, send_times: &SendTimes, peer_id: &PeerId) {
        {
            let mut rs = self.requests.lock().unwrap();
            rs.requests += 1;
            rs.total_sends += send_times.sends.load(Ordering::Relaxed);
            send_times.times.lock().unwrap().values().min().map(|t|{
                rs.total_latency += time::Instant::now()-*t;
            });
        }
        {
            let mut ps = self.peers.lock().unwrap();
            for p in send_times.get_peers() {
                ps.entry(p).or_default().requests += 1;
            }
            if let Some(l) = send_times.latency(peer_id) {
                let mut stats = ps.entry(peer_id.clone()).or_default();
                stats.responses += 1;
                stats.total_latency += l;
            } else {
                // Response without request. THESE ARE SUSPICIOUS AND SHOULD BE DEBUGGED.
                warn!("response without request from {}",peer_id);
                ps.entry(peer_id.clone()).or_default().responses += 1;
            }
        }
    }
}

impl fmt::Debug for PeerStats {
    fn fmt(&self, f :&mut fmt::Formatter<'_>) -> Result<(),fmt::Error> {
        let resp = self.responses;
        let avg = if resp==0 { time::Duration::ZERO } else { self.total_latency/resp };
        f.write_str(&format!("{}/{} avg {:?}",self.responses,self.requests,avg))
    }
}

impl fmt::Debug for PeerStatsMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(),fmt::Error> {
        let mut m = f.debug_map();
        for (_k,v) in self.peers.lock().unwrap().iter() {
            m.entry(&0,v);
        }
        m.finish()
    }
}

#[derive(Default, Debug)]
pub struct Stats {
    pub msgs_sent: AtomicU64,
    pub msgs_send_failures: AtomicU64,
    pub msgs_recv: AtomicU64,

    pub header_start: AtomicU64,
    pub header_done: AtomicU64,
    pub block_start: AtomicU64,
    pub block_done: AtomicU64,
    pub chunk_start: AtomicU64,
    pub chunk_done: AtomicU64,

    pub peers : PeerStatsMap,
}

#[derive(Default)]
struct SendTimes {
    sends: AtomicU64,
    times: Mutex<HashMap<PeerId,time::Instant>>,
}

impl SendTimes {
    fn register(&self, peer_id :&PeerId) {
        self.sends.fetch_add(1,Ordering::Relaxed);
        let mut st = self.times.lock().unwrap();
        st.entry(peer_id.clone()).or_insert_with(time::Instant::now);
    }
    fn get_peers(&self) -> Vec<PeerId> {
        self.times.lock().unwrap().keys().map(|p|p.clone()).collect()
    }
    fn latency(&self, peer_id :&PeerId) -> Option<time::Duration> {
        self.times.lock().unwrap().get(peer_id).map(|t|time::Instant::now()-*t)
    }
}

struct Request<T> {
    send_times : Arc<SendTimes>,
    once : Once<T>,
}

impl<T:Clone+Send+Sync> Request<T> {
    fn new() -> Request<T> {
        Request{
            send_times: Default::default(),
            once: Once::new(),
        }
    }
}

// NetworkData contains the mutable private data of the Network struct.
// TODO: consider replacing the vector of oneshot Senders with a single
// Notify/Once.
struct NetworkData {
    info_futures: Vec<oneshot::Sender<Arc<NetworkInfo>>>,
    info_: Arc<NetworkInfo>,
}

// Network encapsulates PeerManager and exposes an async API for sending RPCs.
pub struct Network {
    pub stats: Stats,
    network_adapter: Arc<dyn PeerManagerAdapter>,
    block_headers: Arc<WeakMap<CryptoHash, Request<Vec<BlockHeader>>>>,
    blocks: Arc<WeakMap<CryptoHash, Request<Block>>>,
    chunks: Arc<WeakMap<ChunkHash, Request<PartialEncodedChunkResponseMsg>>>,
    data: Mutex<NetworkData>,

    chain_id: String,
    // client_config.min_num_peers
    min_peers: usize,
    // Currently it is equivalent to genesis_config.num_block_producer_seats,
    // (see https://cs.github.com/near/nearcore/blob/dae9553670de13c279d3ebd55f17da13d94fa691/nearcore/src/runtime/mod.rs#L1114).
    // AFAICT eventually it will change dynamically (I guess it will be provided in the Block).
    parts_per_chunk: u64,

    request_timeout: tokio::time::Duration,
    rate_limiter: RateLimiter,
}

impl Network {
    pub fn new(
        config: &NearConfig,
        network_adapter: Arc<dyn PeerManagerAdapter>,
        qps_limit: u32,
    ) -> Arc<Network> {
        Arc::new(Network {
            stats: Default::default(),
            network_adapter,
            data: Mutex::new(NetworkData {
                info_: Arc::new(NetworkInfo {
                    connected_peers: vec![],
                    num_connected_peers: 0,
                    peer_max_count: 0,
                    highest_height_peers: vec![],
                    sent_bytes_per_sec: 0,
                    received_bytes_per_sec: 0,
                    known_producers: vec![],
                    peer_counter: 0,
                }),
                info_futures: Default::default(),
            }),
            blocks: WeakMap::new(),
            block_headers: WeakMap::new(),
            chunks: WeakMap::new(),

            chain_id: config.client_config.chain_id.clone(),
            min_peers: config.client_config.min_num_peers,
            parts_per_chunk: config.genesis.config.num_block_producer_seats,
            rate_limiter: RateLimiter::new(
                time::Duration::from_secs(1) / qps_limit,
                qps_limit as u64,
            ),
            request_timeout: time::Duration::from_secs(10),
        })
    }

    // keep_sending() sends periodically (every self.request_timeout)
    // a NetworkRequest produced by <new_req> in an infinite loop.
    // The requests are distributed uniformly among all the available peers.
    // - keep_sending() completes as soon as ctx expires.
    // - keep_sending() respects the global rate limits, so the actual frequency
    //   of the sends may be lower than expected.
    // - keep_sending() may pause if the number of connected peers is too small.
    fn keep_sending(
        self: &Arc<Self>,
        ctx: &Ctx,
        send_times: Arc<SendTimes>,
        new_req: impl Fn(FullPeerInfo) -> NetworkRequests + Send,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        let self_ = self.clone();
        let ctx = ctx.with_label("keep_sending");
        async move {
            loop {
                let mut peers = self_.info(&ctx).await?.connected_peers.clone();
                peers.shuffle(&mut thread_rng());
                for peer in peers {
                    // TODO: rate limit per peer.
                    self_.rate_limiter.allow(&ctx).await?;
                    send_times.register(&peer.peer_info.id);
                    let send = self_
                        .network_adapter
                        .send(PeerManagerMessageRequest::NetworkRequests(new_req(peer.clone())));
                    match send.await? {
                        PeerManagerMessageResponse::NetworkResponses(NetworkResponses::NoResponse) => {
                            self_.stats.msgs_sent.fetch_add(1, Ordering::Relaxed);
                            ctx.wait(self_.request_timeout).await?;
                        }
                        PeerManagerMessageResponse::NetworkResponses(NetworkResponses::RouteNotFound) => {
                            self_.stats.msgs_send_failures.fetch_add(1, Ordering::Relaxed);
                        }
                        status => { return Err(anyhow!("{:?}",status)); }
                    }
                }
            }
        }
    }

    // info() fetches the state of the newest available NetworkInfo.
    // It blocks if the number of connected peers is too small.
    pub async fn info(self: &Arc<Self>, ctx: &Ctx) -> anyhow::Result<Arc<NetworkInfo>> {
        let ctx = ctx.clone();
        let (send, recv) = oneshot::channel();
        {
            let mut n = self.data.lock().unwrap();
            if n.info_.num_connected_peers >= self.min_peers {
                let _ = send.send(n.info_.clone());
            } else {
                n.info_futures.push(send);
            }
        }
        anyhow::Ok(ctx.wrap(recv).await??)
    }

    // fetch_block_headers fetches a batch of headers, starting with the header
    // AFTER the header with the given <hash>. The batch size is bounded by
    // sync::MAX_BLOCK_HEADERS = (currently) 512
    // https://github.com/near/nearcore/blob/ad896e767b0efbce53060dd145836bbeda3d656b/chain/client/src/sync.rs#L38
    pub async fn fetch_block_headers(
        self: &Arc<Self>,
        ctx: &Ctx,
        hash: &CryptoHash,
    ) -> anyhow::Result<Vec<BlockHeader>> {
        Scope::run(ctx, {
            let self_ = self.clone();
            let hash = hash.clone();
            move |ctx, s| async move {
                self_.stats.header_start.fetch_add(1, Ordering::Relaxed);
                let recv = self_.block_headers.get_or_insert(&hash, || Request::new());
                s.spawn_weak({
                    let self_ = &self_;
                    let send_times = recv.send_times.clone();
                    move |ctx| self_.keep_sending(&ctx, send_times, move |peer| NetworkRequests::BlockHeadersRequest {
                        hashes: vec![hash.clone()],
                        peer_id: peer.peer_info.id.clone(),
                    })
                });
                let res = ctx.wrap(recv.once.wait()).await?;
                self_.stats.header_done.fetch_add(1, Ordering::Relaxed);
                anyhow::Ok(res)
            }
        })
        .await
    }

    // fetch_block() fetches a block with a given hash.
    pub async fn fetch_block(
        self: &Arc<Self>,
        ctx: &Ctx,
        hash: &CryptoHash,
    ) -> anyhow::Result<Block> {
        Scope::run(ctx, {
            let self_ = self.clone();
            let hash = hash.clone();
            move |ctx, s| async move {
                self_.stats.block_start.fetch_add(1, Ordering::Relaxed);
                let recv = self_.blocks.get_or_insert(&hash, || Request::new());
                s.spawn_weak({
                    let self_ = &self_;
                    let send_times = recv.send_times.clone();
                    move |ctx| self_.keep_sending(&ctx, send_times, move |peer| NetworkRequests::BlockRequest {
                        hash: hash.clone(),
                        peer_id: peer.peer_info.id.clone(),
                    })
                });
                let res = ctx.wrap(recv.once.wait()).await?;
                self_.stats.block_done.fetch_add(1, Ordering::Relaxed);
                anyhow::Ok(res)
            }
        })
        .await
    }

    // fetch_chunk fetches a chunk for the given chunk header.
    pub async fn fetch_chunk(
        self: &Arc<Self>,
        ctx: &Ctx,
        ch: &ShardChunkHeader,
    ) -> anyhow::Result<PartialEncodedChunkResponseMsg> {
        Scope::run(ctx, {
            let self_ = self.clone();
            let ch = ch.clone();
            move |ctx, s| async move {
                let recv = self_.chunks.get_or_insert(&ch.chunk_hash(), || Request::new());
                // TODO: consider converting wrapping these atomic counters into sth like a Span.
                self_.stats.chunk_start.fetch_add(1, Ordering::Relaxed);
                s.spawn_weak({
                    let self_= &self_;
                    let send_times = recv.send_times.clone();
                    move |ctx| self_.keep_sending(&ctx, send_times, {
                        let ppc = self_.parts_per_chunk;
                        move |peer| NetworkRequests::PartialEncodedChunkRequest {
                            target: peer.peer_info.id.clone(),
                            request: PartialEncodedChunkRequestMsg {
                                chunk_hash: ch.chunk_hash(),
                                part_ords: (0..ppc).collect(),
                                tracking_shards: Default::default(),
                            },
                        }
                    })
                });
                let res = ctx.wrap(recv.once.wait()).await?;
                self_.stats.chunk_done.fetch_add(1, Ordering::Relaxed);
                anyhow::Ok(res)
            }
        })
        .await
    }

    fn notify(&self, msg: NetworkClientMessages) {
        self.stats.msgs_recv.fetch_add(1, Ordering::Relaxed);
        match msg {
            NetworkClientMessages::NetworkInfo(info) => {
                let mut n = self.data.lock().unwrap();
                n.info_ = Arc::new(info);
                if n.info_.num_connected_peers < self.min_peers {
                    info!("connected = {}/{}", n.info_.num_connected_peers, self.min_peers);
                    return;
                }
                for s in n.info_futures.split_off(0) {
                    s.send(n.info_.clone()).unwrap();
                }
            }
            NetworkClientMessages::Block(block, peer_id, _) => {
                self.blocks.get(&block.hash().clone()).map(|r|{
                    if let Ok(_) = r.once.set(block) {
                        self.stats.peers.add_response_time(&r.send_times,&peer_id);
                    }
                });
            }
            NetworkClientMessages::BlockHeaders(headers, peer_id) => {
                if let Some(h) = headers.iter().min_by_key(|h| h.height()) {
                    let hash = h.prev_hash().clone();
                    self.block_headers.get(&hash).map(|r|{
                        if let Ok(_) = r.once.set(headers) {
                            self.stats.peers.add_response_time(&r.send_times,&peer_id);
                        }
                    });
                }
            }
            NetworkClientMessages::PartialEncodedChunkResponse(resp,peer_id) => {
                self.chunks.get(&resp.chunk_hash.clone()).map(|r|{
                    if let Ok(_) = r.once.set(resp) {
                        self.stats.peers.add_response_time(&r.send_times,&peer_id);
                    }
                });
            }
            _ => {}
        }
    }
}

pub struct FakeClientActor {
    network: Arc<Network>,
}

impl FakeClientActor {
    pub fn new(network: Arc<Network>) -> Self {
        FakeClientActor { network }
    }
}

impl Actor for FakeClientActor {
    type Context = Context<Self>;
}

impl Handler<NetworkViewClientMessages> for FakeClientActor {
    type Result = NetworkViewClientResponses;
    fn handle(&mut self, msg: NetworkViewClientMessages, _ctx: &mut Self::Context) -> Self::Result {
        let name = match msg {
            NetworkViewClientMessages::TxStatus { .. } => "TxStatus",
            NetworkViewClientMessages::TxStatusResponse(_) => "TxStatusResponse",
            NetworkViewClientMessages::ReceiptOutcomeRequest(_) => "ReceiptOutcomeRequest",
            NetworkViewClientMessages::ReceiptOutcomeResponse(_) => "ReceiptOutputResponse",
            NetworkViewClientMessages::BlockRequest(_) => "BlockRequest",
            NetworkViewClientMessages::BlockHeadersRequest(_) => "BlockHeadersRequest",
            NetworkViewClientMessages::StateRequestHeader { .. } => "StateRequestHeader",
            NetworkViewClientMessages::StateRequestPart { .. } => "StateRequestPart",
            NetworkViewClientMessages::EpochSyncRequest { .. } => "EpochSyncRequest",
            NetworkViewClientMessages::EpochSyncFinalizationRequest { .. } => {
                "EpochSyncFinalizationRequest"
            }
            NetworkViewClientMessages::GetChainInfo => {
                return NetworkViewClientResponses::ChainInfo {
                    genesis_id: GenesisId {
                        chain_id: self.network.chain_id.clone(),
                        hash: genesis_hash(&self.network.chain_id),
                    },
                    height: 0,
                    tracked_shards: Default::default(),
                    archival: false,
                }
            }
            NetworkViewClientMessages::AnnounceAccount(_) => {
                return NetworkViewClientResponses::NoResponse;
            }
            #[allow(unreachable_patterns)]
            _ => "unknown",
        };
        info!("view_request: {}", name);
        return NetworkViewClientResponses::NoResponse;
    }
}

impl Handler<NetworkClientMessages> for FakeClientActor {
    type Result = NetworkClientResponses;
    fn handle(&mut self, msg: NetworkClientMessages, _ctx: &mut Context<Self>) -> Self::Result {
        self.network.notify(msg);
        return NetworkClientResponses::NoResponse;
    }
}
