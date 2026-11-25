mod engine;
mod model;
mod osd;
mod process;
mod transport;

use crate::{
    config::Config,
    core::handle::Handle,
    utils::{dirs, help},
};
use anyhow::{Context as _, Result, ensure};
use engine::{Counters, Endpoint, Plan};
use model::{Action, RuleStats, Settings, Snapshot, Stored, View};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpStream,
    sync::Mutex,
    time::{sleep, timeout},
};
use transport::Helper;

pub use transport::run_helper;

static STATE: Mutex<Option<Controller>> = Mutex::const_new(None);
static STARTED: OnceLock<()> = OnceLock::new();

#[derive(Default)]
struct Controller {
    stored: Stored,
    snapshot: Snapshot,
    helper: Option<Helper>,
    applied: Option<Plan>,
    probe: Option<(Option<Endpoint>, Instant, bool)>,
    previous: BTreeMap<u32, Counters>,
    sampled_at: Option<Instant>,
    saved_at: Option<Instant>,
    node_checked_at: Option<Instant>,
    system_proxy_checked_at: Option<Instant>,
    attempted: bool,
    dirty: bool,
}

fn settings_path() -> Result<PathBuf> {
    Ok(dirs::app_home_dir()?.join("application-proxy.json"))
}

async fn load(state: &mut Option<Controller>) -> Result<&mut Controller> {
    if state.is_none() {
        let stored: Stored = match tokio::fs::read(settings_path()?).await {
            Ok(bytes) => serde_json::from_slice(&bytes).context("Reading application proxy configuration")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Stored::default(),
            Err(error) => return Err(error).context("Reading application proxy configuration"),
        };
        let rules = stored
            .totals
            .iter()
            .map(|(&id, totals)| {
                (
                    id,
                    RuleStats {
                        upload_total: totals.sent,
                        download_total: totals.received,
                        ..RuleStats::default()
                    },
                )
            })
            .collect();
        *state = Some(Controller {
            stored,
            snapshot: Snapshot {
                rules,
                ..Snapshot::default()
            },
            ..Controller::default()
        });
    }
    state.as_mut().context("Application proxy configuration is unavailable")
}

async fn save(stored: &Stored) -> Result<()> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    help::save_yaml_str(&path, &serde_json::to_string_pretty(stored)?).await
}

async fn tun_active() -> bool {
    Config::verge().await.data_arc().enable_tun_mode.unwrap_or(false)
        || Config::runtime()
            .await
            .data_arc()
            .config
            .as_ref()
            .and_then(|config| config.get("tun"))
            .and_then(|tun| tun.get("enable"))
            .and_then(serde_yaml_ng::Value::as_bool)
            .unwrap_or(false)
}

pub(crate) async fn tun_conflict() -> bool {
    STATE
        .lock()
        .await
        .as_ref()
        .is_some_and(|state| state.stored.settings.enabled)
}

async fn endpoint_and_mode() -> (Option<Endpoint>, bool) {
    let runtime = Config::runtime().await.data_arc();
    let config = runtime.config.as_ref();
    let global = config
        .and_then(|config| config.get("mode"))
        .and_then(serde_yaml_ng::Value::as_str)
        .is_some_and(|mode| mode.eq_ignore_ascii_case("global"));
    let Some(config) = config else { return (None, global) };
    let port = ["socks-port", "mixed-port"]
        .iter()
        .filter_map(|key| config.get(*key)?.as_u64().and_then(|port| u16::try_from(port).ok()))
        .find(|port| *port != 0);
    let endpoint = port.map(|port| {
        let credentials = config
            .get("authentication")
            .and_then(serde_yaml_ng::Value::as_sequence)
            .and_then(|entries| {
                entries
                    .iter()
                    .filter_map(serde_yaml_ng::Value::as_str)
                    .find_map(|entry| {
                        let (user, password) = entry.split_once(':')?;
                        (user.len() <= 255 && password.len() <= 255 && !user.is_empty()).then_some((user, password))
                    })
            });
        let (user, password) = credentials.unwrap_or_default();
        Endpoint {
            port,
            user: user.into(),
            password: password.into(),
        }
    });
    (endpoint, global)
}

