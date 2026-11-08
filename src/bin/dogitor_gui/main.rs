//! Dogitor.app — the macOS GUI entry point. Wraps the same background tasks
//! `src/main.rs` (the headless CLI) runs, but resolves config/paths for a
//! Finder-launched bundle (fixed Application Support location, no reliance on
//! cwd or an inherited shell `PATH`) and adds a native control surface.

mod app;
mod clips_view;
mod mediamtx;
mod paths;
mod persist;
#[cfg(test)]
mod preview;
mod settings_form;
mod settings_view;
mod status_view;
mod theme;
mod widgets;

use std::fs::File;
use std::path::Path;

use anyhow::Result;
use eframe::egui;
use tracing_subscriber::EnvFilter;

use dogitor::config::Config;

use app::{DogitorApp, Phase, RunningState};
use settings_form::SettingsForm;

fn main() -> Result<()> {
    fix_up_path();

    let support_dir = paths::app_support_dir();
    std::fs::create_dir_all(&support_dir)?;

    init_logging(&paths::log_path())?;

    let env_path = paths::env_path();
    if env_path.exists() {
        dotenvy::from_path(&env_path).ok();
    }

    let config_path = paths::config_path();
    if !config_path.exists() {
        write_default_config(&config_path, &support_dir.join("clips"))?;
    }

    let cfg = Config::load(config_path.to_str().expect("config path is valid utf-8"))?;

    let mediamtx_yml = paths::mediamtx_yml_path();
    mediamtx::sync_config(&mediamtx_yml, cfg.api.port)?;
    let settings = SettingsForm::from_config(&cfg);

    let phase = if cfg.token().is_ok() && cfg.api_token().is_ok() {
        let mediamtx_log = open_log_file(&paths::log_path())?;
        let launched = app::launch(cfg.clone(), &mediamtx_yml, &mediamtx_log)?;
        Phase::Running(RunningState::new(
            launched,
            cfg.recording.output_dir.clone(),
        ))
    } else {
        Phase::Setup
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 700.0])
            .with_min_inner_size([780.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Dogitor",
        native_options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(DogitorApp::new(phase, settings, config_path)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;

    Ok(())
}

/// Finder launches GUI apps with a minimal `PATH` that typically excludes
/// Homebrew's install location, so `ffmpeg`/`ffprobe`/`mediamtx` (spawned as
/// child processes throughout `dogitor`) would otherwise fail to resolve even
/// though they work fine from a Terminal-launched `cargo run`.
fn fix_up_path() {
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<String> = current
        .split(':')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if Path::new(dir).is_dir() && !parts.iter().any(|p| p == dir) {
            parts.push(dir.to_string());
        }
    }
    std::env::set_var("PATH", parts.join(":"));
}

fn open_log_file(path: &Path) -> Result<File> {
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?)
}

fn init_logging(path: &Path) -> Result<()> {
    let file = open_log_file(path)?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,dogitor=info")),
        )
        .with_writer(move || file.try_clone().expect("clone log file handle"))
        .init();
    Ok(())
}

fn write_default_config(path: &Path, output_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;
    let mut cfg = Config::default();
    cfg.recording.output_dir = output_dir.to_path_buf();
    let toml_str = toml::to_string_pretty(&cfg)?;
    std::fs::write(path, toml_str)?;
    Ok(())
}
