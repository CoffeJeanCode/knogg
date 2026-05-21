//! P2P mesh layer — federation without a central hub.

pub mod client;
pub mod events;
pub mod pool;
pub mod serve;

use std::sync::{Arc, OnceLock};

pub use client::{init_from_env, with_client};
pub use events::{emit_task_done, subscribe};
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

pub fn with_pool<F, T>(f: F) -> Option<T>
where F: FnOnce(&Pool) -> T,
{
    PEER_POOL.get().map(|p| f(p))
}

pub fn pool_handle() -> Option<Arc<Pool>> {
    PEER_POOL.get().cloned()
}
