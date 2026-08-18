use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use ipnet::IpNet;
use shadowsocks_service::acl::AccessControl;
use shadowsocks_service::config::{
    Config, ConfigType, LocalConfig, LocalInstanceConfig, ProtocolType, ServerInstanceConfig,
};
use shadowsocks_service::local::dns::NameServerAddr;
use shadowsocks_service::local::Server;
use shadowsocks_service::net::FlowStat;
use shadowsocks_service::shadowsocks::config::{Mode, ServerAddr, ServerConfig};
use shadowsocks_service::shadowsocks::crypto::CipherKind;
use shadowsocks_service::shadowsocks::plugin::PluginConfig;
use shadowsocks_service::shadowsocks::relay::socks5::Address;

use crate::error::{AppError, AppResult};
use crate::password;
use crate::profile::Profile;

pub const TUN_ADDRESS: &str = "10.255.0.1/24";
pub const DNS_RELAY_PORT: u16 = 15353;
pub const REMOTE_DNS_PORT: u16 = 53;
const ACL_PATH: &str = ".config/socks/shadowsocks.acl";

pub struct LocalRuntime {
    pub server: Server,
    pub flow_stat: std::sync::Arc<FlowStat>,
}

pub fn build_server_config(
    profile: &Profile,
    outbound_bind_interface: Option<String>,
    bundled_plugin_dir: Option<&Path>,
    tun_fd_path: Option<&Path>,
    local_dns_ip: IpAddr,
) -> AppResult<Config> {
    let method = CipherKind::from_str(&profile.method)
        .map_err(|_| AppError::msg(format!("Unknown encryption method: {}", profile.method)))?;

    let addr = if let Ok(ip) = profile.server.parse::<std::net::IpAddr>() {
        ServerAddr::SocketAddr(SocketAddr::new(ip, profile.port))
    } else {
        ServerAddr::DomainName(profile.server.clone(), profile.port)
    };

    let password = password::normalize_for_method(&profile.method, &profile.password)?;
    eprintln!(
        "[socks] build sslocal config server={}:{} method={} normalized_password_len={} outbound_if={:?}",
        profile.server,
        profile.port,
        profile.method,
        password.len(),
        outbound_bind_interface
    );
    let mut server_cfg = ServerConfig::new(addr, password, method)
        .map_err(|err| AppError::msg(format!("Invalid server configuration: {err}")))?;
    server_cfg.set_mode(Mode::TcpAndUdp);

    if let Some(plugin) = profile.plugin.as_ref() {
        let resolved_plugin = resolve_plugin_path(plugin, bundled_plugin_dir);
        eprintln!(
            "[socks] build sslocal plugin path={} opts={:?}",
            resolved_plugin, profile.plugin_opts
        );
        server_cfg.set_plugin(PluginConfig {
            plugin: resolved_plugin,
            plugin_opts: profile.plugin_opts.clone(),
            plugin_args: Vec::new(),
            plugin_mode: Mode::TcpAndUdp,
        });
    } else {
        eprintln!("[socks] build sslocal plugin disabled");
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
    #[cfg(unix)]
    if let Some(path) = tun_fd_path {
        eprintln!("[socks] build sslocal tun fd path={}", path.display());
        local.tun_device_fd_from_path = Some(path.to_path_buf());
    }

    let mut config = Config::new(ConfigType::Local);
    config
        .local
        .push(LocalInstanceConfig::with_local_config(local));

    let mut dns = LocalConfig::new(ProtocolType::Dns);
    dns.addr = Some(ServerAddr::SocketAddr(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        DNS_RELAY_PORT,
    )));
    dns.mode = Mode::TcpAndUdp;
    dns.local_dns_addr = Some(NameServerAddr::SocketAddr(SocketAddr::new(
        local_dns_ip,
        REMOTE_DNS_PORT,
    )));
    dns.remote_dns_addr = Some(Address::from(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
        REMOTE_DNS_PORT,
    )));
    eprintln!(
        "[socks] build sslocal dns bind=127.0.0.1:{} local_dns={}:{} remote_dns=8.8.8.8:{} mode={:?}",
        DNS_RELAY_PORT,
        local_dns_ip,
        REMOTE_DNS_PORT,
        REMOTE_DNS_PORT,
        dns.mode
    );
    config
        .local
        .push(LocalInstanceConfig::with_local_config(dns));

    config.server.push(instance);
    config.outbound_bind_interface = outbound_bind_interface;
    if let Some(acl_path) = user_acl_path() {
        config.acl = Some(AccessControl::load_from_file(&acl_path).map_err(|err| {
            AppError::msg(format!("Failed to load ACL {}: {err}", acl_path.display()))
        })?);
    }
    Ok(config)
}

fn user_acl_path() -> Option<PathBuf> {
    let path = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(ACL_PATH))?;

    path.is_file().then_some(path)
}

fn resolve_plugin_path(plugin: &str, bundled_plugin_dir: Option<&Path>) -> String {
    let plugin_path = Path::new(plugin);
    if plugin_path.is_absolute() || plugin_path.components().count() > 1 {
        return plugin.to_string();
    }

    bundled_plugin_dir
        .map(|dir| dir.join(plugin))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(plugin))
        .to_string_lossy()
        .into_owned()
}

pub async fn start_local(config: Config) -> AppResult<LocalRuntime> {
    let server = Server::new(config).await.map_err(|err| {
        let message = err.to_string();
        if message.contains("Operation not permitted") || message.contains("os error 1") {
            return AppError::msg(format!(
                "Failed to start sslocal: {message}. The privileged helper is installed for route and DNS changes, but TUN creation still runs inside the app process and was denied by macOS."
            ));
        }
        AppError::msg(format!("Failed to start sslocal: {message}"))
    })?;
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
