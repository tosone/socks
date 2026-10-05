//! Lifecycle of the Shadowsocks data plane.

use std::{
    net::IpAddr,
    os::raw::c_void,
    sync::{Arc, Mutex},
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

/// Handle to a running data plane.
struct Engine {
    runtime: Runtime,
    tx: mpsc::UnboundedSender<Bytes>,
    events: EventCallback,
}

/// Only one tunnel can run at a time, and the FFI calls are synchronous.
static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

pub(crate) fn start(
    config: StartConfig,
    tunnel_address: &str,
    tunnel_netmask: &str,
    send: SocksSendFn,
    event: SocksEventFn,
    ctx: *mut c_void,
) -> Result<(), String> {
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

    runtime.spawn(async move {
        if let Err(err) = tun.run().await {
            log::error!("tun service stopped with error: {err}");
            run_events.emit(&error_event(&err.to_string()));
        }
    });

    events.emit(&status_event("started"));

    let mut guard = ENGINE.lock().map_err(|_| "core state is poisoned".to_owned())?;
    *guard = Some(Engine { runtime, tx, events });
    Ok(())
}

pub(crate) fn stop() -> Result<(), String> {
    let engine = {
        let mut guard = ENGINE.lock().map_err(|_| "core state is poisoned".to_owned())?;
        guard.take()
    };

    if let Some(engine) = engine {
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

pub(crate) fn notify_network_changed() -> Result<(), String> {
    // TODO(M5): drop in-flight TCP/UDP state so the stack reconnects on the new
    // path. shadowsocks-service has no hook for this yet.
    Ok(())
}

fn status_event(status: &str) -> String {
    serde_json::json!({ "type": "status", "status": status }).to_string()
}

fn error_event(message: &str) -> String {
    serde_json::json!({ "type": "error", "message": message }).to_string()
}
