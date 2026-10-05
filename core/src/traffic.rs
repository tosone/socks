//! Tunnel byte counters.
//!
//! These count what crosses the tunnel, which is what the UI shows: `tx` is what
//! the system handed to the tunnel (uploads), `rx` is what the tunnel handed
//! back (downloads).
//!
//! `shadowsocks-service`'s own `FlowStat` cannot be reused here: it is only
//! incremented by the server-side `mon_stream`/`mon_socket` wrappers, and the
//! `local-tun` module never touches it.

use std::sync::atomic::{AtomicU64, Ordering};

/// Byte counters for one running tunnel.
#[derive(Debug, Default)]
pub struct TrafficCounters {
    tx: AtomicU64,
    rx: AtomicU64,
}

impl TrafficCounters {
    /// Record bytes received from the tunnel (an upload).
    pub fn add_tx(&self, bytes: u64) {
        self.tx.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record bytes sent back to the tunnel (a download).
    pub fn add_rx(&self, bytes: u64) {
        self.rx.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Current `(tx, rx)` totals since the tunnel was started.
    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.tx.load(Ordering::Relaxed),
            self.rx.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_each_direction_independently() {
        let counters = TrafficCounters::default();
        counters.add_tx(100);
        counters.add_rx(40);
        counters.add_tx(1);

        assert_eq!(counters.snapshot(), (101, 40));
    }
}
