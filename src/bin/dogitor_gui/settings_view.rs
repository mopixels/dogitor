//! Settings tab (and the first-run setup screen, which is the same form with
//! a welcome header): sectioned cards in a scroll area, with a save bar
//! pinned to the bottom that shows whether there are unsaved changes.

use std::path::Path;

use eframe::egui;
use egui_phosphor::fill as phosphor;

use dogitor::state::SharedState;

use crate::persist;
use crate::settings_form::{random_token, SettingsForm, TRIGGER_DB_RANGE};
use crate::theme;
use crate::widgets;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    /// First run: nothing is running yet; "Save & Start" is always enabled.
    Setup,
    /// Normal Settings tab; "Save & Restart" is enabled only with changes.
    Running,
}

/// Draws the form. Returns `true` the frame the save button is clicked.
pub fn show(
    ui: &mut egui::Ui,
    form: &mut SettingsForm,
    mode: Mode,
    live: Option<&SharedState>,
    config_path: &Path,
) -> bool {
    form.ensure_cameras_listed();
    let save_clicked = save_bar(ui, form, mode);

    widgets::scroll_page(ui, "settings_page", |ui| {
        match mode {
            Mode::Setup => welcome_header(ui),
            Mode::Running => widgets::page_header(
                ui,
                "Settings",
                "Changes take effect after Save & Restart (sensitivity applies immediately).",
                |_| {},
            ),
        }

        telegram_card(ui, form);
        widgets::gap(ui);
        sensitivity_card(ui, form, live, config_path);
        widgets::gap(ui);
        camera_card(ui, form);
        widgets::gap(ui);
        phone_card(ui, form);
        widgets::gap(ui);
        advanced_section(ui, form);
        ui.add_space(8.0);
    });

    save_clicked
}

fn welcome_header(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(phosphor::PAW_PRINT)
                .size(40.0)
                .color(theme::ACCENT),
        );
        ui.heading(
            egui::RichText::new("Welcome to Dogitor")
                .size(24.0)
                .color(theme::TEXT),
        );
        ui.label(
            widgets::muted(
                "Fill in a couple of details below, then start watching your home. \
                 You only need to do this once.",
            )
            .size(13.5),
        );
    });
    ui.add_space(16.0);
}

/// Bottom bar: validation error (if any), dirty indicator on the left,
/// Discard + primary Save on the right.
fn save_bar(ui: &mut egui::Ui, form: &mut SettingsForm, mode: Mode) -> bool {
    let mut save_clicked = false;
    let dirty = form.is_dirty();

    egui::TopBottomPanel::bottom("settings_save_bar")
        .frame(
            egui::Frame::NONE
                .fill(theme::BG_WINDOW)
                .inner_margin(egui::Margin::symmetric(0, 12)),
        )
        .show_inside(ui, |ui| {
            widgets::centered_column(ui, widgets::PAGE_WIDTH, |ui| {
                if let Some(err) = form.error.clone() {
                    widgets::banner(ui, phosphor::WARNING, &err, theme::DANGER);
                }
                ui.horizontal(|ui| {
                    match mode {
                        Mode::Setup => {
                            ui.label(widgets::muted(
                                "You can change all of this later in Settings.",
                            ));
                        }
                        Mode::Running if dirty => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  Unsaved changes",
                                    phosphor::CIRCLE
                                ))
                                .color(theme::ACCENT)
                                .size(12.5),
                            );
                        }
                        Mode::Running => {
                            ui.label(widgets::muted("All changes saved."));
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = match mode {
                            Mode::Setup => "Save & Start",
                            Mode::Running => "Save & Restart",
                        };
                        let enabled = mode == Mode::Setup || dirty;
                        if ui
                            .add_enabled(enabled, widgets::primary_button(label, enabled))
                            .clicked()
                        {
                            save_clicked = true;
                        }
                        if mode == Mode::Running && dirty && ui.button("Discard").clicked() {
                            form.discard();
                        }
                    });
                });
            });
        });

    save_clicked
}

fn telegram_card(ui: &mut egui::Ui, form: &mut SettingsForm) {
    widgets::titled_card(ui, phosphor::PAPER_PLANE_TILT, "Telegram alerts", |ui| {
        widgets::caption(ui, "Bot token");
        secret_field(
            ui,
            &mut form.values.telegram_token,
            &mut form.show_telegram_token,
        );
        widgets::text_row(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(widgets::muted("Get one from").size(12.0));
            ui.hyperlink_to(
                egui::RichText::new("@BotFather").size(12.0),
                "https://t.me/BotFather",
            );
            ui.label(widgets::muted("in Telegram.").size(12.0));
        });

        widgets::caption(ui, "Chat ID");
        ui.add(
            widgets::text_field(ui, &mut form.values.telegram_chat_id, None)
                .hint_text("Filled in automatically"),
        );
        widgets::hint(
            ui,
            "Send /start to your bot in Telegram and this fills in (and saves) on its own.",
        );
    });
}

