use std::net::SocketAddr;
use std::str::FromStr;

use ipnet::IpNet;
use shadowsocks_service::config::{
    Config, ConfigType, LocalConfig, LocalInstanceConfig, ProtocolType, ServerInstanceConfig,
};
use shadowsocks_service::local::Server;
use shadowsocks_service::net::FlowStat;
use shadowsocks_service::shadowsocks::config::{Mode, ServerAddr, ServerConfig};
use shadowsocks_service::shadowsocks::crypto::CipherKind;
use shadowsocks_service::shadowsocks::plugin::PluginConfig;

use crate::error::{AppError, AppResult};
use crate::profile::Profile;

pub const TUN_ADDRESS: &str = "10.255.0.1/24";

pub struct LocalRuntime {
    pub server: Server,
    pub flow_stat: std::sync::Arc<FlowStat>,
}

pub fn build_server_config(
    profile: &Profile,
    outbound_bind_interface: Option<String>,
) -> AppResult<Config> {
    let method = CipherKind::from_str(&profile.method)
        .map_err(|_| AppError::msg(format!("Unknown encryption method: {}", profile.method)))?;

    let addr = if let Ok(ip) = profile.server.parse::<std::net::IpAddr>() {
        ServerAddr::SocketAddr(SocketAddr::new(ip, profile.port))
    } else {
        ServerAddr::DomainName(profile.server.clone(), profile.port)
    };

    let mut server_cfg = ServerConfig::new(addr, profile.password.clone(), method)
        .map_err(|err| AppError::msg(format!("Invalid server configuration: {err}")))?;
    server_cfg.set_mode(Mode::TcpAndUdp);

    if let Some(plugin) = profile.plugin.as_ref() {
        server_cfg.set_plugin(PluginConfig {
            plugin: plugin.clone(),
            plugin_opts: profile.plugin_opts.clone(),
            plugin_args: Vec::new(),
            plugin_mode: Mode::TcpAndUdp,
        });
    }

    let mut instance = ServerInstanceConfig::with_server_config(server_cfg);
    instance.outbound_bind_interface = outbound_bind_interface.clone();

    let mut local = LocalConfig::new(ProtocolType::Tun);
    local.mode = Mode::TcpAndUdp;
    local.tun_interface_address = Some(
        TUN_ADDRESS
            .parse::<IpNet>()
            .map_err(|err| AppError::msg(format!("Invalid TUN address: {err}")))?,
    );

    let mut config = Config::new(ConfigType::Local);
    config
        .local
        .push(LocalInstanceConfig::with_local_config(local));
    config.server.push(instance);
    config.outbound_bind_interface = outbound_bind_interface;
    Ok(config)
}

pub async fn start_local(config: Config) -> AppResult<LocalRuntime> {
    let server = Server::new(config)
        .await
        .map_err(|err| AppError::msg(format!("Failed to start sslocal: {err}")))?;
    let flow_stat = server.server_balancer().context().flow_stat();
    Ok(LocalRuntime { server, flow_stat })
}

pub async fn resolve_server_ip(host: &str, port: u16) -> AppResult<String> {
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Ok(ip.to_string());
    }

    let mut addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|err| AppError::msg(format!("Failed to resolve the server address: {err}")))?;
    addrs
        .next()
        .map(|addr| addr.ip().to_string())
        .ok_or_else(|| AppError::msg("Could not resolve the server address"))
}
