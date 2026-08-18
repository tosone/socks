use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use shadowsocks_service::net::FlowStat;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::error::{AppError, AppResult};
use crate::helper::{self, HelperStatus};
use crate::macos_route::{self, AppliedRoutes};
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
pub struct TrafficEvent {
    pub profile_id: String,
    pub tx: u64,
    pub rx: u64,
    pub up_bps: u64,
    pub down_bps: u64,
    pub total_tx: u64,
    pub total_rx: u64,
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
    routes: Option<AppliedRoutes>,
    server_task: JoinHandle<()>,
    poller_task: JoinHandle<()>,
    connectivity_task: JoinHandle<()>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeState {
    routes: AppliedRoutes,
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
        if helper::helper_status().installed {
            let _ = recover_runtime_state(&data_dir);
        }

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
        recover_runtime_state(&self.data_dir)?;

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
        let bundled_plugin_dir = self
            .app
            .path()
            .resource_dir()
            .ok()
            .map(|path| path.join("plugins"));
        let tun_fd_path = self.data_dir.join("tun-fd.sock");
        let _ = fs::remove_file(&tun_fd_path);
        let tun_fd_task = {
            let tun_fd_path = tun_fd_path.clone();
            tokio::task::spawn_blocking(move || {
                helper::provide_tun_fd(&tun_fd_path, sslocal::TUN_ADDRESS)
            })
        };
        let config = sslocal::build_server_config(
            &profile,
            Some(snapshot.interface.clone()),
            bundled_plugin_dir.as_deref(),
            Some(&tun_fd_path),
        )?;
        let runtime =
            match tokio::time::timeout(Duration::from_secs(10), sslocal::start_local(config)).await
            {
                Ok(Ok(runtime)) => runtime,
                Ok(Err(err)) => {
                    let _ = tun_fd_task.await;
                    return Err(err);
                }
                Err(_) => {
                    let helper_result = tun_fd_task
                        .await
                        .map_err(|err| AppError::msg(format!("TUN helper task failed: {err}")))?;
                    return Err(helper_result.err().unwrap_or_else(|| {
                        AppError::msg("Timed out waiting for TUN file descriptor")
                    }));
                }
            };
        tun_fd_task
            .await
            .map_err(|err| AppError::msg(format!("TUN helper task failed: {err}")))??;
        let flow_stat = runtime.flow_stat.clone();

        let server_task = tokio::spawn(async move {
            if let Err(err) = runtime.server.run().await {
                eprintln!("sslocal stopped: {err}");
            }
        });

        let tun_name = match macos_route::wait_for_tun_name(Duration::from_secs(8)).await {
            Ok(name) => name,
            Err(err) => {
                server_task.abort();
                return Err(err);
            }
        };

        let routes = match macos_route::apply_global_routes(&snapshot, &tun_name, &server_ip) {
            Ok(routes) => routes,
            Err(err) => {
                server_task.abort();
                return Err(err);
            }
        };
        save_runtime_state(
            &self.data_dir,
            &RuntimeState {
                routes: routes.clone(),
            },
        )?;

        let poller_task = spawn_traffic_poller(
            self.app.clone(),
            profile.id.clone(),
            flow_stat,
            self.data_dir.clone(),
            self.traffic_totals.clone(),
        );
        let connectivity_task = spawn_connectivity_check(self.app.clone(), profile.id.clone());

        let mut session = self.session.lock().await;
        *session = Some(RuntimeSession {
            profile_id: profile.id,
            tun_name,
            routes: Some(routes),
            server_task,
            poller_task,
            connectivity_task,
        });
        drop(session);
        Ok(self.runtime_status().await)
    }

    pub async fn disconnect(&self) -> AppResult<RuntimeStatus> {
        let mut session = self.session.lock().await;
        if let Some(current) = session.take() {
            current.poller_task.abort();
            current.connectivity_task.abort();
            current.server_task.abort();
            if let Some(routes) = current.routes {
                macos_route::restore_routes(&routes)?;
            }
            remove_runtime_state(&self.data_dir)?;
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

fn spawn_traffic_poller(
    app: AppHandle,
    profile_id: String,
    flow_stat: Arc<FlowStat>,
    data_dir: PathBuf,
    traffic_totals: Arc<Mutex<HashMap<String, TrafficTotals>>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_tx = flow_stat.tx();
        let mut last_rx = flow_stat.rx();
        let (mut total_tx, mut total_rx) = {
            let total = traffic_totals
                .lock()
                .await
                .get(&profile_id)
                .cloned()
                .unwrap_or_else(|| load_traffic_totals(&data_dir, &profile_id).unwrap_or_default());
            (total.tx, total.rx)
        };
        let mut last_save = Instant::now();
        let mut ticker = tokio::time::interval(Duration::from_millis(500));
        loop {
            ticker.tick().await;
            let tx = flow_stat.tx();
            let rx = flow_stat.rx();
            let tx_delta = tx.saturating_sub(last_tx);
            let rx_delta = rx.saturating_sub(last_rx);
            let now = Instant::now();
            let up_bps = tx_delta.saturating_mul(2);
            let down_bps = rx_delta.saturating_mul(2);
            total_tx = total_tx.saturating_add(tx_delta);
            total_rx = total_rx.saturating_add(rx_delta);
            let total = TrafficTotals {
                tx: total_tx,
                rx: total_rx,
            };
            traffic_totals
                .lock()
                .await
                .insert(profile_id.clone(), total.clone());
            if now.duration_since(last_save) >= Duration::from_secs(10) {
                let _ = save_traffic_totals(&data_dir, &profile_id, &total);
                last_save = now;
            }
            let event = TrafficEvent {
                profile_id: profile_id.clone(),
                tx,
                rx,
                up_bps,
                down_bps,
                total_tx,
                total_rx,
            };
            last_tx = tx;
            last_rx = rx;
            let _ = app.emit("traffic", event);
        }
    })
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
        .get("https://dns.google")
        .send()
        .await
        .map_err(|err| AppError::msg(format!("Failed to reach https://dns.google: {err}")))?;
    if !response.status().is_success() {
        return Err(AppError::msg(format!(
            "https://dns.google returned HTTP {}",
            response.status()
        )));
    }
    Ok(())
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

fn runtime_state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime-state.json")
}

fn load_runtime_state(data_dir: &Path) -> AppResult<Option<RuntimeState>> {
    let path = runtime_state_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&raw)?))
}

fn save_runtime_state(data_dir: &Path, state: &RuntimeState) -> AppResult<()> {
    fs::create_dir_all(data_dir)?;
    let raw = serde_json::to_string_pretty(state)?;
    fs::write(runtime_state_path(data_dir), raw)?;
    Ok(())
}

fn remove_runtime_state(data_dir: &Path) -> AppResult<()> {
    let path = runtime_state_path(data_dir);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn recover_runtime_state(data_dir: &Path) -> AppResult<()> {
    let Some(state) = load_runtime_state(data_dir)? else {
        return Ok(());
    };
    macos_route::restore_routes(&state.routes)?;
    remove_runtime_state(data_dir)?;
    Ok(())
}
