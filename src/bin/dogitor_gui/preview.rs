//! Offscreen screenshots of every screen, for checking layout without
//! launching the real app (which would start capture, MediaMTX and network
//! listeners). Renders with wgpu via `egui_kittest`; ignored by default.
//!
//! ```sh
//! DOGITOR_PREVIEW_DIR=/tmp/dogitor-preview \
//!   cargo test --bin dogitor_gui preview -- --ignored
//! ```

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{Duration, Local};
use eframe::egui;
use egui_kittest::Harness;

use dogitor::clips::ClipInfo;
use dogitor::config::Config;
use dogitor::server::ServerHandle;
use dogitor::state::SharedState;

use crate::app::{DogitorApp, Launched, Phase, Tab};
use crate::mediamtx::MediaMtxProcess;
use crate::settings_form::SettingsForm;
use crate::theme;

fn out_dir() -> PathBuf {
    let dir = std::env::var("DOGITOR_PREVIEW_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("dogitor-preview"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(dir: &Path) -> (Config, PathBuf) {
    let mut cfg = Config::default();
    cfg.telegram.token = Some("123456:preview-token".into());
    cfg.telegram.chat_id = Some(6_235_825_964);
    cfg.api.token = Some("0123456789abcdef0123456789abcdef0123456789abcdef".into());
    cfg.recording.output_dir = dirs_home().join("Library/Application Support/Dogitor/clips");
    let path = dir.join("config.toml");
    crate::persist::write_config(&path, &cfg).unwrap();
    (cfg, path)
}

fn dirs_home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn fake_launched(cfg: &Config, clips: usize, recording: bool) -> Launched {
    let state = Arc::new(SharedState::new(true, 0, cfg.audio.trigger_db));
    state.set_current_db(-41.3);
    state.set_recording(recording);
    state.set_last_trigger(Local::now() - Duration::minutes(7));

    let clip_list = (0..clips)
        .map(|i| {
            let started = Local::now() - Duration::hours(i as i64 * 3);
            let name = format!("clip_{}.mp4", started.format("%Y%m%d_%H%M%S"));
            ClipInfo {
                path: PathBuf::from(format!("/nonexistent/{name}")),
                name,
                started_at: started,
                duration_s: 12.0 + i as f32 * 4.0,
                size_bytes: 3_400_000 + i as u64 * 900_000,
                thumb_path: None,
            }
        })
        .collect();

    Launched {
        server: ServerHandle {
            state,
            rec_tx: tokio::sync::mpsc::unbounded_channel().0,
            events_tx: tokio::sync::broadcast::channel(4).0,
            cfg: cfg.clone(),
        },
        mediamtx: MediaMtxProcess::none(),
        clips: Arc::new(Mutex::new(clip_list)),
    }
}

fn harness(size: egui::Vec2, app: DogitorApp) -> Harness<'static, DogitorApp> {
    let mut app = Some(app);
    Harness::builder()
        .with_size(size)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_eframe(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            app.take().unwrap()
        })
}

fn save(h: &mut Harness<'_, DogitorApp>, name: &str) {
    h.run_steps(8);
    let img = h.render().expect("render");
    let path = out_dir().join(format!("{name}.png"));
    img.save(&path).unwrap();
    eprintln!("wrote {}", path.display());
}

fn running_app(dir: &Path, clips: usize, recording: bool) -> DogitorApp {
    let (cfg, path) = config(dir);
    DogitorApp::for_preview(
        fake_launched(&cfg, clips, recording),
        SettingsForm::from_config(&cfg),
        path,
    )
}

#[test]
#[ignore = "writes screenshots; run explicitly"]
fn preview() {
    let dir = out_dir();
    let normal = egui::vec2(980.0, 700.0);
    let min = egui::vec2(780.0, 560.0);
    let tall = egui::vec2(980.0, 1500.0);

    let mut h = harness(normal, running_app(&dir, 7, true));
    save(&mut h, "status");

    h.state_mut().set_tab(Tab::Clips);
    save(&mut h, "clips");

    let mut h = harness(normal, running_app(&dir, 0, false));
    h.state_mut().set_tab(Tab::Clips);
    save(&mut h, "clips_empty");

    let mut h = harness(min, running_app(&dir, 3, false));
    save(&mut h, "status_min");
    h.state_mut().set_tab(Tab::Clips);
    save(&mut h, "clips_min");

    let mut h = harness(tall, running_app(&dir, 0, false));
    h.state_mut().set_tab(Tab::Settings);
    h.state_mut().settings_mut().show_advanced = true;
    save(&mut h, "settings_full");

    // Dirty form: edit a field so the save bar shows "Unsaved changes".
    let mut h = harness(normal, running_app(&dir, 0, false));
    h.state_mut().set_tab(Tab::Settings);
    h.state_mut().settings_mut().values.max_clip_secs = 90;
    save(&mut h, "settings_dirty");

    let (mut cfg, path) = config(&dir);
    cfg.telegram.token = None;
    let setup = DogitorApp::new(Phase::Setup, SettingsForm::from_config(&cfg), path);
    let mut h = harness(egui::vec2(980.0, 1100.0), setup);
    save(&mut h, "setup");
}