async fn socks_ready(endpoint: &Endpoint) -> Result<bool> {
    let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port)).await?;
    let greeting: &[u8] = if endpoint.user.is_empty() {
        &[5, 1, 0]
    } else {
        &[5, 2, 0, 2]
    };
    stream.write_all(greeting).await?;
    let mut answer = [0; 2];
    stream.read_exact(&mut answer).await?;
    if answer == [5, 0] {
        return Ok(true);
    }
    if answer != [5, 2] || endpoint.user.is_empty() {
        return Ok(false);
    }
    let mut login = vec![1, endpoint.user.len() as u8];
    login.extend(endpoint.user.as_bytes());
    login.push(endpoint.password.len() as u8);
    login.extend(endpoint.password.as_bytes());
    stream.write_all(&login).await?;
    stream.read_exact(&mut answer).await?;
    Ok(answer == [1, 0])
}

fn rate(bytes: u64, elapsed: Duration) -> u64 {
    let millis = elapsed.as_millis();
    if millis == 0 {
        0
    } else {
        u64::try_from(u128::from(bytes) * 1000 / millis).unwrap_or(u64::MAX)
    }
}

impl Controller {
    fn view(&self) -> View {
        View {
            settings: self.stored.settings.clone(),
            stats: self.snapshot.clone(),
        }
    }

    async fn stop(&mut self) {
        if let Some(helper) = self.helper.take() {
            helper.stop().await;
        }
        self.applied = None;
        self.previous.clear();
        self.sampled_at = None;
        self.snapshot.running = false;
        self.snapshot.endpoint_ready = false;
        self.snapshot.node = None;
        self.snapshot.node_delay = None;
        for stat in self.snapshot.rules.values_mut() {
            stat.upload_rate = 0;
            stat.download_rate = 0;
            stat.active = 0;
        }
    }

    async fn sample(&mut self) -> Result<()> {
        let Some(helper) = &mut self.helper else { return Ok(()) };
        let counts = helper.sample().await?;
        let instant = Instant::now();
        let elapsed = self
            .sampled_at
            .replace(instant)
            .map(|before| instant.duration_since(before));
        for (id, count) in counts {
            let prev = self.previous.insert(id, count).unwrap_or_default();
            let sent = count.sent.saturating_sub(prev.sent);
            let received = count.received.saturating_sub(prev.received);
            let totals = self.stored.totals.entry(id).or_default();
            let stat: &mut RuleStats = self.snapshot.rules.entry(id).or_default();
            totals.sent = totals.sent.saturating_add(sent);
            totals.received = totals.received.saturating_add(received);
            stat.upload_total = totals.sent;
            stat.download_total = totals.received;
            stat.session_upload = stat.session_upload.saturating_add(sent);
            stat.session_download = stat.session_download.saturating_add(received);
            stat.upload_rate = elapsed.map_or(0, |period| rate(sent, period));
            stat.download_rate = elapsed.map_or(0, |period| rate(received, period));
            stat.active = count.active;
            stat.failures = stat.failures.saturating_add(count.failed.saturating_sub(prev.failed));
            let established = count.established.saturating_sub(prev.established);
            if established != 0 {
                let mean = count.connect_ms.saturating_sub(prev.connect_ms) / established;
                stat.connection_delay = Some(stat.connection_delay.map_or(mean, |old| (old * 3 + mean) / 4));
            }
            self.dirty |= sent != 0 || received != 0;
        }
        Ok(())
    }

    async fn update_node(&mut self, global: bool) {
        if !global {
            self.snapshot.node = None;
            self.snapshot.node_delay = None;
            return;
        }
        if self
            .node_checked_at
            .is_some_and(|instant| instant.elapsed() < Duration::from_secs(10))
        {
            return;
        }
        self.node_checked_at = Some(Instant::now());
        self.snapshot.node = None;
        self.snapshot.node_delay = None;
        let Ok(Ok(data)) = timeout(Duration::from_secs(1), Handle::mihomo().get_proxies()).await else {
            return;
        };
        let mut name = "GLOBAL";
        for _ in 0..16 {
            let Some(proxy) = data.proxies.get(name) else { break };
            if matches!(proxy.proxy_type, tauri_plugin_mihomo::models::ProxyType::LoadBalance) {
                break;
            }
            if let Some(selected) = proxy.now.as_deref().filter(|value| !value.is_empty() && *value != name) {
                name = selected;
            } else if proxy.all.is_none() && name != "GLOBAL" {
                self.snapshot.node = Some(name.to_owned());
                self.snapshot.node_delay = proxy
                    .history
                    .last()
                    .map(|entry| entry.delay)
                    .filter(|delay| *delay != 0);
                break;
            } else {
                break;
            }
        }
    }

