use eframe::egui;

pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(228, 66, 66);
pub const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(200, 50, 50);
pub const BG: egui::Color32 = egui::Color32::from_rgb(24, 24, 28);
pub const PANEL_BG: egui::Color32 = egui::Color32::from_rgb(32, 32, 38);
pub const FIELD_BG: egui::Color32 = egui::Color32::from_rgb(42, 42, 50);
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(220, 220, 225);
pub const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(130, 130, 140);
pub const GREEN: egui::Color32 = egui::Color32::from_rgb(80, 200, 120);
pub const BORDER: egui::Color32 = egui::Color32::from_rgb(55, 55, 65);
pub const ROW_ALT: egui::Color32 = egui::Color32::from_rgb(38, 38, 45);
pub const YELLOW: egui::Color32 = egui::Color32::from_rgb(220, 180, 60);

pub fn apply_theme(ctx: &egui::Context) {
    ctx.options_mut(|opt| opt.theme_preference = egui::ThemePreference::Dark);
    let mut style = (*ctx.style()).clone();

    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
    style.visuals.window_corner_radius = egui::CornerRadius::same(8);

    style.visuals.dark_mode = true;
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = PANEL_BG;
    style.visuals.extreme_bg_color = FIELD_BG;

    style.visuals.widgets.noninteractive.bg_fill = PANEL_BG;
    style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT_DIM);

    style.visuals.widgets.inactive.bg_fill = FIELD_BG;
    style.visuals.widgets.inactive.weak_bg_fill = FIELD_BG;
    style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);

    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(52, 52, 62);
    style.visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(52, 52, 62);
    style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);

    style.visuals.widgets.active.bg_fill = ACCENT;
    style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

    style.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.4);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);

    ctx.set_style(style);
}
