use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;

use eframe::egui;
use tokio::sync::broadcast;

use crate::bridge::UiBridge;
use crate::state::{UiState, Workflow};
use crate::theme::*;
use crate::views;
use crate::widgets;

pub struct RfLogApp {
    pub state: rf_web::AppState,
    pub bridge: UiBridge,
    pub ui_state: UiState,
    pub collect_state: views::collect::CollectState,
    pub exploit_state: views::exploit::ExploitState,
    pub plan_state: views::plan::PlanState,
    pub watchdog_state: views::watchdog::WatchdogState,
    /// EventBus subscription for SIEM live tail
    event_bus_rx: broadcast::Receiver<Arc<rf_events::LogRecord>>,
    /// Shared atomic for alert volume (cpal callback reads this)
    pub alert_volume_atomic: Arc<AtomicU32>,
    /// Shared atomic for monitor volume (cpal callback reads this)
    pub volume_atomic: Arc<AtomicU32>,
    /// Shared atomic for mute state (cpal callback reads this)
    pub muted_atomic: Arc<AtomicBool>,
    /// Recorder command channel — UI sends start/stop/feed commands
    pub rec_cmd_tx: mpsc::Sender<rf_recorder::RecorderCommand>,
    /// Playback channel — WAV samples sent to cpal callback
    pub playback_tx: mpsc::Sender<Vec<f32>>,
}

impl RfLogApp {
    pub fn new(
        state: rf_web::AppState,
        bridge: UiBridge,
        event_bus_rx: broadcast::Receiver<Arc<rf_events::LogRecord>>,
        alert_volume_atomic: Arc<AtomicU32>,
        volume_atomic: Arc<AtomicU32>,
        muted_atomic: Arc<AtomicBool>,
        rec_cmd_tx: mpsc::Sender<rf_recorder::RecorderCommand>,
        playback_tx: mpsc::Sender<Vec<f32>>,
        cc: &eframe::CreationContext<'_>,
    ) -> Self {
        // Restore persisted UI state
        let ui_state: UiState = cc
            .storage
            .and_then(|s| eframe::get_value(s, "ui_state"))
            .unwrap_or_default();

        // Sync the atomics with the persisted/config values
        alert_volume_atomic.store(ui_state.alert_volume.to_bits(), Ordering::Relaxed);
        volume_atomic.store(ui_state.volume.to_bits(), Ordering::Relaxed);
        muted_atomic.store(ui_state.muted, Ordering::Relaxed);

        Self {
            state,
            bridge,
            ui_state,
            collect_state: views::collect::CollectState::default(),
            exploit_state: views::exploit::ExploitState::default(),
            plan_state: views::plan::PlanState::default(),
            watchdog_state: views::watchdog::WatchdogState::default(),
            event_bus_rx,
            alert_volume_atomic,
            volume_atomic,
            muted_atomic,
            rec_cmd_tx,
            playback_tx,
        }
    }
}