fn sensitivity_card(
    ui: &mut egui::Ui,
    form: &mut SettingsForm,
    live: Option<&SharedState>,
    config_path: &Path,
) {
    widgets::titled_card(ui, phosphor::WAVEFORM, "Sensitivity", |ui| {
        widgets::hint(
            ui,
            "How loud does a sound need to be before Dogitor notices it?",
        );

        let mut db = form.values.trigger_db;
        let response = widgets::fill_slider(
            ui,
            egui::Slider::new(&mut db, TRIGGER_DB_RANGE).step_by(1.0),
            None,
        );
        if let Err(e) = persist::apply_trigger(&response, db, form, live, config_path) {
            form.error = Some(format!("Couldn't save sensitivity: {e}"));
        }

        ui.add_space(-6.0);
        widgets::split_row(
            ui,
            widgets::muted("More sensitive").size(12.0),
            widgets::muted("Less sensitive").size(12.0),
        );
        widgets::hint(
            ui,
            "More sensitive picks up quieter sounds but may trigger on background noise; \
             less sensitive waits for something clearly loud, like a bark.",
        );
    });
}

fn camera_card(ui: &mut egui::Ui, form: &mut SettingsForm) {
    widgets::titled_card(ui, phosphor::VIDEO_CAMERA, "Camera", |ui| {
        widgets::caption(ui, "Device");
        let listing = form.listing_cameras();
        let selected = if listing && form.cameras.is_empty() {
            "Looking for cameras…".to_string()
        } else {
            form.selected_camera_name()
        };
        let refresh = widgets::field_with_button(
            ui,
            |ui, width| {
                egui::ComboBox::from_id_salt("camera_combo")
                    .width(width)
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for (idx, name) in &form.cameras {
                            ui.selectable_value(&mut form.values.video_device, idx.clone(), name);
                        }
                    });
            },
            |ui| {
                ui.add_enabled(!listing, egui::Button::new(phosphor::ARROW_CLOCKWISE))
                    .on_hover_text("Look for cameras again")
            },
        );
        if refresh.clicked() {
            form.refresh_cameras();
        }
        if let Some(err) = &form.camera_error {
            widgets::banner(ui, phosphor::WARNING, err, theme::DANGER);
        }

        ui.add_space(2.0);
        widgets::split_row(
            ui,
            widgets::muted("Clip length limit"),
            egui::RichText::new(format!("{} sec", form.values.max_clip_secs))
                .color(theme::TEXT)
                .size(13.0),
        );
        widgets::fill_slider(
            ui,
            egui::Slider::new(&mut form.values.max_clip_secs, 10..=180),
            None,
        );

        widgets::caption(ui, "Clips folder");
        let path = tilde_path(&form.values.output_dir);
        let change = widgets::field_with_button(
            ui,
            |ui, width| {
                let mut text = path.as_str();
                ui.add(widgets::text_field(ui, &mut text, Some(width)))
                    .on_hover_text(&path);
            },
            |ui| ui.button("Change…"),
        );
        if change.clicked() {
            form.pick_output_dir();
        }
    });
}

fn phone_card(ui: &mut egui::Ui, form: &mut SettingsForm) {
    widgets::titled_card(ui, phosphor::DEVICE_MOBILE, "Phone app access", |ui| {
        widgets::hint(
            ui,
            "Enter this in the Dogitor app on your phone to connect to this Mac.",
        );
        widgets::caption(ui, "Access code");
        secret_field(ui, &mut form.values.api_token, &mut form.show_api_token);
        ui.horizontal(|ui| {
            if ui
                .button(format!("{}  New code", phosphor::ARROW_CLOCKWISE))
                .on_hover_text("Phones using the old code will need the new one")
                .clicked()
            {
                form.values.api_token = random_token();
            }
            if ui.button(format!("{}  Copy", phosphor::COPY)).clicked() {
                ui.ctx().copy_text(form.values.api_token.clone());
            }
        });
    });
}

