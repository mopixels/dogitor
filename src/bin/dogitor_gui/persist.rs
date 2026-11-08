//! Writing the GUI's `config.toml`, plus the "live" settings that are
//! applied to the running server immediately and persisted on their own
//! (without touching any other, possibly unsaved, Settings edits).

use std::path::Path;

use anyhow::Result;
use eframe::egui;

use dogitor::config::Config;
use dogitor::state::SharedState;

use crate::settings_form::SettingsForm;

/// Write `cfg` atomically (temp file + rename), so a crash mid-write can
/// never leave a truncated config behind.
pub fn write_config(path: &Path, cfg: &Config) -> Result<()> {
    let toml_str = toml::to_string_pretty(cfg)?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, toml_str)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read the config file as-is (no env-var overrides, so secrets from
/// `.env` never get copied into it), change it, and write it back.
pub fn patch_config(path: &Path, patch: impl FnOnce(&mut Config)) -> Result<()> {
    let mut cfg: Config = toml::from_str(&std::fs::read_to_string(path)?)?;
    patch(&mut cfg);
    write_config(path, &cfg)
}

/// Shared handling for every trigger-threshold slider (Status tab and
/// Settings → Sensitivity), so they all behave the same way:
/// - while it moves, the new value is applied live and mirrored in the form;
/// - when it's released (or nudged with the keyboard), it's saved to disk.
///
/// `live` is `None` during first-run setup, where nothing is running yet and
/// the whole form is written on "Save & Start" anyway.
pub fn apply_trigger(
    response: &egui::Response,
    db: f32,
    form: &mut SettingsForm,
    live: Option<&SharedState>,
    config_path: &Path,
) -> Result<()> {
    if response.changed() {
        form.set_trigger_db(db);
        if let Some(state) = live {
            state.set_trigger_db(db);
        }
    }
    let committed = response.drag_stopped() || (response.changed() && !response.dragged());
    if committed && live.is_some() {
        let (trigger, quiet) = (form.values.trigger_db, form.values.quiet_db);
        patch_config(config_path, |cfg| {
            cfg.audio.trigger_db = trigger;
            cfg.audio.quiet_db = quiet;
        })?;
    }
    Ok(())
}
