use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use eframe::egui::{self, Color32, CornerRadius, Stroke, Style, Visuals};

// ─── Theme mode ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }

    pub fn all() -> &'static [ThemeMode] {
        &[ThemeMode::Dark, ThemeMode::Light]
    }
}

// Global atomic storing the current mode (0 = Dark, 1 = Light).
static THEME_MODE: AtomicU8 = AtomicU8::new(0);

pub fn set_theme_mode(mode: ThemeMode) {
    THEME_MODE.store(match mode { ThemeMode::Dark => 0, ThemeMode::Light => 1 }, Ordering::Relaxed);
}

pub fn theme_mode() -> ThemeMode {
    match THEME_MODE.load(Ordering::Relaxed) {
        1 => ThemeMode::Light,
        _ => ThemeMode::Dark,
    }
}

fn is_dark() -> bool { theme_mode() == ThemeMode::Dark }

// ─── Custom font ────────────────────────────────────────────────────────

static FONT_INSTALLED: AtomicBool = AtomicBool::new(false);

fn install_custom_font(ctx: &egui::Context) {
    if FONT_INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }
    let font_data = include_bytes!("../../assets/fonts/default.otf");
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "custom_default".to_owned(),
        egui::FontData::from_static(font_data).into(),
    );
    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "custom_default".to_owned());
    fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "custom_default".to_owned());
    ctx.set_fonts(fonts);
}

// ─── Semantic colors ────────────────────────────────────────────────────
// Each function picks between dark / light variant.

// Accent / status (same in both modes)
pub fn accent_blue() -> Color32 { Color32::from_rgb(10, 132, 255) }
pub fn success_green() -> Color32 { Color32::from_rgb(48, 209, 88) }
pub fn warning_orange() -> Color32 { Color32::from_rgb(255, 159, 10) }
pub fn error_red() -> Color32 { if is_dark() { Color32::from_rgb(255, 69, 58) } else { Color32::from_rgb(220, 40, 30) } }

// Panel backgrounds
pub fn sidebar_bg() -> Color32 { if is_dark() { Color32::from_rgb(28, 28, 30) } else { Color32::from_rgb(240, 240, 242) } }
pub fn toolbar_bg() -> Color32 { if is_dark() { Color32::from_rgb(36, 36, 38) } else { Color32::from_rgb(232, 232, 234) } }
pub fn statusbar_bg() -> Color32 { toolbar_bg() }
pub fn panel_bg() -> Color32 { if is_dark() { Color32::from_rgb(36, 36, 38) } else { Color32::from_rgb(246, 246, 248) } }

// General surfaces
pub fn window_fill() -> Color32 { if is_dark() { Color32::from_rgb(28, 28, 30) } else { Color32::from_rgb(248, 248, 250) } }
pub fn faint_bg() -> Color32 { if is_dark() { Color32::from_rgb(44, 44, 46) } else { Color32::from_rgb(232, 232, 234) } }
pub fn extreme_bg() -> Color32 { if is_dark() { Color32::from_rgb(22, 22, 24) } else { Color32::from_rgb(255, 255, 255) } }

// Widget backgrounds
pub fn widget_bg() -> Color32 { if is_dark() { Color32::from_rgb(44, 44, 46) } else { Color32::from_rgb(228, 228, 230) } }
pub fn widget_bg_inactive() -> Color32 { if is_dark() { Color32::from_rgb(54, 54, 56) } else { Color32::from_rgb(220, 220, 222) } }
pub fn widget_bg_hovered() -> Color32 { if is_dark() { Color32::from_rgb(64, 64, 68) } else { Color32::from_rgb(210, 210, 214) } }
pub fn button_bg() -> Color32 { widget_bg_inactive() }

// Borders / dividers
pub fn separator_color() -> Color32 { if is_dark() { Color32::from_rgb(50, 50, 52) } else { Color32::from_rgb(200, 200, 204) } }
pub fn border_color() -> Color32 { if is_dark() { Color32::from_rgb(55, 55, 58) } else { Color32::from_rgb(195, 195, 200) } }
pub fn card_border() -> Color32 { if is_dark() { Color32::from_rgb(50, 50, 54) } else { Color32::from_rgb(200, 200, 205) } }

// Card / info box
pub fn card_bg() -> Color32 { if is_dark() { Color32::from_rgb(18, 18, 20) } else { Color32::from_rgb(255, 255, 255) } }
pub fn card_bg_elevated() -> Color32 { if is_dark() { Color32::from_rgb(44, 44, 46) } else { Color32::from_rgb(240, 240, 242) } }
pub fn card_shadow() -> Color32 { if is_dark() { Color32::from_rgba_premultiplied(0, 0, 0, 40) } else { Color32::from_rgba_premultiplied(0, 0, 0, 15) } }
pub fn card_icon_stroke() -> Color32 { if is_dark() { Color32::from_rgb(72, 72, 76) } else { Color32::from_rgb(180, 180, 185) } }

// Settings dialog specific
pub fn settings_sidebar_bg() -> Color32 { if is_dark() { Color32::from_rgb(30, 30, 32) } else { Color32::from_rgb(236, 236, 238) } }
pub fn settings_selected_bg() -> Color32 { if is_dark() { Color32::from_rgb(50, 50, 55) } else { Color32::from_rgb(210, 215, 225) } }
pub fn settings_hover_bg() -> Color32 { if is_dark() { Color32::from_rgb(42, 42, 46) } else { Color32::from_rgb(220, 220, 224) } }

