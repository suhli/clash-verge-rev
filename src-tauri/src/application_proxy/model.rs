use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    #[default]
    Proxy,
    Direct,
    Block,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Both,
    Tcp,
    Udp,
}

const fn default_true() -> bool {
    true
}

fn default_wildcard() -> String {
    "*".into()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: u32,
    pub name: String,
    pub pattern: String,
    #[serde(default = "default_wildcard")]
    pub target_hosts: String,
    #[serde(default = "default_wildcard")]
    pub target_ports: String,
    #[serde(default = "default_wildcard")]
    pub target_domains: String,
    #[serde(default)]
    pub protocol: Protocol,
    pub icon: Option<String>,
    pub action: Action,
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub overlay: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Anchor {
    #[default]
    OutsideTopLeft,
    OutsideTopCenter,
    OutsideTopRight,
    OutsideBottomLeft,
    OutsideBottomCenter,
    OutsideBottomRight,
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Delay {
    #[default]
    Node,
    Connection,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Overlay {
    pub global: bool,
    pub windows: bool,
    pub refresh_ms: u64,
    pub font_size: u16,
    pub foreground: u32,
    pub background: u32,
    pub opacity: u8,
    pub scale_percent: u16,
    pub anchor: Anchor,
    pub offset_x: i32,
    pub offset_y: i32,
    pub global_x: i32,
    pub global_y: i32,
    pub delay: Delay,
    pub hide_idle: bool,
    pub click_through: bool,
    pub locked: bool,
    pub show_rates: bool,
    pub show_totals: bool,
    pub show_delay: bool,
}

impl Default for Overlay {
    fn default() -> Self {
        Self {
            global: false,
            windows: false,
            refresh_ms: 500,
            font_size: 12,
            foreground: 0xf4f4f5,
            background: 0x18181b,
            opacity: 224,
            scale_percent: 100,
            anchor: Anchor::OutsideTopLeft,
            offset_x: 0,
            offset_y: 4,
            global_x: 32,
            global_y: 64,
            delay: Delay::Node,
            hide_idle: false,
            click_through: true,
            locked: true,
            show_rates: true,
            show_totals: true,
            show_delay: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub enabled: bool,
    pub fail_open: bool,
    #[serde(default = "default_true")]
    pub send_domain_to_proxy: bool,
    pub rules: Vec<Rule>,
    pub overlay: Overlay,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            fail_open: false,
            send_domain_to_proxy: true,
            rules: Vec::new(),
            overlay: Overlay::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub sent: u64,
    pub received: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Stored {
    pub settings: Settings,
    pub last_id: u32,
    pub totals: BTreeMap<u32, Totals>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleStats {
    pub upload_rate: u64,
    pub download_rate: u64,
    pub upload_total: u64,
    pub download_total: u64,
    pub session_upload: u64,
    pub session_download: u64,
    pub active: u64,
    pub failures: u64,
    pub connection_delay: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub running: bool,
    pub tun_conflict: bool,
    pub system_proxy: bool,
    pub endpoint_ready: bool,
    pub error: Option<String>,
    pub node: Option<String>,
    pub node_delay: Option<u16>,
    pub rules: BTreeMap<u32, RuleStats>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub settings: Settings,
    pub stats: Snapshot,
}
