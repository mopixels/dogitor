//! Small reusable pieces shared by the Status/Clips/Settings views: page
//! layout (centered column + header), cards, form rows, a pill badge, a
//! banner, a level meter, an iOS-style toggle switch, a sidebar nav row, and
//! an empty-state placeholder. Every view builds on these so spacing,
//! widths and text styles stay identical across tabs.

use std::ops::RangeInclusive;

use eframe::egui;

use crate::theme;

/// Max content width shared by every page, so the heading and cards sit in
/// the same place whichever tab is open.
pub const PAGE_WIDTH: f32 = 720.0;
/// Width of the label column in [`field_row`] forms.
const LABEL_WIDTH: f32 = 150.0;
/// Width of the value column after a [`fill_slider`].
const SLIDER_VALUE_WIDTH: f32 = 70.0;
/// Gap between cards (and between clip cards in the grid).
pub const GAP: f32 = 12.0;

/// Vertical space between stacked cards: tops up egui's item spacing to
/// exactly [`GAP`], the same gap the clip grid uses.
pub fn gap(ui: &mut egui::Ui) {
    ui.add_space((GAP - ui.spacing().item_spacing.y).max(0.0));
}

/// Muted secondary text — captions, hints, metadata.
pub fn muted(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .color(theme::TEXT_MUTED)
        .size(12.5)
}

/// A scrollable page: a [`PAGE_WIDTH`] column centered in the available
/// space. Used by every tab so content never hugs the left edge of a wide
/// window and never jumps sideways when switching tabs.
pub fn scroll_page(ui: &mut egui::Ui, id: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| centered_column(ui, PAGE_WIDTH, add_contents));
}

/// Centers a fixed-width column inside whatever (wider) space is available.
pub fn centered_column(ui: &mut egui::Ui, width: f32, add_contents: impl FnOnce(&mut egui::Ui)) {
    let content_width = width.min(ui.available_width());
    let margin = ((ui.available_width() - content_width) / 2.0).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_width(content_width);
            add_contents(ui);
        });
    });
}

/// Page title + one-line muted subtitle, with optional actions aligned to
/// the right of the title. Identical on every tab.
pub fn page_header(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    actions: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.heading(egui::RichText::new(title).color(theme::TEXT));
            ui.label(muted(subtitle).size(13.5));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
    });
    ui.add_space(8.0);
}

/// A rounded, bordered panel used to visually group a section of controls.
/// Always claims the ui's full available width, so stacked cards line up
/// into a single tidy column.
pub fn card(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(theme::BG_CARD)
        .stroke(egui::Stroke::new(1.0, theme::STROKE))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
}

/// A [`card`] with an icon + title header row above the content — the
/// standard shape for each Settings section and each Status panel.
pub fn titled_card(
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(egui::RichText::new(icon).color(theme::ACCENT).size(16.0));
            ui.label(egui::RichText::new(title).color(theme::TEXT).size(15.0));
        });
        ui.add_space(2.0);
        add_contents(ui);
    });
}

/// A small muted caption meant to sit directly above an input.
pub fn caption(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(muted(text));
}

/// A muted help line under a field.
pub fn hint(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.add(egui::Label::new(muted(text).size(12.0)).wrap());
}

/// Two-column form row: a fixed-width muted label on the left, the control
/// filling the rest. Consecutive rows line up into a tidy grid.
pub fn field_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    add_control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        let height = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_WIDTH, height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(LABEL_WIDTH);
                ui.label(muted(label).size(13.0));
            },
        );
        add_control(ui)
    })
    .inner
}

/// Adds a slider (without egui's value box) stretched to the available
/// width instead of egui's fixed 100 px default. With `value`, the value is
/// shown right-aligned in a fixed-width column after the slider, so a stack
/// of sliders shares one right edge whatever their units.
pub fn fill_slider(
    ui: &mut egui::Ui,
    slider: egui::Slider<'_>,
    value: Option<String>,
) -> egui::Response {
    let reserve = if value.is_some() {
        SLIDER_VALUE_WIDTH + ui.spacing().item_spacing.x
    } else {
        0.0
    };
    ui.spacing_mut().slider_width = (ui.available_width() - reserve).max(60.0);
    let response = ui.add(slider.show_value(false));
    if let Some(value) = value {
        ui.allocate_ui_with_layout(
            egui::vec2(SLIDER_VALUE_WIDTH, response.rect.height()),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_width(SLIDER_VALUE_WIDTH);
                ui.label(egui::RichText::new(value).color(theme::TEXT).size(13.0));
            },
        );
    }
    response
}