impl eframe::App for RfLogApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "ui_state", &self.ui_state);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll backend channels
        self.bridge.poll();

        // Drain EventBus into watchdog live tail (capped to prevent UI stall)
        let frame_time = ctx.input(|i| i.time);
        let mut drain_count = 0u32;
        loop {
            if drain_count >= 20 {
                break; // Cap per-frame drain — at 4 Hz repaint, this is 80 events/sec max
            }
            match self.event_bus_rx.try_recv() {
                Ok(record) => {
                    self.watchdog_state.push_live_event((*record).clone(), frame_time);
                    drain_count += 1;
                }
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    continue; // Skip silently — burst traffic is normal
                }
                Err(_) => break, // Empty or Closed
            }
        }

        // Update live tail event rate (runs regardless of active workflow)
        self.watchdog_state.update_rate(frame_time);

        // Sync UI state from heartbeat (so sliders reflect backend state)
        // NOTE: volume/muted are NOT synced from heartbeat — UI is the source of truth
        // and writes directly to cpal atomics. Syncing from heartbeat causes snap-back.
        if self.bridge.heartbeat.is_some() {
            self.ui_state.squelch = self.bridge.hb_f64("squelch") as f32;
            self.ui_state.gain = self.bridge.hb_f64("gain") as f32;
        }

        // Handle keyboard shortcuts
        let mut vol_delta: i32 = 0;
        let mut tune_dir: i32 = 0;
        let mut tune_large = false;
        ctx.input(|input| {
            // Ctrl+1-4: workflow switching
            if input.modifiers.ctrl {
                if input.key_pressed(egui::Key::Num1) {
                    self.ui_state.active_workflow = Workflow::Collect;
                } else if input.key_pressed(egui::Key::Num2) {
                    self.ui_state.active_workflow = Workflow::Exploit;
                } else if input.key_pressed(egui::Key::Num3) {
                    self.ui_state.active_workflow = Workflow::Plan;
                } else if input.key_pressed(egui::Key::Num4) {
                    self.ui_state.active_workflow = Workflow::Watchdog;
                }

                // Ctrl+Left/Right: large tune step (1 MHz)
                if input.key_pressed(egui::Key::ArrowLeft) {
                    tune_dir = -1;
                    tune_large = true;
                } else if input.key_pressed(egui::Key::ArrowRight) {
                    tune_dir = 1;
                    tune_large = true;
                }
            } else {
                // Up/Down: volume
                if input.key_pressed(egui::Key::ArrowUp) {
                    vol_delta = 5;
                } else if input.key_pressed(egui::Key::ArrowDown) {
                    vol_delta = -5;
                }
                // Left/Right: fine tune (12.5 kHz channel step)
                if input.key_pressed(egui::Key::ArrowLeft) {
                    tune_dir = -1;
                } else if input.key_pressed(egui::Key::ArrowRight) {
                    tune_dir = 1;
                }
            }
        });

        // Apply volume change
        if vol_delta != 0 {
            let new_vol = ((self.ui_state.volume * 100.0) as i32 + vol_delta).clamp(0, 100);
            self.ui_state.volume = new_vol as f32 / 100.0;
            self.volume_atomic.store(self.ui_state.volume.to_bits(), Ordering::Relaxed);
            crate::commands::set_volume(
                &self.state,
                new_vol as u8,
                self.ui_state.muted,
            );
        }

        // Apply tuning change
        if tune_dir != 0 {
            let current_freq = self.bridge.hb_f64("monitor_freq");
            if current_freq > 0.0 {
                let new_freq = widgets::freq_digits::tune_step(current_freq, tune_dir, tune_large);
                crate::commands::monitor_signal(&self.state, new_freq);
            }
        }

        // --- StatusHUD (top) ---
        egui::TopBottomPanel::top("status_hud")
            .exact_height(STATUS_HUD_HEIGHT)
            .show(ctx, |ui| {
                widgets::status_hud::show(ui, &self.ui_state, &self.bridge, self.collect_state.active_wx_count, &self.watchdog_state);
            });

        // --- MonitorDock (bottom) — auto-height for who's-talking + controls ---
        egui::TopBottomPanel::bottom("monitor_dock")
            .show(ctx, |ui| {
                widgets::monitor_dock::show(ui, &mut self.ui_state, &self.bridge, &self.state);
            });

        // --- NavRail (left) ---
        egui::SidePanel::left("nav_rail")
            .exact_width(NAV_RAIL_WIDTH)
            .resizable(false)
            .show(ctx, |ui| {
                widgets::nav_rail::show(ui, &mut self.ui_state);
            });

        // --- Central view area ---
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.ui_state.active_workflow {
                Workflow::Collect => views::collect::show(
                    ui,
                    &mut self.ui_state,
                    &self.bridge,
                    &self.state,
                    &mut self.collect_state,
                    &self.rec_cmd_tx,
                    &self.playback_tx,
                ),
                Workflow::Exploit => {
                    views::exploit::show(
                        ui, &mut self.ui_state, &self.bridge,
                        &self.state, &mut self.exploit_state,
                    );
                }
                Workflow::Plan => {
                    views::plan::show(
                        ui, &mut self.ui_state, &self.bridge,
                        &self.state, &mut self.plan_state,
                    );
                }
                Workflow::Watchdog => {
                    views::watchdog::show(
                        ui,
                        &mut self.ui_state,
                        &self.bridge,
                        &self.state.db(),
                        &mut self.watchdog_state,
                    );
                }
            }
        });

        // Sync audio atomics from UI state (cpal callback reads these)
        self.alert_volume_atomic.store(
            self.ui_state.alert_volume.to_bits(),
            Ordering::Relaxed,
        );
        self.volume_atomic.store(
            self.ui_state.volume.to_bits(),
            Ordering::Relaxed,
        );
        self.muted_atomic.store(self.ui_state.muted, Ordering::Relaxed);

        // Repaint at a reasonable interval — not every frame.
        // 4 Hz is enough for live spectrum + status updates without burning CPU.
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}
