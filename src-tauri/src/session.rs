use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::error::{AppError, AppResult};
use crate::outline_config;
use crate::profiles::{self, Profile, ProfileInput};

const CONNECTIVITY_ATTEMPTS: usize = 3;
const CONNECTIVITY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub active_profile_id: Option<String>,
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
    connectivity_task: JoinHandle<()>,
}

pub struct AppState {
    data_dir: PathBuf,
    app: AppHandle,
    profiles: Mutex<Vec<Profile>>,
    traffic_totals: Mutex<HashMap<String, TrafficTotals>>,
    session: Mutex<Option<RuntimeSession>>,
}

impl AppState {
    pub fn load(data_dir: PathBuf, app: AppHandle) -> AppResult<Self> {
        let profiles = profiles::load_profiles(&data_dir)?;
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
            profiles: Mutex::new(profiles),
            traffic_totals: Mutex::new(traffic_totals),
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
        let created = profiles::create_profile(input)?;
        let mut profiles = self.profiles.lock().await;
        profiles.push(created.clone());
        profiles::save_profiles(&self.data_dir, &profiles)?;
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
        profiles::apply_update(profile, input)?;
        let updated = profile.clone();
        profiles::save_profiles(&self.data_dir, &profiles)?;
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
        profiles::save_profiles(&self.data_dir, &profiles)?;
        self.traffic_totals.lock().await.remove(id);
        remove_traffic_totals(&self.data_dir, id)?;
        Ok(())
    }

    pub async fn runtime_status(&self) -> RuntimeStatus {
        let session = self.session.lock().await;
        RuntimeStatus {
            active_profile_id: session.as_ref().map(|s| s.profile_id.clone()),
        }
    }

    pub async fn connect(&self, id: &str) -> AppResult<RuntimeStatus> {
        self.disconnect().await?;

        let profile = {
            let profiles = self.profiles.lock().await;
            profiles
                .iter()
                .find(|p| p.id == id)
                .cloned()
                .ok_or_else(|| AppError::msg("Profile not found"))?
        };

        let transport_config = outline_config::transport_config(&profile)?;
        start_packet_tunnel(&profile, &transport_config).await?;

        let connectivity_task = spawn_connectivity_check(self.app.clone(), profile.id.clone());
        let mut session = self.session.lock().await;
        *session = Some(RuntimeSession {
            profile_id: profile.id,
            connectivity_task,
        });
        drop(session);
        Ok(self.runtime_status().await)
    }

    pub async fn disconnect(&self) -> AppResult<RuntimeStatus> {
        let mut session = self.session.lock().await;
        if let Some(current) = session.take() {
            current.connectivity_task.abort();
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

async fn start_packet_tunnel(_profile: &Profile, _transport_config: &str) -> AppResult<()> {
    Err(AppError::msg(
        "Packet Tunnel backend is not wired yet. The legacy helper TUN/route/DNS path has been removed.",
    ))
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

        let result = check_dns_google_with_retries().await;
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

async fn check_dns_google_with_retries() -> AppResult<()> {
    let mut last_error = AppError::msg("Connectivity check did not run");
    for _ in 0..CONNECTIVITY_ATTEMPTS {
        match check_dns_google().await {
            Ok(()) => return Ok(()),
            Err(err) => last_error = err,
        }
    }
    Err(last_error)
}

async fn check_dns_google() -> AppResult<()> {
    let client = reqwest::Client::builder()
        .timeout(CONNECTIVITY_TIMEOUT)
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
