use eframe::egui;

pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(211, 139, 77);
pub const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(230, 158, 91);
pub const CANVAS: egui::Color32 = egui::Color32::from_rgb(17, 19, 23);
pub const PANEL: egui::Color32 = egui::Color32::from_rgb(23, 26, 31);
pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(29, 33, 39);
pub const BORDER: egui::Color32 = egui::Color32::from_rgb(50, 56, 65);
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(226, 229, 234);
pub const MUTED: egui::Color32 = egui::Color32::from_rgb(143, 151, 163);
pub const WARNING: egui::Color32 = egui::Color32::from_rgb(224, 174, 86);

pub fn apply(context: &egui::Context) {
    context.set_theme(egui::Theme::Dark);
    let mut style = (*context.style_of(egui::Theme::Dark)).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 7.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 28.0;
    style.spacing.window_margin = egui::Margin::same(14);

    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(20.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
    );

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = CANVAS;
    visuals.faint_bg_color = egui::Color32::from_rgb(35, 39, 46);
    visuals.window_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = egui::CornerRadius::same(8);
    visuals.selection.bg_fill = egui::Color32::from_rgb(92, 60, 39);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT_HOVER);
    visuals.hyperlink_color = ACCENT_HOVER;
    visuals.warn_fg_color = WARNING;
    visuals.error_fg_color = egui::Color32::from_rgb(226, 107, 107);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(43, 48, 56);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(75, 82, 94));
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(81, 54, 36);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(5);
    }
    style.visuals = visuals;
    context.set_style_of(egui::Theme::Dark, style);
}

pub fn section(ui: &mut egui::Ui, title: &str, detail: impl Into<egui::WidgetText>) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title.to_uppercase())
                .size(11.0)
                .strong()
                .color(MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(detail);
        });
    });
}

pub fn property_grid<R>(
    ui: &mut egui::Ui,
    add_rows: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, add_rows)
}
