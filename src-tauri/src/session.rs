use std::collections::HashMap;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::error::{AppError, AppResult};
use crate::helper::{self, HelperStatus};
use crate::macos_route;
use crate::profile::{self, Profile, ProfileInput};
use crate::sslocal;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub active_profile_id: Option<String>,
    pub tun_name: Option<String>,
    pub helper_installed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityEvent {
    pub profile_id: String,
    pub status: ConnectivityStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectivityStatus {
    Checking,
    Connected,
    Failed,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficTotals {
    pub tx: u64,
    pub rx: u64,
}

struct RuntimeSession {
    profile_id: String,
    tun_name: String,
    connectivity_task: JoinHandle<()>,
}

pub struct AppState {
    data_dir: PathBuf,
    app: AppHandle,
    profiles: Arc<Mutex<Vec<Profile>>>,
    traffic_totals: Arc<Mutex<HashMap<String, TrafficTotals>>>,
    session: Mutex<Option<RuntimeSession>>,
}

impl AppState {
    pub fn load(data_dir: PathBuf, app: AppHandle) -> AppResult<Self> {
        let profiles = profile::load_profiles(&data_dir)?;
        let traffic_totals = profiles
            .iter()
            .map(|profile| {
                (
                    profile.id.clone(),
                    load_traffic_totals(&data_dir, &profile.id).unwrap_or_default(),
                )
            })
            .collect();
        Ok(Self {
            data_dir,
            app,
            profiles: Arc::new(Mutex::new(profiles)),
            traffic_totals: Arc::new(Mutex::new(traffic_totals)),
            session: Mutex::new(None),
        })
    }

    pub async fn list_profiles(&self) -> Vec<Profile> {
        self.profiles.lock().await.clone()
    }

    pub async fn list_traffic_totals(&self) -> HashMap<String, TrafficTotals> {
        let profiles = self.profiles.lock().await;
        let totals = self.traffic_totals.lock().await;
        profiles
            .iter()
            .map(|profile| {
                (
                    profile.id.clone(),
                    totals.get(&profile.id).cloned().unwrap_or_default(),
                )
            })
            .collect()
    }

    pub async fn create_profile(&self, input: ProfileInput) -> AppResult<Profile> {
        let created = profile::create_profile(input)?;
        let mut profiles = self.profiles.lock().await;
        profiles.push(created.clone());
        profile::save_profiles(&self.data_dir, &profiles)?;
        self.traffic_totals
            .lock()
            .await
            .insert(created.id.clone(), TrafficTotals::default());
        save_traffic_totals(&self.data_dir, &created.id, &TrafficTotals::default())?;
        Ok(created)
    }

    pub async fn update_profile(&self, id: &str, input: ProfileInput) -> AppResult<Profile> {
        let mut profiles = self.profiles.lock().await;
        let profile = profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::msg("Profile not found"))?;
        profile::apply_update(profile, input)?;
        let updated = profile.clone();
        profile::save_profiles(&self.data_dir, &profiles)?;
        drop(profiles);
        if self.active_id().await.as_deref() == Some(id) {
            self.connect(id).await?;
        }
        Ok(updated)
    }

    pub async fn delete_profile(&self, id: &str) -> AppResult<()> {
        if self.active_id().await.as_deref() == Some(id) {
            self.disconnect().await?;
        }
        let mut profiles = self.profiles.lock().await;
        let before = profiles.len();
        profiles.retain(|p| p.id != id);
        if profiles.len() == before {
            return Err(AppError::msg("Profile not found"));
        }
        profile::save_profiles(&self.data_dir, &profiles)?;
        self.traffic_totals.lock().await.remove(id);
        remove_traffic_totals(&self.data_dir, id)?;
        Ok(())
    }

    pub async fn runtime_status(&self) -> RuntimeStatus {
        let session = self.session.lock().await;
        RuntimeStatus {
            active_profile_id: session.as_ref().map(|s| s.profile_id.clone()),
            tun_name: session.as_ref().map(|s| s.tun_name.clone()),
            helper_installed: helper::helper_status().installed,
        }
    }

    pub fn helper_status(&self) -> HelperStatus {
        helper::helper_status()
    }

    pub fn install_helper(&self) -> AppResult<HelperStatus> {
        helper::install_helper()
    }

    pub async fn uninstall_helper(&self) -> AppResult<HelperStatus> {
        self.disconnect().await?;
        helper::uninstall_helper()
    }

    pub async fn connect(&self, id: &str) -> AppResult<RuntimeStatus> {
        if !helper::helper_status().installed {
            return Err(AppError::msg(
                "Install the helper first. After that, connecting will not ask for an administrator password.",
            ));
        }
        self.disconnect().await?;

        let profile = {
            let profiles = self.profiles.lock().await;
            profiles
                .iter()
                .find(|p| p.id == id)
                .cloned()
                .ok_or_else(|| AppError::msg("Profile not found"))?
        };

        let snapshot = macos_route::current_default_route()?;
        let server_ip = sslocal::resolve_server_ip(&profile.server, profile.port).await?;
        let local_dns_ip = local_dns_ip(&snapshot)?;
        eprintln!(
            "[socks] connect profile id={} name={} server={}:{} method={} plugin={} plugin_opts={} password_len={}",
            profile.id,
            profile.name,
            profile.server,
            profile.port,
            profile.method,
            profile.plugin.as_deref().unwrap_or("<none>"),
            profile
                .plugin_opts
                .as_ref()
                .map(|opts| format!("{} bytes", opts.len()))
                .unwrap_or_else(|| "<none>".to_string()),
            profile.password.len()
        );
        eprintln!(
            "[socks] route snapshot gateway={} interface={} original_dns={:?} selected_local_dns={} server_ip={}",
            snapshot.gateway,
            snapshot.interface,
            snapshot.dns.as_ref().map(|dns| &dns.servers),
            local_dns_ip,
            server_ip
        );
        let bundled_plugin_dir = self
            .app
            .path()
            .resource_dir()
            .ok()
            .map(|path| path.join("plugins"));
        let acl_path = sslocal::default_acl_path();
        let tun_name = helper::start_runtime(helper::StartRuntimeInput {
            profile: profile.clone(),
            outbound_interface: snapshot.interface.clone(),
            server_ip,
            gateway: snapshot.gateway.clone(),
            dns: snapshot.dns.clone(),
            local_dns_ip,
            bundled_plugin_dir,
            acl_path,
        })?;
        eprintln!("[socks] helper runtime started tun={tun_name}");
        let connectivity_task = spawn_connectivity_check(self.app.clone(), profile.id.clone());

        let mut session = self.session.lock().await;
        *session = Some(RuntimeSession {
            profile_id: profile.id,
            tun_name,
            connectivity_task,
        });
        drop(session);
        Ok(self.runtime_status().await)
    }

    pub async fn disconnect(&self) -> AppResult<RuntimeStatus> {
        let mut session = self.session.lock().await;
        if let Some(current) = session.take() {
            current.connectivity_task.abort();
            helper::stop_runtime()?;
            if let Some(total) = self
                .traffic_totals
                .lock()
                .await
                .get(&current.profile_id)
                .cloned()
            {
                save_traffic_totals(&self.data_dir, &current.profile_id, &total)?;
            }
        }
        drop(session);
        Ok(self.runtime_status().await)
    }

    pub async fn shutdown(&self) {
        let _ = self.disconnect().await;
    }

    async fn active_id(&self) -> Option<String> {
        self.session
            .lock()
            .await
            .as_ref()
            .map(|s| s.profile_id.clone())
    }
}

fn spawn_connectivity_check(app: AppHandle, profile_id: String) -> JoinHandle<()> {
    tokio::spawn(async move {
        let _ = app.emit(
            "connectivity",
            ConnectivityEvent {
                profile_id: profile_id.clone(),
                status: ConnectivityStatus::Checking,
                message: None,
            },
        );

        let result = check_dns_google().await;
        let (status, message) = match result {
            Ok(()) => (ConnectivityStatus::Connected, None),
            Err(err) => (ConnectivityStatus::Failed, Some(err.to_string())),
        };
        let _ = app.emit(
            "connectivity",
            ConnectivityEvent {
                profile_id,
                status,
                message,
            },
        );
    })
}

async fn check_dns_google() -> AppResult<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|err| AppError::msg(format!("Failed to create HTTP client: {err}")))?;
    let response = client
        .get("https://8.8.8.8")
        .header(reqwest::header::HOST, "dns.google")
        .send()
        .await
        .map_err(|err| AppError::msg(format!("Failed to reach https://8.8.8.8: {err}")))?;
    if !response.status().is_success() {
        return Err(AppError::msg(format!(
            "https://8.8.8.8 returned HTTP {}",
            response.status()
        )));
    }
    tokio::net::lookup_host(("dns.google", 443))
        .await
        .map_err(|err| AppError::msg(format!("Failed to resolve dns.google: {err}")))?
        .next()
        .ok_or_else(|| AppError::msg("dns.google resolved to no addresses"))?;
    Ok(())
}

