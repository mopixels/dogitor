//! Status tab: arm/disarm, live mic level with the trigger threshold, and
//! recent activity / quick actions.

use std::path::Path;

use eframe::egui;
use egui_phosphor::fill as phosphor;

use dogitor::events::RecorderCommand;
use dogitor::state::SharedState;

use crate::app::RunningState;
use crate::persist;
use crate::settings_form::{SettingsForm, TRIGGER_DB_RANGE};
use crate::theme;
use crate::widgets;

pub fn show(
    ui: &mut egui::Ui,
    state: &mut RunningState,
    form: &mut SettingsForm,
    config_path: &Path,
) {
    let sstate = state.launched.server.state.clone();

    widgets::scroll_page(ui, "status_page", |ui| {
        widgets::page_header(
            ui,
            "Status",
            "Live microphone level and recording controls.",
            |_| {},
        );

        if let Some(err) = &state.launched.mediamtx.error {
            widgets::banner(
                ui,
                phosphor::WARNING,
                &format!("MediaMTX: {err}"),
                theme::DANGER,
            );
            widgets::gap(ui);
        }
        if let Some(err) = &state.notice {
            widgets::banner(ui, phosphor::WARNING, err, theme::DANGER);
            widgets::gap(ui);
        }

        armed_card(ui, &sstate);
        widgets::gap(ui);

        widgets::titled_card(ui, phosphor::WAVEFORM, "Microphone level", |ui| {
            let current = sstate.current_db();
            widgets::split_row(ui, widgets::muted("Current level"), value_text(current, 1));
            widgets::level_meter(ui, current, sstate.trigger_db(), TRIGGER_DB_RANGE);

            ui.add_space(-4.0);
            let mut trigger = sstate.trigger_db();
            let response = widgets::fill_slider(
                ui,
                egui::Slider::new(&mut trigger, TRIGGER_DB_RANGE).step_by(1.0),
                None,
            );
            if let Err(e) =
                persist::apply_trigger(&response, trigger, form, Some(&sstate), config_path)
            {
                state.notice = Some(format!("Couldn't save the trigger threshold: {e}"));
            }

            widgets::split_row(
                ui,
                widgets::muted("Trigger threshold"),
                value_text(trigger, 0),
            );
            widgets::hint(
                ui,
                "A recording starts once the level crosses the white line and stays loud briefly. \
                 Drag the slider to move the line — it takes effect immediately.",
            );
        });
        widgets::gap(ui);

        activity_card(ui, state, &sstate);
    });
}

/// "−42.3 dBFS" in regular text color, or "—" when there's no signal yet.
fn value_text(db: f32, decimals: usize) -> egui::RichText {
    let text = if db <= -100.0 {
        "—".to_string()
    } else {
        format!("{db:.decimals$} dBFS").replace('-', "\u{2212}")
    };
    egui::RichText::new(text).color(theme::TEXT).size(13.0)
}

fn armed_card(ui: &mut egui::Ui, sstate: &SharedState) {
    let armed = sstate.armed();
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            let mut on = armed;
            if widgets::toggle_switch(ui, &mut on, "Armed").changed() {
                sstate.set_armed(on);
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    egui::RichText::new(if armed { "Armed" } else { "Disarmed" })
                        .size(17.0)
                        .color(if armed { theme::ACCENT } else { theme::TEXT }),
                );
                ui.label(widgets::muted(if armed {
                    "Watching for loud sounds."
                } else {
                    "Monitoring is paused — flip the switch to start watching."
                }));
            });
        });
    });
}

fn activity_card(ui: &mut egui::Ui, state: &RunningState, sstate: &SharedState) {
    widgets::titled_card(ui, phosphor::CLOCK_COUNTDOWN, "Activity", |ui| {
        let last = match sstate.last_trigger() {
            Some(t) => egui::RichText::new(t.format("%b %-d, %H:%M:%S").to_string())
                .color(theme::TEXT)
                .size(13.0),
            None => widgets::muted("Never"),
        };
        widgets::split_row(ui, widgets::muted("Last trigger"), last);
        ui.separator();

        ui.horizontal_wrapped(|ui| {
            if ui
                .button(format!("{}  Snapshot", phosphor::CAMERA))
                .on_hover_text("Take a photo now and send it to Telegram")
                .clicked()
            {
                let _ = state.launched.server.rec_tx.send(RecorderCommand::Snapshot);
            }

            let last_clip = sstate.last_clip();
            let open = ui
                .add_enabled(
                    last_clip.is_some(),
                    egui::Button::new(format!("{}  Open last clip", phosphor::PLAY)),
                )
                .on_disabled_hover_text("No clip recorded since Dogitor started");
            if open.clicked() {
                if let Some(clip) = last_clip {
                    let _ = std::process::Command::new("open").arg(&clip).spawn();
                }
            }

            if ui
                .button(format!("{}  Reveal folder", phosphor::FOLDER_OPEN))
                .on_hover_text("Show the clips folder in Finder")
                .clicked()
            {
                let _ = std::process::Command::new("open")
                    .arg(&state.output_dir)
                    .spawn();
            }
        });
    });
}