    async fn tick(&mut self) -> Result<()> {
        self.snapshot.tun_conflict = tun_active().await;
        if self
            .system_proxy_checked_at
            .is_none_or(|time| time.elapsed() >= Duration::from_secs(3))
        {
            self.system_proxy_checked_at = Some(Instant::now());
            self.snapshot.system_proxy = Config::verge().await.data_arc().enable_system_proxy.unwrap_or(false)
                || sysproxy::Sysproxy::get_system_proxy().is_ok_and(|proxy| proxy.enable)
                || sysproxy::Autoproxy::get_auto_proxy().is_ok_and(|proxy| proxy.enable);
        }
        let has_rules = self
            .stored
            .settings
            .rules
            .iter()
            .any(|rule| rule.enabled && rule.action != Action::Direct);
        if !self.stored.settings.enabled || !has_rules || self.snapshot.tun_conflict {
            if self.helper.is_some() {
                let _ = self.sample().await;
            }
            self.stop().await;
            self.attempted = false;
            self.snapshot.error = None;
            return Ok(());
        }
        if self.helper.is_none() && !self.attempted {
            self.attempted = true;
            self.helper = Some(Helper::start().await?);
        }
        let Some(helper) = &mut self.helper else { return Ok(()) };
        let (endpoint, global) = endpoint_and_mode().await;
        if self
            .probe
            .as_ref()
            .is_none_or(|(last, time, _)| last != &endpoint || time.elapsed() >= Duration::from_secs(3))
        {
            let ready = if let Some(endpoint) = &endpoint {
                matches!(
                    timeout(Duration::from_millis(450), socks_ready(endpoint)).await,
                    Ok(Ok(true))
                )
            } else {
                false
            };
            self.probe = Some((endpoint.clone(), Instant::now(), ready));
        }
        let ready = self.probe.as_ref().is_some_and(|(_, _, ready)| *ready);
        let plan = Plan {
            endpoint,
            rules: self
                .stored
                .settings
                .rules
                .iter()
                .map(engine::EngineRule::from)
                .collect(),
            fail_open: self.stored.settings.fail_open,
            ready,
        };
        if self.applied.as_ref() != Some(&plan) {
            helper.apply(plan.clone()).await?;
            self.applied = Some(plan);
        }
        self.sample().await?;
        self.snapshot.running = true;
        self.snapshot.endpoint_ready = ready;
        self.snapshot.error = None;
        self.update_node(global && ready).await;
        Ok(())
    }

    async fn persist_totals(&mut self, force: bool) -> Result<()> {
        if self.dirty
            && (force
                || self
                    .saved_at
                    .is_none_or(|instant| instant.elapsed() >= Duration::from_secs(15)))
        {
            save(&self.stored).await?;
            self.saved_at = Some(Instant::now());
            self.dirty = false;
        }
        Ok(())
    }
}

fn validate(settings: &mut Settings, last_id: &mut u32) -> Result<()> {
    ensure!(
        settings.rules.len() <= 256,
        "A maximum of 256 application rules is supported"
    );
    let mut ids = BTreeSet::new();
    for rule in &mut settings.rules {
        if rule.id == 0 {
            *last_id = last_id.checked_add(1).context("Application rule ID exhausted")?;
            rule.id = *last_id;
        }
        ensure!(
            rule.id <= 65535 && ids.insert(rule.id),
            "Invalid or duplicate application rule ID"
        );
        *last_id = (*last_id).max(rule.id);
        rule.pattern = rule.pattern.trim().replace('/', "\\");
        ensure!(
            !rule.pattern.is_empty() && rule.pattern.len() < 24576 && !rule.pattern.contains('\0'),
            "Invalid executable match"
        );
        ensure!(
            !rule.name.trim().is_empty() && rule.name.len() <= 256,
            "Invalid application display name"
        );
    }
    let overlay = &settings.overlay;
    ensure!(
        [250, 500, 1000, 2000].contains(&overlay.refresh_ms),
        "Invalid rate refresh interval"
    );
    ensure!(
        (8..=48).contains(&overlay.font_size) && (50..=300).contains(&overlay.scale_percent),
        "Invalid overlay font size or scale"
    );
    Ok(())
}

pub async fn view() -> Result<Value> {
    let mut state = STATE.lock().await;
    let state = load(&mut state).await?;
    let mut view = state.view();
    for (&id, totals) in &state.stored.totals {
        let stats = view.stats.rules.entry(id).or_default();
        stats.upload_total = totals.sent;
        stats.download_total = totals.received;
    }
    Ok(serde_json::to_value(view)?)
}