fn advanced_section(ui: &mut egui::Ui, form: &mut SettingsForm) {
    // `show_unindented`: the regular body indent would make these cards
    // narrower than (and misaligned with) the cards above.
    let header = egui::CollapsingHeader::new(
        egui::RichText::new(format!("{}  Advanced settings", phosphor::WRENCH)).color(theme::TEXT),
    )
    .id_salt("advanced_settings")
    .open(Some(form.show_advanced))
    .show_unindented(ui, |ui| {
        widgets::hint(
            ui,
            "For fine-tuning — most people won't need to change these.",
        );
        let v = &mut form.values;

        widgets::titled_card(ui, phosphor::SLIDERS, "Detection timing", |ui| {
            widgets::field_row(ui, "Quiet level", |ui| {
                let value = format!("{:.0} dBFS", v.quiet_db).replace('-', "\u{2212}");
                widgets::fill_slider(
                    ui,
                    egui::Slider::new(&mut v.quiet_db, -80.0..=0.0).step_by(1.0),
                    Some(value),
                )
            });
            widgets::field_row(ui, "Trigger hold", |ui| {
                let value = format!("{:.1} s", v.trigger_hold_secs);
                widgets::fill_slider(
                    ui,
                    egui::Slider::new(&mut v.trigger_hold_secs, 0.0..=5.0).step_by(0.1),
                    Some(value),
                )
            });
            widgets::field_row(ui, "Quiet timeout", |ui| {
                let value = format!("{:.1} s", v.quiet_timeout_secs);
                widgets::fill_slider(
                    ui,
                    egui::Slider::new(&mut v.quiet_timeout_secs, 0.0..=30.0).step_by(0.5),
                    Some(value),
                )
            });
            widgets::field_row(ui, "Cooldown", |ui| {
                ui.add(
                    egui::DragValue::new(&mut v.cooldown_secs)
                        .range(0..=600)
                        .suffix(" s"),
                )
            });
            widgets::hint(
                ui,
                "Trigger hold: how long it must stay loud to start. Quiet level / timeout: \
                 recording stops once it stays below the quiet level this long. Cooldown: \
                 pause before the next clip.",
            );
        });
        widgets::gap(ui);

        widgets::titled_card(ui, phosphor::FILM_STRIP, "Video", |ui| {
            widgets::field_row(ui, "Device index", |ui| {
                ui.add(widgets::text_field(ui, &mut v.video_device, Some(60.0)))
            });
            widgets::field_row(ui, "Resolution", |ui| {
                ui.add(
                    egui::DragValue::new(&mut v.width)
                        .range(160..=3840)
                        .suffix(" px"),
                );
                ui.label(widgets::muted("×"));
                ui.add(
                    egui::DragValue::new(&mut v.height)
                        .range(120..=2160)
                        .suffix(" px"),
                )
            });
            widgets::field_row(ui, "Frame rate", |ui| {
                ui.add(
                    egui::DragValue::new(&mut v.framerate)
                        .range(1..=60)
                        .suffix(" fps"),
                )
            });
        });
        widgets::gap(ui);

        widgets::titled_card(ui, phosphor::LINK, "Companion API", |ui| {
            widgets::field_row(ui, "Port", |ui| {
                ui.add(egui::DragValue::new(&mut v.api_port).range(1..=65535))
            });
        });
        widgets::gap(ui);

        widgets::titled_card(ui, phosphor::BROADCAST, "Live media (MediaMTX)", |ui| {
            widgets::field_row(ui, "RTSP URL", |ui| {
                ui.add(widgets::text_field(ui, &mut v.mediamtx_rtsp, None))
            });
            widgets::field_row(ui, "WHEP URL", |ui| {
                ui.add(widgets::text_field(ui, &mut v.whep_url, None))
                    .on_hover_text("The live-view address the phone app connects to")
            });
            widgets::field_row(ui, "Idle timeout", |ui| {
                ui.add(egui::DragValue::new(&mut v.live_idle_secs).suffix(" s"))
            });
            widgets::field_row(ui, "Lease TTL", |ui| {
                ui.add(egui::DragValue::new(&mut v.lease_ttl_secs).suffix(" s"))
            });
        });
    });
    if header.header_response.clicked() {
        form.show_advanced = !form.show_advanced;
    }
}

/// A password-style field filling the row, with a show/hide eye button.
fn secret_field(ui: &mut egui::Ui, value: &mut String, show: &mut bool) {
    let toggle = widgets::field_with_button(
        ui,
        |ui, width| {
            ui.add(widgets::text_field(ui, value, Some(width)).password(!*show));
        },
        |ui| {
            let eye = if *show {
                phosphor::EYE_SLASH
            } else {
                phosphor::EYE
            };
            ui.button(eye)
                .on_hover_text(if *show { "Hide" } else { "Show" })
        },
    );
    if toggle.clicked() {
        *show = !*show;
    }
}

/// `/Users/me/Library/…` → `~/Library/…`, for display only.
fn tilde_path(path: &Path) -> String {
    let full = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && full.starts_with(&home) => {
            format!("~{}", &full[home.len()..])
        }
        _ => full,
    }
}