/// A full-width single-line field with a trailing button (show/hide, copy,
/// "Change…"). The field always fills whatever the button leaves, so rows
/// with different buttons still share the same right edge.
pub fn field_with_button<R>(
    ui: &mut egui::Ui,
    add_field: impl FnOnce(&mut egui::Ui, f32),
    add_button: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = add_button(ui);
            let width = ui.available_width();
            add_field(ui, width);
            r
        })
        .inner
    })
    .inner
}

/// A text left, another text right, on one line (label/value readouts).
pub fn split_row(ui: &mut egui::Ui, left: egui::RichText, right: egui::RichText) {
    text_row(ui, |ui| {
        ui.label(left);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(right);
        });
    });
}

/// A horizontal row of plain text (labels/links) that's only as tall as the
/// text — `ui.horizontal` otherwise reserves a full 30 px control height,
/// which leaves a visibly larger gap above/below text-only rows.
pub fn text_row(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 0.0;
        ui.horizontal_wrapped(add_contents);
    });
}

/// The standard single-line text field: control height, text vertically
/// centered, filling `width` (or the row, if `None`).
pub fn text_field<'t>(
    ui: &egui::Ui,
    text: &'t mut dyn egui::TextBuffer,
    width: Option<f32>,
) -> egui::TextEdit<'t> {
    egui::TextEdit::singleline(text)
        .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
        .vertical_align(egui::Align::Center)
        .desired_width(width.unwrap_or(f32::INFINITY))
}

/// The app's primary call-to-action button: accent fill + dark text when
/// enabled, a plain (and then greyed-out) button when not — a faded orange
/// fill reads as "broken" rather than "nothing to do".
pub fn primary_button(text: &str, enabled: bool) -> egui::Button<'static> {
    let label = egui::RichText::new(text).size(14.5);
    let button = if enabled {
        egui::Button::new(label.color(theme::ACCENT_TEXT)).fill(theme::ACCENT)
    } else {
        egui::Button::new(label)
    };
    button.min_size(egui::vec2(120.0, 34.0))
}

/// A red-filled button for destructive confirmations.
pub fn danger_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE)).fill(theme::DANGER)
}

/// A small pill-shaped colored badge, e.g. for "Armed" / "Recording" status.
pub fn pill(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::NONE
        .fill(color.gamma_multiply(0.16))
        .stroke(egui::Stroke::new(1.0, color))
        .corner_radius(egui::CornerRadius::same(20))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(color).size(12.5));
        });
}

/// A colored banner for warnings/errors (MediaMTX problems, form validation).
/// Long messages wrap inside the banner instead of overflowing the card.
pub fn banner(ui: &mut egui::Ui, icon: &str, text: &str, color: egui::Color32) {
    egui::Frame::NONE
        .fill(color.gamma_multiply(0.14))
        .stroke(egui::Stroke::new(1.0, color))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(icon).color(color));
                ui.add(egui::Label::new(egui::RichText::new(text).color(theme::TEXT)).wrap());
            });
        });
}

/// Centered "nothing here yet" placeholder for empty lists.
pub fn empty_state(ui: &mut egui::Ui, icon: &str, message: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.label(
            egui::RichText::new(icon)
                .size(40.0)
                .color(theme::TEXT_MUTED),
        );
        ui.add_space(4.0);
        ui.label(muted(message).size(14.0));
    });
}

