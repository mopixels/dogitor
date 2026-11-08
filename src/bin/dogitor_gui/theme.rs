//! Visual theme for the GUI, built around the app's own icon (see
//! `assets/icon.svg`): a dark slate background, a warm orange accent for
//! the primary "armed/watching" state, and a clearly distinct red reserved
//! for recording/alerts. Centralizing the palette here keeps every view
//! consistent instead of each widget picking its own colors, and loads the
//! fonts (system UI font + Phosphor icons) used throughout.

use eframe::egui;

pub const BG_WINDOW: egui::Color32 = egui::Color32::from_rgb(0x14, 0x19, 0x22);
pub const BG_PANEL: egui::Color32 = egui::Color32::from_rgb(0x1c, 0x24, 0x33);
pub const BG_CARD: egui::Color32 = egui::Color32::from_rgb(0x21, 0x2a, 0x3b);
pub const BG_CARD_HOVER: egui::Color32 = egui::Color32::from_rgb(0x29, 0x33, 0x48);
pub const BG_PRESSED: egui::Color32 = egui::Color32::from_rgb(0x32, 0x3d, 0x55);
pub const STROKE: egui::Color32 = egui::Color32::from_rgb(0x2e, 0x38, 0x4e);

pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0xff, 0x8a, 0x3d);
pub const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(0xff, 0xa1, 0x5e);
/// Text drawn *on* an `ACCENT` fill (primary buttons) — white on this
/// orange is only ~2.4:1 contrast, dark brown is ~8:1.
pub const ACCENT_TEXT: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x18, 0x08);
/// Recording / errors. A true red, so it can't be mistaken for the orange
/// "Armed" accent when both pills sit next to each other.
pub const DANGER: egui::Color32 = egui::Color32::from_rgb(0xef, 0x44, 0x44);
pub const CALM: egui::Color32 = egui::Color32::from_rgb(0x4f, 0xd1, 0xc5);

pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xee, 0xf1, 0xf6);
pub const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(0x93, 0x9d, 0xb4);

/// macOS system UI font (SF Pro). Loaded at runtime rather than bundled; if
/// it's ever missing we silently keep egui's built-in font.
const SYSTEM_FONT_PATH: &str = "/System/Library/Fonts/SFNS.ttf";

/// Linearly blend two colors (plain u8-channel interpolation — good enough
/// for UI accents, no need for gamma-correct blending at this scale).
pub fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    egui::Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Fill);

    if let Ok(bytes) = std::fs::read(SYSTEM_FONT_PATH) {
        fonts
            .font_data
            .insert("system-ui".into(), egui::FontData::from_owned(bytes).into());
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "system-ui".into());
        }
    }

    ctx.set_fonts(fonts);
}

/// Installs fonts and a dark, rounded, spacious style. Called once from
/// `main` before the window is shown.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut visuals = egui::Visuals::dark();
    // No `override_text_color`: it would force one color onto every widget
    // state and make the per-state `fg_stroke` colors below dead code.
    visuals.window_fill = BG_WINDOW;
    visuals.panel_fill = BG_WINDOW;
    visuals.extreme_bg_color = BG_PANEL;
    visuals.faint_bg_color = BG_CARD;
    visuals.code_bg_color = BG_CARD;
    visuals.hyperlink_color = ACCENT;
    visuals.window_stroke = egui::Stroke::new(1.0, STROKE);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.3);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT_HOVER);
    visuals.slider_trailing_fill = true;

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.weak_bg_fill = BG_PANEL;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, STROKE);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);

    // NOTE: sliders/checkboxes/combo-boxes paint their track/box using this
    // exact color (see egui's `slider.rs`), so it must differ from `BG_CARD`
    // or those controls become invisible when drawn inside a `widgets::card`
    // — only their handle/checkmark would show, floating with no track.
    visuals.widgets.inactive.bg_fill = BG_PANEL;
    visuals.widgets.inactive.weak_bg_fill = BG_PANEL;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, STROKE);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);

    visuals.widgets.hovered.bg_fill = BG_CARD_HOVER;
    visuals.widgets.hovered.weak_bg_fill = BG_CARD_HOVER;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, TEXT);

    // Pressed state: a brighter surface + accent outline, *not* an orange
    // fill. egui also uses `active.fg_stroke` as the `.strong()` text color
    // everywhere, so it has to stay readable on the dark background.
    visuals.widgets.active.bg_fill = BG_PRESSED;
    visuals.widgets.active.weak_bg_fill = BG_PRESSED;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT_HOVER);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, TEXT);

    visuals.widgets.open.bg_fill = BG_CARD_HOVER;
    visuals.widgets.open.weak_bg_fill = BG_CARD_HOVER;
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, TEXT);

    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = egui::CornerRadius::same(8);
    }
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);

    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.window_margin = egui::Margin::same(16);
        style.spacing.interact_size.y = 30.0;
        style.spacing.scroll.bar_width = 8.0;

        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(22.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(14.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}
