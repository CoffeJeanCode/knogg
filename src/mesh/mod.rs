//! P2P mesh layer — federation without a central hub.

pub mod client;
pub mod events;
pub mod pool;
pub mod serve;

use std::sync::{Arc, OnceLock};

use pool::Pool;

static PEER_POOL: OnceLock<Arc<Pool>> = OnceLock::new();

pub fn init_pool(cfg: &crate::core::config::MeshSection) {
    let pool = Pool::new();
    for (name, addr) in &cfg.peers {
        pool.add_peer(name, addr);
    }
    if !cfg.peers.is_empty() {
        eprintln!("[mesh] {} peer(s) configured, supervisors started",
            cfg.peers.len());
    }
    let _ = PEER_POOL.set(Arc::new(pool));
}
