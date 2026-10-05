//! Lifecycle of the Shadowsocks data plane.

use std::{
    net::IpAddr,
    os::raw::c_void,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use shadowsocks_service::{
    config::ServerInstanceConfig,
    local::{context::ServiceContext, loadbalancing::PingBalancerBuilder, tun::TunBuilder},
};
use tokio::{runtime::Runtime, sync::mpsc};

use crate::{
    config::StartConfig,
    device::{EventCallback, SendCallback, SocksEventFn, SocksPacket, SocksSendFn, VirtualDevice},
};

/// Everything needed to bring the data plane back up after a network change.
#[derive(Clone)]
struct StartParams {
    config: StartConfig,
    tunnel_address: String,
    tunnel_netmask: String,
    log_dir: Option<String>,
    send: SocksSendFn,
    event: SocksEventFn,
    ctx: *mut c_void,
}

// SAFETY: `ctx` is the caller-provided opaque pointer. The FFI contract requires
// it to stay valid until the core is stopped, which is the same guarantee
// `SendCallback`/`EventCallback` already rely on.
unsafe impl Send for StartParams {}

/// Handle to a running data plane.
struct Engine {
    runtime: Runtime,
    tx: mpsc::UnboundedSender<Bytes>,
    events: EventCallback,
    params: StartParams,
    /// Set by [`stop`] so the tun task can tell a deliberate shutdown apart from
    /// a real failure and skip the error log/event.
    shutdown: Arc<AtomicBool>,
}

/// Only one tunnel can run at a time, and the FFI calls are synchronous.
static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

pub(crate) fn start(
    config: StartConfig,
    tunnel_address: &str,
    tunnel_netmask: &str,
    log_dir: Option<String>,
    send: SocksSendFn,
    event: SocksEventFn,
    ctx: *mut c_void,
) -> Result<(), String> {
    // Install the rolling log first so startup failures are captured too.
    // Reconfiguring an already installed logger is a no-op whenever the settings
    // are unchanged, which matters because a network change restarts the core.
    if let Some(dir) = log_dir.as_deref() {
        if let Err(err) = crate::logging::install(Path::new(dir), config.log_level_filter()) {
            log::warn!("failed to install the rolling log in {dir}: {err}");
        }
    }

    // Make `start` idempotent: a second start replaces the first tunnel.
    let _ = stop();

    let mode = config.mode();
    let server = config.server_config()?;

    let address = tunnel_address
        .parse::<IpAddr>()
        .map_err(|err| format!("invalid tunnel address {tunnel_address:?}: {err}"))?;
    let netmask = tunnel_netmask
        .parse::<IpAddr>()
        .map_err(|err| format!("invalid tunnel netmask {tunnel_netmask:?}: {err}"))?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("socks-core")
        .build()
        .map_err(|err| format!("failed to create runtime: {err}"))?;

    let (tx, rx) = mpsc::unbounded_channel();
    let device = VirtualDevice::new(address, netmask, rx, SendCallback::new(send, ctx));

    let udp_timeout = config.udp_timeout.map(Duration::from_secs);
    let udp_capacity = config.udp_max_associations;

    let tun = runtime.block_on(async move {
        let context = Arc::new(ServiceContext::new());

        let mut balancer_builder = PingBalancerBuilder::new(context.clone(), mode);
        balancer_builder.add_server(ServerInstanceConfig::with_server_config(server));
        let balancer = balancer_builder
            .build()
            .await
            .map_err(|err| format!("failed to build server balancer: {err}"))?;

        let mut builder = TunBuilder::new(context, balancer);
        builder.mode(mode);
        if let Some(timeout) = udp_timeout {
            builder.udp_expiry_duration(timeout);
        }
        if let Some(capacity) = udp_capacity {
            builder.udp_capacity(capacity);
        }

        builder
            .build_with_device(Box::new(device))
            .await
            .map_err(|err| format!("failed to build tun service: {err}"))
    })?;

    let events = EventCallback::new(event, ctx);
    let run_events = events;
    let shutdown = Arc::new(AtomicBool::new(false));
    let run_shutdown = shutdown.clone();

    runtime.spawn(async move {
        if let Err(err) = tun.run().await {
            if run_shutdown.load(Ordering::Relaxed) {
                // Expected: the channel is closed and the runtime dropped on stop.
                log::debug!("tun service stopped");
            } else {
                log::error!("tun service stopped with error: {err}");
                run_events.emit(&error_event(&err.to_string()));
            }
        }
    });

    events.emit(&status_event("started"));

    let mut guard = ENGINE.lock().map_err(|_| "core state is poisoned".to_owned())?;
    *guard = Some(Engine {
        runtime,
        tx,
        events,
        params: StartParams {
            config,
            tunnel_address: tunnel_address.to_owned(),
            tunnel_netmask: tunnel_netmask.to_owned(),
            log_dir,
            send,
            event,
            ctx,
        },
        shutdown,
    });
    Ok(())
}

pub(crate) fn stop() -> Result<(), String> {
    let engine = {
        let mut guard = ENGINE.lock().map_err(|_| "core state is poisoned".to_owned())?;
        guard.take()
    };

    if let Some(engine) = engine {
        // Let the tun task know this is deliberate before the channel closes.
        engine.shutdown.store(true, Ordering::Relaxed);
        // Closing the channel makes the device read side fail, and dropping the
        // runtime aborts the tun task.
        drop(engine.tx);
        engine.events.emit(&status_event("stopped"));
        drop(engine.runtime);
    }

    Ok(())
}

/// Enqueue packets read from the tunnel so the device can consume them.
///
/// # Safety
///
/// `packets` must point to `count` valid [`SocksPacket`] entries, each with a
/// `data` pointer valid for `len` bytes.
pub(crate) unsafe fn push(packets: *const SocksPacket, count: usize) -> Result<(), String> {
    if count == 0 {
        return Ok(());
    }
    if packets.is_null() {
        return Err("packets must not be null".to_owned());
    }

    let guard = ENGINE.lock().map_err(|_| "core state is poisoned".to_owned())?;
    let engine = guard.as_ref().ok_or_else(|| "core is not running".to_owned())?;

    for index in 0..count {
        let packet = &*packets.add(index);
        if packet.data.is_null() || packet.len == 0 {
            continue;
        }
        let bytes = std::slice::from_raw_parts(packet.data, packet.len);
        engine
            .tx
            .send(Bytes::copy_from_slice(bytes))
            .map_err(|_| "core is not running".to_owned())?;
    }

    Ok(())
}

/// The host's default network path changed (Wi-Fi switch, sleep/wake, ...).
///
/// In-flight TCP/UDP state is invalid after such a switch and
/// `shadowsocks-service` exposes no reset hook for it, so restart the whole
/// data plane: every association and outbound socket is dropped and new
/// connections pick up the new path. The tunnel interface itself stays up.
pub(crate) fn notify_network_changed() -> Result<(), String> {
    let params = {
        let guard = ENGINE.lock().map_err(|_| "core state is poisoned".to_owned())?;
        guard
            .as_ref()
            .ok_or_else(|| "core is not running".to_owned())?
            .params
            .clone()
    };

    log::info!("default network path changed; restarting the data plane");
    // `start` is idempotent: it stops any previous instance first.
    let StartParams {
        config,
        tunnel_address,
        tunnel_netmask,
        log_dir,
        send,
        event,
        ctx,
    } = params;
    start(
        config,
        &tunnel_address,
        &tunnel_netmask,
        log_dir,
        send,
        event,
        ctx,
    )
}

fn status_event(status: &str) -> String {
    serde_json::json!({ "type": "status", "status": status }).to_string()
}

fn error_event(message: &str) -> String {
    serde_json::json!({ "type": "error", "message": message }).to_string()
}
