//! The `eframe::App` implementation: sidebar + Status / Clips / Settings
//! tabs (each in its own `*_view` module), plus the one-time "launch
//! everything" step that starts MediaMTX and the Dogitor server (unchanged
//! async tasks from `dogitor::server`) on a background thread while
//! `eframe` owns the main thread, as required on macOS.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use eframe::egui;
use egui_phosphor::fill as phosphor;

use dogitor::clips::ClipInfo;
use dogitor::config::Config;
use dogitor::server::ServerHandle;

use crate::mediamtx::MediaMtxProcess;
use crate::settings_form::SettingsForm;
use crate::settings_view::Mode;
use crate::{clips_view, persist, settings_view, status_view, theme, widgets};

/// Everything running in the background, owned by the GUI for its lifetime.
pub struct Launched {
    pub server: ServerHandle,
    pub mediamtx: MediaMtxProcess,
    pub clips: Arc<Mutex<Vec<ClipInfo>>>,
}

/// Spawn MediaMTX and the Dogitor server (on its own OS thread + tokio
/// runtime) and block briefly until the server reports it's up. Called once,
/// either at startup (config already complete) or right after the first-run
/// Settings form is saved (see `main.rs` — both paths write config then
/// re-exec, so this only ever runs at process start).
pub fn launch(cfg: Config, mediamtx_yml: &Path, log_file: &std::fs::File) -> Result<Launched> {
    let mediamtx = MediaMtxProcess::spawn(mediamtx_yml, log_file);

    let clips_cache = Arc::new(Mutex::new(Vec::new()));
    let clips_for_bg = clips_cache.clone();
    let output_dir = cfg.recording.output_dir.clone();

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("dogitor-server".into())
        .spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(anyhow!("building tokio runtime: {e}")));
                    return;
                }
            };
            rt.block_on(async move {
                match dogitor::server::start(cfg).await {
                    Ok(handle) => {
                        tokio::spawn(clips_refresh_loop(output_dir, clips_for_bg));
                        let _ = tx.send(Ok(handle));
                        std::future::pending::<()>().await;
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                    }
                }
            });
        })
        .map_err(|e| anyhow!("spawning server thread: {e}"))?;

    let server = rx
        .recv()
        .map_err(|_| anyhow!("server thread exited before starting"))??;

    Ok(Launched {
        server,
        mediamtx,
        clips: clips_cache,
    })
}

async fn clips_refresh_loop(dir: PathBuf, cache: Arc<Mutex<Vec<ClipInfo>>>) {
    loop {
        if let Ok(list) = dogitor::clips::list(&dir).await {
            *cache.lock().unwrap() = list;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Relaunch this same binary and exit the current process — how "Save &
/// Start"/"Save & Restart" apply settings that need a fresh capture pipeline
/// (camera device, resolution, ports, timings) without in-process hot-reload
/// plumbing for every field.
fn relaunch() -> ! {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
    std::process::exit(0);
}

#[derive(PartialEq, Clone, Copy)]
pub enum Tab {
    Status,
    Clips,
    Settings,
}

pub enum Phase {
    /// Required secrets (bot token / API token) are missing — show a
    /// settings-only onboarding screen instead of starting anything.
    Setup,
    Running(RunningState),
}

pub struct RunningState {
    pub launched: Launched,
    pub tab: Tab,
    pub output_dir: PathBuf,
    pub clip_pending_delete: Option<PathBuf>,
    /// A problem worth surfacing on the Status tab (e.g. a failed save).
    pub notice: Option<String>,
}

impl RunningState {
    pub fn new(launched: Launched, output_dir: PathBuf) -> Self {
        Self {
            launched,
            tab: Tab::Status,
            output_dir,
            clip_pending_delete: None,
            notice: None,
        }
    }
}

pub struct DogitorApp {
    phase: Phase,
    settings: SettingsForm,
    config_path: PathBuf,
}

impl DogitorApp {
    pub fn new(phase: Phase, settings: SettingsForm, config_path: PathBuf) -> Self {
        Self {
            phase,
            settings,
            config_path,
        }
    }

    fn render_setup(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_WINDOW)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ctx, |ui| {
                if settings_view::show(ui, &mut self.settings, Mode::Setup, None, &self.config_path)
                {
                    self.save_and_relaunch();
                }
            });
    }

    fn render_running(&mut self, ctx: &egui::Context) {
        let Phase::Running(state) = &mut self.phase else {
            return;
        };

        sidebar(ctx, state, self.settings.is_dirty());

        let mut save_clicked = false;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_WINDOW)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ctx, |ui| match state.tab {
                Tab::Status => status_view::show(ui, state, &mut self.settings, &self.config_path),
                Tab::Clips => clips_view::show(ui, state),
                Tab::Settings => {
                    let live = state.launched.server.state.clone();
                    save_clicked = settings_view::show(
                        ui,
                        &mut self.settings,
                        Mode::Running,
                        Some(&live),
                        &self.config_path,
                    );
                }
            });

        if save_clicked {
            self.save_and_relaunch();
        }
    }

    /// The bot learns the chat ID at runtime (someone sends `/start`); adopt
    /// it into the form and persist it right away, so it survives restarts
    /// and a later "Save & Restart" doesn't write a blank one back.
    fn sync_chat_id(&mut self) {
        let Phase::Running(state) = &mut self.phase else {
            return;
        };
        let live_id = state.launched.server.state.chat_id();
        if live_id == 0 || !self.settings.sync_chat_id(live_id) {
            return;
        }
        if let Err(e) = persist::patch_config(&self.config_path, |cfg| {
            cfg.telegram.chat_id = Some(live_id);
        }) {
            state.notice = Some(format!("Couldn't save the Telegram chat ID: {e}"));
        }
    }

    fn save_and_relaunch(&mut self) {
        let cfg = match self.settings.build_config() {
            Ok(cfg) => cfg,
            Err(e) => {
                self.settings.error = Some(e);
                return;
            }
        };
        if let Err(e) = persist::write_config(&self.config_path, &cfg) {
            self.settings.error = Some(format!("Couldn't save settings: {e}"));
            return;
        }
        // `std::process::exit` below skips Drop entirely, so the old
        // MediaMTX child would otherwise leak and hold the RTSP/WebRTC ports
        // the relaunched process needs.
        if let Phase::Running(state) = &mut self.phase {
            state.launched.mediamtx.kill();
        }
        relaunch();
    }
}