fn local_dns_ip(snapshot: &macos_route::RouteSnapshot) -> AppResult<IpAddr> {
    if let Some(ip) = snapshot.dns.as_ref().and_then(|dns| {
        dns.servers
            .iter()
            .find_map(|server| server.parse::<IpAddr>().ok())
    }) {
        return Ok(ip);
    }
    snapshot
        .gateway
        .parse::<IpAddr>()
        .map_err(|err| AppError::msg(format!("Failed to choose local DNS server: {err}")))
}

fn traffic_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("traffic")
}

fn traffic_path(data_dir: &Path, profile_id: &str) -> PathBuf {
    traffic_dir(data_dir).join(format!("{profile_id}.json"))
}

fn load_traffic_totals(data_dir: &Path, profile_id: &str) -> AppResult<TrafficTotals> {
    let path = traffic_path(data_dir, profile_id);
    if !path.exists() {
        return Ok(TrafficTotals::default());
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(TrafficTotals::default());
    }
    Ok(serde_json::from_str(&raw)?)
}

fn save_traffic_totals(data_dir: &Path, profile_id: &str, totals: &TrafficTotals) -> AppResult<()> {
    fs::create_dir_all(traffic_dir(data_dir))?;
    let raw = serde_json::to_string_pretty(totals)?;
    fs::write(traffic_path(data_dir, profile_id), raw)?;
    Ok(())
}

fn remove_traffic_totals(data_dir: &Path, profile_id: &str) -> AppResult<()> {
    let path = traffic_path(data_dir, profile_id);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}
