//! Clips tab: an evenly spaced grid of clip cards that stretches to fill the
//! page width, with play / reveal / delete actions per clip.

use eframe::egui;
use egui_phosphor::fill as phosphor;

use dogitor::clips::ClipInfo;

use crate::app::RunningState;
use crate::theme;
use crate::widgets;

/// Cards never get narrower than this; the column count adapts instead.
const MIN_CARD_WIDTH: f32 = 200.0;

pub fn show(ui: &mut egui::Ui, state: &mut RunningState) {
    let clips = state.launched.clips.lock().unwrap().clone();

    widgets::scroll_page(ui, "clips_page", |ui| {
        let subtitle = match clips.len() {
            0 => "Nothing recorded yet.".to_string(),
            1 => "1 clip".to_string(),
            n => format!("{n} clips, newest first"),
        };
        widgets::page_header(ui, "Clips", &subtitle, |ui| {
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

        if clips.is_empty() {
            widgets::empty_state(
                ui,
                phosphor::FILM_STRIP,
                "Clips appear here after Dogitor detects a loud sound.",
            );
            return;
        }

        clip_grid(ui, state, &clips);
    });
}

fn clip_grid(ui: &mut egui::Ui, state: &mut RunningState, clips: &[ClipInfo]) {
    let gap = widgets::GAP;
    let available = ui.available_width();
    let columns = (((available + gap) / (MIN_CARD_WIDTH + gap)).floor() as usize).max(1);
    let card_width = (available - gap * (columns - 1) as f32) / columns as f32;

    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    for row in clips.chunks(columns) {
        ui.horizontal_top(|ui| {
            for clip in row {
                clip_card(ui, state, clip, card_width);
            }
        });
    }
}

fn clip_card(ui: &mut egui::Ui, state: &mut RunningState, clip: &ClipInfo, width: f32) {
    ui.vertical(|ui| {
        ui.set_width(width);
        egui::Frame::NONE
            .fill(theme::BG_CARD)
            .stroke(egui::Stroke::new(1.0, theme::STROKE))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                let inner = ui.available_width();
                ui.set_width(inner);
                ui.spacing_mut().item_spacing.y = 4.0;

                thumbnail(ui, clip, inner);
                ui.add_space(4.0);

                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&clip.name)
                            .color(theme::TEXT)
                            .size(13.0),
                    )
                    .truncate(),
                );
                ui.add(
                    egui::Label::new(
                        widgets::muted(format!(
                            "{} \u{b7} {:.0}s \u{b7} {:.1} MB",
                            clip.started_at.format("%b %-d, %H:%M"),
                            clip.duration_s,
                            clip.size_bytes as f64 / 1_048_576.0
                        ))
                        .size(12.0),
                    )
                    .truncate(),
                );
                ui.add_space(4.0);
                clip_actions(ui, state, clip);
            });
    });
}

/// A fixed 16:9 slot (so every card is the same height), with the thumbnail
/// letterboxed and centered inside it whatever its own aspect ratio.
fn thumbnail(ui: &mut egui::Ui, clip: &ClipInfo, width: f32) {
    let size = egui::vec2(width, (width * 9.0 / 16.0).round());
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let radius = egui::CornerRadius::same(6);
    ui.painter().rect_filled(rect, radius, theme::BG_PANEL);

    let painted = clip.thumb_path.as_ref().is_some_and(|thumb| {
        let image = egui::Image::new(format!("file://{}", thumb.display())).corner_radius(radius);
        match image.load_and_calc_size(ui, rect.size()) {
            Some(img_size) => {
                image.paint_at(ui, egui::Rect::from_center_size(rect.center(), img_size));
                true
            }
            None => false,
        }
    });
    if !painted {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            phosphor::FILM_STRIP,
            egui::FontId::proportional(28.0),
            theme::TEXT_MUTED,
        );
    }
}

fn clip_actions(ui: &mut egui::Ui, state: &mut RunningState, clip: &ClipInfo) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let confirming = state.clip_pending_delete.as_deref() == Some(clip.path.as_path());
        if confirming {
            if ui.add(widgets::danger_button("Delete")).clicked() {
                let _ = std::fs::remove_file(&clip.path);
                if let Some(thumb) = &clip.thumb_path {
                    let _ = std::fs::remove_file(thumb);
                }
                state
                    .launched
                    .clips
                    .lock()
                    .unwrap()
                    .retain(|c| c.path != clip.path);
                state.clip_pending_delete = None;
            }
            if ui.button("Cancel").clicked() {
                state.clip_pending_delete = None;
            }
            return;
        }

        if ui.button(phosphor::PLAY).on_hover_text("Play").clicked() {
            let _ = std::process::Command::new("open").arg(&clip.path).spawn();
        }
        if ui
            .button(phosphor::ARROW_SQUARE_OUT)
            .on_hover_text("Reveal in Finder")
            .clicked()
        {
            let _ = std::process::Command::new("open")
                .arg("-R")
                .arg(&clip.path)
                .spawn();
        }
        if ui
            .button(phosphor::TRASH)
            .on_hover_text("Delete…")
            .clicked()
        {
            state.clip_pending_delete = Some(clip.path.clone());
        }
    });
}