// Disk card
pub fn disk_card_bg() -> Color32 { if is_dark() { Color32::from_rgb(38, 38, 40) } else { Color32::from_rgb(242, 242, 244) } }
pub fn disk_card_border() -> Color32 { border_color() }

// Info bar (host info in settings)
pub fn info_bar_bg() -> Color32 { if is_dark() { Color32::from_rgb(35, 40, 48) } else { Color32::from_rgb(220, 230, 245) } }
pub fn info_bar_text() -> Color32 { if is_dark() { Color32::from_rgb(160, 170, 185) } else { Color32::from_rgb(60, 70, 90) } }

// Warning banner
pub fn warning_banner_bg() -> Color32 { if is_dark() { Color32::from_rgb(60, 40, 20) } else { Color32::from_rgb(255, 240, 210) } }
pub fn warning_banner_text() -> Color32 { if is_dark() { Color32::from_rgb(210, 180, 130) } else { Color32::from_rgb(120, 80, 20) } }

// Danger / destructive
pub fn danger_red() -> Color32 { if is_dark() { Color32::from_rgb(255, 80, 80) } else { Color32::from_rgb(210, 40, 40) } }
pub fn danger_button_bg() -> Color32 { if is_dark() { Color32::from_rgb(180, 40, 40) } else { Color32::from_rgb(210, 50, 50) } }

// Text hierarchy
pub fn text_primary() -> Color32 { if is_dark() { Color32::from_rgb(230, 230, 235) } else { Color32::from_rgb(30, 30, 32) } }
pub fn text_secondary() -> Color32 { if is_dark() { Color32::from_rgb(142, 142, 147) } else { Color32::from_rgb(100, 100, 105) } }
pub fn text_tertiary() -> Color32 { if is_dark() { Color32::from_rgb(99, 99, 102) } else { Color32::from_rgb(140, 140, 145) } }
pub fn text_bright() -> Color32 { if is_dark() { Color32::from_rgb(240, 240, 245) } else { Color32::from_rgb(20, 20, 22) } }
pub fn text_value() -> Color32 { if is_dark() { Color32::from_rgb(220, 220, 225) } else { Color32::from_rgb(40, 40, 44) } }
pub fn text_muted() -> Color32 { if is_dark() { Color32::from_rgb(110, 110, 115) } else { Color32::from_rgb(140, 140, 145) } }
pub fn text_dim() -> Color32 { if is_dark() { Color32::from_rgb(100, 100, 105) } else { Color32::from_rgb(150, 150, 155) } }
pub fn text_subtle() -> Color32 { if is_dark() { Color32::from_rgb(120, 120, 125) } else { Color32::from_rgb(130, 130, 135) } }
pub fn text_placeholder() -> Color32 { if is_dark() { Color32::from_rgb(130, 130, 135) } else { Color32::from_rgb(150, 150, 155) } }
pub fn text_disabled() -> Color32 { if is_dark() { Color32::from_rgb(160, 160, 160) } else { Color32::from_rgb(140, 140, 140) } }
pub fn text_on_accent() -> Color32 { Color32::WHITE }

// Folder header
pub fn folder_header_text() -> Color32 { if is_dark() { Color32::from_rgb(210, 210, 215) } else { Color32::from_rgb(50, 50, 55) } }
pub fn empty_folder_text() -> Color32 { if is_dark() { Color32::from_rgb(72, 72, 74) } else { Color32::from_rgb(170, 170, 175) } }

// Sidebar VM icon tint
pub fn icon_tint_active() -> Color32 { Color32::WHITE }
pub fn icon_tint_inactive() -> Color32 { if is_dark() { Color32::from_rgb(110, 110, 115) } else { Color32::from_rgb(150, 150, 155) } }

// Monospace / path text
pub fn text_mono() -> Color32 { if is_dark() { Color32::from_rgb(180, 180, 180) } else { Color32::from_rgb(60, 60, 65) } }

// ─── Apply theme ────────────────────────────────────────────────────────

pub fn apply_theme(ctx: &egui::Context) {
    install_custom_font(ctx);

    let mut style = Style::default();
    let mut visuals = if is_dark() { Visuals::dark() } else { Visuals::light() };

    visuals.window_fill = window_fill();
    visuals.panel_fill = panel_bg();
    visuals.faint_bg_color = faint_bg();
    visuals.extreme_bg_color = extreme_bg();

    // Widget styling — softer, rounder
    visuals.widgets.noninteractive.bg_fill = widget_bg();
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text_secondary());
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(0.5, separator_color());

    visuals.widgets.inactive.bg_fill = widget_bg_inactive();
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, text_secondary());
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);

    visuals.widgets.hovered.bg_fill = widget_bg_hovered();
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, text_primary());
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);

    visuals.widgets.active.bg_fill = accent_blue();
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, text_on_accent());
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    visuals.selection.bg_fill = accent_blue();
    visuals.selection.stroke = Stroke::new(1.0, text_on_accent());

    style.visuals = visuals;

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(14.0, 7.0);
    style.spacing.window_margin = egui::Margin::same(16);

    ctx.set_style(style);
}