fn sidebar(ctx: &egui::Context, state: &mut RunningState, settings_dirty: bool) {
    egui::SidePanel::left("nav")
        .exact_width(200.0)
        .resizable(false)
        .frame(
            egui::Frame::NONE
                .fill(theme::BG_PANEL)
                .inner_margin(egui::Margin::symmetric(12, 20)),
        )
        .show(ctx, |ui| {
            widgets::inset(ui, widgets::NAV_INSET, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        egui::RichText::new(phosphor::PAW_PRINT)
                            .size(20.0)
                            .color(theme::ACCENT),
                    );
                    ui.label(egui::RichText::new("Dogitor").size(17.0).color(theme::TEXT));
                });
            });
            ui.add_space(14.0);

            ui.spacing_mut().item_spacing.y = 2.0;
            let tabs = [
                (Tab::Status, phosphor::GAUGE, "Status", false),
                (Tab::Clips, phosphor::FILM_STRIP, "Clips", false),
                (
                    Tab::Settings,
                    phosphor::GEAR_SIX,
                    "Settings",
                    settings_dirty,
                ),
            ];
            for (tab, icon, label, dot) in tabs {
                if widgets::nav_item(ui, icon, label, state.tab == tab, dot) {
                    state.tab = tab;
                }
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                let sstate = &state.launched.server.state;
                // Bottom-up: the first pill added sits lowest.
                widgets::inset(ui, widgets::NAV_INSET, |ui| {
                    let armed = sstate.armed();
                    widgets::pill(
                        ui,
                        if armed { "Armed" } else { "Disarmed" },
                        if armed {
                            theme::ACCENT
                        } else {
                            theme::TEXT_MUTED
                        },
                    );
                    if sstate.recording() {
                        widgets::pill(
                            ui,
                            &format!("{} Recording", phosphor::RECORD),
                            theme::DANGER,
                        );
                    }
                });
            });
        });
}

impl eframe::App for DogitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.settings.poll();

        // The level meter needs a smooth refresh; elsewhere only slow-moving
        // state (recording pill, clip list) changes, so idle at ~1 fps.
        let fast = match &self.phase {
            Phase::Running(s) => s.tab == Tab::Status || self.settings.listing_cameras(),
            Phase::Setup => self.settings.listing_cameras(),
        };
        ctx.request_repaint_after(Duration::from_millis(if fast { 100 } else { 1000 }));

        match self.phase {
            Phase::Setup => self.render_setup(ctx),
            Phase::Running(_) => {
                self.sync_chat_id();
                self.render_running(ctx);
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Phase::Running(state) = &mut self.phase {
            state.launched.mediamtx.kill();
        }
    }
}

#[cfg(test)]
impl DogitorApp {
    /// Tests can't go through `launch` (it starts real capture, MediaMTX and
    /// network listeners), so they build the running UI around a fake.
    pub fn for_preview(launched: Launched, settings: SettingsForm, config_path: PathBuf) -> Self {
        let output_dir = settings.values.output_dir.clone();
        Self::new(
            Phase::Running(RunningState::new(launched, output_dir)),
            settings,
            config_path,
        )
    }

    pub fn settings_mut(&mut self) -> &mut SettingsForm {
        &mut self.settings
    }

    pub fn set_tab(&mut self, tab: Tab) {
        if let Phase::Running(state) = &mut self.phase {
            state.tab = tab;
        }
    }
}