/// An iOS-style on/off switch — a friendlier "consumer app" affordance than
/// a checkbox for the single most important control (Armed/Disarmed).
/// Mirrors `ui.checkbox`'s usage: pass `&mut bool`, check `.changed()`.
pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool, label: &str) -> egui::Response {
    let desired_size = egui::vec2(46.0, 26.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label)
    });

    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool(response.id, *on);
        let track = theme::lerp_color(theme::STROKE, theme::ACCENT, how_on);
        let radius = 0.5 * rect.height();
        ui.painter().rect(
            rect,
            radius,
            track,
            egui::Stroke::NONE,
            egui::StrokeKind::Inside,
        );
        let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        let center = egui::pos2(knob_x, rect.center().y);
        ui.painter().circle(
            center,
            radius - 3.0,
            egui::Color32::WHITE,
            egui::Stroke::NONE,
        );
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Horizontal distance from a full-width slider's edge to its handle at
/// min/max. egui shrinks the handle's travel by its radius (`height / 2.5`);
/// [`level_meter`] maps values onto the same span, so a slider drawn
/// directly under the meter has its handle exactly on the threshold tick.
fn slider_handle_inset(ui: &egui::Ui) -> f32 {
    let thickness = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.spacing().interact_size.y);
    thickness / 2.5
}

/// A horizontal meter for the current mic dBFS reading: a filled bar that
/// shades from calm (quiet) through accent (approaching the trigger) to
/// danger (at/above the trigger), plus a tick mark at the trigger threshold.
/// Spans the full width, like the threshold slider meant to sit under it.
pub fn level_meter(ui: &mut egui::Ui, db: f32, trigger_db: f32, range: RangeInclusive<f32>) {
    let desired_size = egui::vec2(ui.available_width(), 16.0);
    let (rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }

    let inset = slider_handle_inset(ui);
    let (lo, hi) = (*range.start(), *range.end());
    let x_for = |v: f32| {
        let frac = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
        rect.left() + inset + frac * (rect.width() - 2.0 * inset)
    };

    let radius = egui::CornerRadius::same(8);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, theme::BG_PANEL);

    if db > lo {
        let fill_rect = egui::Rect::from_min_max(rect.min, egui::pos2(x_for(db), rect.max.y));
        let color = if db >= trigger_db {
            theme::DANGER
        } else {
            let t = ((db - (trigger_db - 20.0)) / 20.0).clamp(0.0, 1.0);
            theme::lerp_color(theme::CALM, theme::ACCENT, t)
        };
        painter.rect_filled(fill_rect, radius, color);
    }

    let x = x_for(trigger_db);
    painter.line_segment(
        [
            egui::pos2(x, rect.top() - 3.0),
            egui::pos2(x, rect.bottom() + 3.0),
        ],
        egui::Stroke::new(2.0, theme::TEXT),
    );
}

/// One row in the sidebar nav: an icon + label that highlights when
/// `selected` and on hover, with an optional accent dot (e.g. "unsaved
/// settings"). Returns `true` on click.
pub fn nav_item(ui: &mut egui::Ui, icon: &str, label: &str, selected: bool, dot: bool) -> bool {
    let desired_size = egui::vec2(ui.available_width(), 36.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
    });
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if ui.is_rect_visible(rect) {
        let bg = if selected {
            theme::ACCENT.gamma_multiply(0.18)
        } else if response.hovered() {
            theme::BG_CARD_HOVER
        } else {
            egui::Color32::TRANSPARENT
        };
        let fg = if selected {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        };
        let painter = ui.painter();
        painter.rect_filled(rect, egui::CornerRadius::same(8), bg);
        painter.text(
            egui::pos2(rect.left() + NAV_INSET, rect.center().y),
            egui::Align2::LEFT_CENTER,
            icon,
            egui::FontId::proportional(17.0),
            fg,
        );
        painter.text(
            egui::pos2(rect.left() + NAV_INSET + 28.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(14.0),
            if selected {
                theme::TEXT
            } else {
                theme::TEXT_MUTED
            },
        );
        if dot {
            painter.circle_filled(
                egui::pos2(rect.right() - NAV_INSET - 3.0, rect.center().y),
                3.5,
                theme::ACCENT,
            );
        }
    }

    response.clicked()
}

/// Left padding inside sidebar nav rows. The brand row and status pills use
/// the same inset (via [`inset`]) so everything in the sidebar shares one
/// left edge.
pub const NAV_INSET: f32 = 10.0;

/// Indent content by `left` pixels.
pub fn inset<R>(ui: &mut egui::Ui, left: f32, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: left as i8,
            ..Default::default()
        })
        .show(ui, add_contents)
        .inner
}