pub async fn save_settings(settings: Value) -> Result<Value> {
    let mut settings: Settings = serde_json::from_value(settings)?;
    {
        let _configuration = Config::lock_config_write().await;
        ensure!(
            !settings.enabled || !tun_active().await,
            "Turn off TUN before enabling application proxy"
        );
        let mut state = STATE.lock().await;
        let state = load(&mut state).await?;
        let mut stored = state.stored.clone();
        validate(&mut settings, &mut stored.last_id)?;
        stored.settings = settings;
        stored
            .totals
            .retain(|id, _| stored.settings.rules.iter().any(|rule| rule.id == *id));
        save(&stored).await?;
        state
            .snapshot
            .rules
            .retain(|id, _| stored.settings.rules.iter().any(|rule| rule.id == *id));
        if state.helper.is_none() {
            state.attempted = false;
        }
        state.stored = stored;
        osd::publish(&state.stored.settings, &state.snapshot);
    }
    start_monitor();
    let _ = crate::core::tray::Tray::global().update_menu().await;
    view().await
}

pub(crate) async fn tray_switches() -> (bool, bool, bool) {
    let mut guard = STATE.lock().await;
    load(&mut guard).await.map_or((false, false, false), |state| {
        let settings = &state.stored.settings;
        (settings.enabled, settings.overlay.global, settings.overlay.windows)
    })
}

pub(crate) enum TraySwitch {
    Proxy,
    GlobalOverlay,
    WindowOverlay,
}

pub(crate) async fn toggle_tray(switch: TraySwitch) -> Result<()> {
    let _configuration = Config::lock_config_write().await;
    let mut guard = STATE.lock().await;
    let state = load(&mut guard).await?;
    let mut stored = state.stored.clone();
    match switch {
        TraySwitch::Proxy => {
            let enabled = !stored.settings.enabled;
            ensure!(
                !enabled || !tun_active().await,
                "Turn off TUN before enabling application proxy"
            );
            stored.settings.enabled = enabled;
            if state.helper.is_none() {
                state.attempted = false;
            }
        }
        TraySwitch::GlobalOverlay => stored.settings.overlay.global = !stored.settings.overlay.global,
        TraySwitch::WindowOverlay => stored.settings.overlay.windows = !stored.settings.overlay.windows,
    }
    save(&stored).await?;
    state.stored = stored;
    osd::publish(&state.stored.settings, &state.snapshot);
    start_monitor();
    Ok(())
}

pub async fn reset_totals() -> Result<Value> {
    {
        let mut state = STATE.lock().await;
        let state = load(&mut state).await?;
        let _ = state.sample().await;
        let mut stored = state.stored.clone();
        stored.totals.clear();
        save(&stored).await?;
        state.stored = stored;
        for stat in state.snapshot.rules.values_mut() {
            stat.upload_total = 0;
            stat.download_total = 0;
            stat.session_upload = 0;
            stat.session_download = 0;
        }
    }
    view().await
}

pub async fn processes() -> Result<Value> {
    Ok(serde_json::to_value(
        tokio::task::spawn_blocking(process::running).await??,
    )?)
}

pub(crate) async fn save_overlay_position(x: i32, y: i32) {
    let mut guard = STATE.lock().await;
    if let Ok(state) = load(&mut guard).await {
        let mut stored = state.stored.clone();
        stored.settings.overlay.global_x = x;
        stored.settings.overlay.global_y = y;
        if save(&stored).await.is_ok() {
            state.stored = stored;
        }
    }
}

pub(crate) async fn shutdown() {
    let mut guard = STATE.lock().await;
    if let Some(state) = guard.as_mut() {
        let _ = state.sample().await;
        state.stop().await;
        let _ = state.persist_totals(true).await;
    }
}

pub fn start_monitor() {
    STARTED.get_or_init(|| {
        tauri::async_runtime::spawn(async {
            while !Handle::global().is_exiting() {
                let interval = {
                    let mut guard = STATE.lock().await;
                    if let Ok(state) = load(&mut guard).await {
                        if let Err(error) = state.tick().await {
                            state.snapshot.error = Some(format!("{error:#}"));
                            state.stop().await;
                        }
                        if let Err(error) = state.persist_totals(false).await {
                            state.snapshot.error = Some(format!("{error:#}"));
                        }
                        osd::publish(&state.stored.settings, &state.snapshot);
                        if state.stored.settings.enabled {
                            state.stored.settings.overlay.refresh_ms
                        } else {
                            1000
                        }
                    } else {
                        2000
                    }
                };
                sleep(Duration::from_millis(interval)).await;
            }
        });
    });
}
