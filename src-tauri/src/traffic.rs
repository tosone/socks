//! Tunnel traffic stats.
//!
//! The extension publishes its byte counters to `<shared>/traffic.json`; this
//! module polls that file and turns the counters into per-second speeds plus
//! lifetime totals for the UI.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::{sync::Mutex, task::JoinHandle};

use crate::session::TrafficTotals;

/// File the extension writes, inside the App Group container.
pub const SHARED_FILE_NAME: &str = "traffic.json";

/// How often the shared file is polled.
const POLL_INTERVAL: Duration = Duration::from_millis(1_000);

/// How often accumulated totals are written to disk.
const PERSIST_INTERVAL: Duration = Duration::from_secs(10);

/// A snapshot older than this means the extension is gone: report zero speed
/// rather than a misleading value.
const STALE_AFTER: Duration = Duration::from_secs(3);

/// Event shape expected by the frontend (`src/types.ts`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrafficEvent {
    profile_id: String,
    tx: u64,
    rx: u64,
    up_bps: u64,
    down_bps: u64,
    total_tx: u64,
    total_rx: u64,
}

/// One `traffic.json` snapshot published by the extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrafficSnapshot {
    tx: u64,
    rx: u64,
    updated_at_ms: u64,
}

impl TrafficSnapshot {
    /// A snapshot written more than `stale_after` ago belongs to a tunnel that
    /// is no longer running (the extension can be killed without notice).
    fn is_stale(self, now_ms: u64, stale_after: Duration) -> bool {
        now_ms.saturating_sub(self.updated_at_ms) > stale_after.as_millis() as u64
    }
}

/// Turns a stream of cumulative counters into per-sample deltas.
///
/// The counters restart at zero whenever the data plane restarts (a reconnect or
/// a network change), so a decrease is treated as a reset and yields no delta.
#[derive(Debug, Default)]
struct TrafficDelta {
    last: Option<(u64, u64)>,
}

impl TrafficDelta {
    fn update(&mut self, tx: u64, rx: u64) -> (u64, u64) {
        let delta = match self.last {
            Some((prev_tx, prev_rx)) if tx >= prev_tx && rx >= prev_rx => {
                (tx - prev_tx, rx - prev_rx)
            }
            _ => (0, 0),
        };
        self.last = Some((tx, rx));
        delta
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn read_snapshot(path: &Path) -> Option<TrafficSnapshot> {
    let raw = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some(TrafficSnapshot {
        tx: value.get("tx")?.as_u64()?,
        rx: value.get("rx")?.as_u64()?,
        updated_at_ms: value.get("updatedAtMs")?.as_u64()?,
    })
}

/// Poll the extension's byte counters until the returned handle is aborted.
///
/// Totals are accumulated into `totals` and persisted to `data_dir` every
/// [`PERSIST_INTERVAL`] so a crash does not lose more than a few seconds.
pub fn spawn_poll(
    app: AppHandle,
    profile_id: String,
    shared_file: PathBuf,
    totals: Arc<Mutex<HashMap<String, TrafficTotals>>>,
    data_dir: PathBuf,
) -> JoinHandle<()> {
    // Drop the stale remains of a previous session so the first poll cannot read
    // counters left behind by an earlier tunnel.
    let _ = fs::remove_file(&shared_file);

    tokio::spawn(async move {
        let mut delta = TrafficDelta::default();
        let mut ticker = tokio::time::interval(POLL_INTERVAL);
        let mut last_persist = Instant::now();

        loop {
            ticker.tick().await;

            let live = read_snapshot(&shared_file)
                .filter(|snapshot| !snapshot.is_stale(now_ms(), STALE_AFTER));

            let (delta_tx, delta_rx) = match live {
                Some(snapshot) => delta.update(snapshot.tx, snapshot.rx),
                None => (0, 0),
            };

            let window_ms = POLL_INTERVAL.as_millis().max(1) as u64;
            let up_bps = delta_tx.saturating_mul(1_000) / window_ms;
            let down_bps = delta_rx.saturating_mul(1_000) / window_ms;

            let totals_snapshot = {
                let mut guard = totals.lock().await;
                let entry = guard.entry(profile_id.clone()).or_default();
                entry.tx = entry.tx.saturating_add(delta_tx);
                entry.rx = entry.rx.saturating_add(delta_rx);
                entry.clone()
            };

            let _ = app.emit(
                "traffic",
                TrafficEvent {
                    profile_id: profile_id.clone(),
                    tx: live.map(|snapshot| snapshot.tx).unwrap_or(0),
                    rx: live.map(|snapshot| snapshot.rx).unwrap_or(0),
                    up_bps,
                    down_bps,
                    total_tx: totals_snapshot.tx,
                    total_rx: totals_snapshot.rx,
                },
            );

            if last_persist.elapsed() >= PERSIST_INTERVAL {
                if let Err(err) = crate::session::save_traffic_totals(
                    &data_dir,
                    &profile_id,
                    &totals_snapshot,
                ) {
                    eprintln!("failed to persist traffic totals: {err}");
                }
                last_persist = Instant::now();
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_follow_the_counters() {
        let mut delta = TrafficDelta::default();

        assert_eq!(delta.update(100, 200), (0, 0), "first sample has no baseline");
        assert_eq!(delta.update(160, 240), (60, 40));
        assert_eq!(delta.update(160, 240), (0, 0), "no traffic, no delta");
    }

    #[test]
    fn counter_reset_yields_no_delta() {
        let mut delta = TrafficDelta::default();
        delta.update(1_000, 2_000);

        // The data plane restarted, so the counters went back to zero.
        assert_eq!(delta.update(10, 20), (0, 0));
        // ... and counting resumes from the new baseline.
        assert_eq!(delta.update(30, 50), (20, 30));
    }

    #[test]
    fn stale_snapshots_are_detected() {
        let snapshot = TrafficSnapshot {
            tx: 1,
            rx: 2,
            updated_at_ms: 1_000,
        };

        assert!(!snapshot.is_stale(1_500, STALE_AFTER));
        assert!(!snapshot.is_stale(4_000, STALE_AFTER), "exactly at the limit");
        assert!(snapshot.is_stale(4_001, STALE_AFTER));
    }

    /// The field names must stay in sync with the extension's writer
    /// (`RustRelay.publishTraffic` in PacketTunnelProvider.swift).
    #[test]
    fn reads_the_document_the_extension_publishes() {
        let path = std::env::temp_dir().join(format!(
            "socks-traffic-snapshot-{}.json",
            std::process::id()
        ));
        fs::write(&path, r#"{"tx":120,"rx":340,"updatedAtMs":5000}"#).unwrap();

        assert_eq!(
            read_snapshot(&path),
            Some(TrafficSnapshot {
                tx: 120,
                rx: 340,
                updated_at_ms: 5000,
            })
        );

        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_missing_document_reads_as_nothing() {
        assert_eq!(
            read_snapshot(Path::new("/nonexistent/socks-traffic.json")),
            None
        );
    }
}
