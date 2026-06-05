use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Workflow {
    Collect,
    Exploit,
    Plan,
    Watchdog,
}

impl Workflow {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Collect => "COLLECT",
            Self::Exploit => "EXPLOIT",
            Self::Plan => "PLAN",
            Self::Watchdog => "WATCHDOG",
        }
    }

}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollectTab {
    Signals,
    P25,
    Dispatch,
    Field,
    Rec,
}

impl CollectTab {
    pub const ALL: &[Self] = &[Self::Signals, Self::P25, Self::Dispatch, Self::Field, Self::Rec];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Signals => "SIGNALS",
            Self::P25 => "P25",
            Self::Dispatch => "DISPATCH",
            Self::Field => "FIELD",
            Self::Rec => "REC",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExploitTab {
    Decode,
    Traffic,
    Fingerprint,
    Crypto,
    Net,
    Intel,
    Events,
}

impl ExploitTab {
    pub const ALL: &[Self] = &[Self::Decode, Self::Traffic, Self::Fingerprint, Self::Crypto, Self::Net, Self::Intel, Self::Events];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Decode => "DECODE",
            Self::Traffic => "TRAFFIC",
            Self::Fingerprint => "FINGERPRINT",
            Self::Crypto => "CRYPTO",
            Self::Net => "NET",
            Self::Intel => "INTEL",
            Self::Events => "EVENTS",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanTab {
    Ops,
    Sites,
    Targets,
    Data,
    Config,
}

impl PlanTab {
    pub const ALL: &[Self] = &[Self::Ops, Self::Sites, Self::Targets, Self::Data, Self::Config];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Ops => "OPS",
            Self::Sites => "SITES",
            Self::Targets => "TARGETS",
            Self::Data => "DATA",
            Self::Config => "CONFIG",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchdogTab {
    Dashboard,
    Threats,
    Events,
    Alerts,
    Rules,
    Sweep,
    Baseline,
}

impl WatchdogTab {
    pub const ALL: &[Self] = &[
        Self::Dashboard, Self::Threats, Self::Events,
        Self::Alerts, Self::Rules, Self::Sweep, Self::Baseline,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Dashboard => "DASHBOARD",
            Self::Threats => "THREATS",
            Self::Events => "EVENTS",
            Self::Alerts => "ALERTS",
            Self::Rules => "RULES",
            Self::Sweep => "SWEEP",
            Self::Baseline => "BASELINE",
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct UiState {
    // Navigation
    pub active_workflow: Workflow,
    pub collect_tab: CollectTab,
    pub exploit_tab: ExploitTab,
    pub plan_tab: PlanTab,
    pub watchdog_tab: WatchdogTab,

    // Radio controls (mirrored from backend for display)
    pub gain: f32,
    pub squelch: f32,
    pub volume: f32,
    pub muted: bool,

    // Alert volume (independent of monitor volume, persisted)
    pub alert_volume: f32,

    // Layout split ratios
    pub collect_split_v: f32,
    pub collect_split_h: f32,
    pub watchdog_split: f32,

    // Watchdog panel sizes (pixels, resizable by user)
    pub watchdog_live_tail_h: f32,
    pub watchdog_facet_w: f32,

    // P25 custom talkgroup groups: group_name → list of TGIDs
    #[serde(default)]
    pub tg_groups: BTreeMap<String, Vec<i32>>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            active_workflow: Workflow::Collect,
            collect_tab: CollectTab::Signals,
            exploit_tab: ExploitTab::Decode,
            plan_tab: PlanTab::Ops,
            watchdog_tab: WatchdogTab::Dashboard,
            gain: 40.0,
            squelch: -40.0,
            volume: 0.5,
            muted: false,
            alert_volume: 0.8,
            collect_split_v: 0.6,
            collect_split_h: 0.35,
            watchdog_split: 0.3,
            watchdog_live_tail_h: 160.0,
            watchdog_facet_w: 180.0,
            tg_groups: BTreeMap::new(),
        }
    }
}
