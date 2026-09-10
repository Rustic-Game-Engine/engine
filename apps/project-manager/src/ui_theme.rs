use eframe::egui;

pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(211, 139, 77);
pub const CANVAS: egui::Color32 = egui::Color32::from_rgb(17, 19, 23);
pub const PANEL: egui::Color32 = egui::Color32::from_rgb(23, 26, 31);
pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(29, 33, 39);
pub const BORDER: egui::Color32 = egui::Color32::from_rgb(50, 56, 65);
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(226, 229, 234);
pub const MUTED: egui::Color32 = egui::Color32::from_rgb(143, 151, 163);
pub const WARNING: egui::Color32 = egui::Color32::from_rgb(224, 174, 86);
pub const SUCCESS: egui::Color32 = egui::Color32::from_rgb(100, 190, 125);

pub fn apply(context: &egui::Context, dark: bool) {
    let theme = if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    context.set_theme(theme);
    let mut style = (*context.style_of(theme)).clone();
    style.spacing.item_spacing = egui::vec2(9.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = 30.0;
    style.spacing.window_margin = egui::Margin::same(16);
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(22.0));

    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    if dark {
        visuals.panel_fill = PANEL;
        visuals.window_fill = SURFACE;
        visuals.extreme_bg_color = CANVAS;
        visuals.faint_bg_color = egui::Color32::from_rgb(35, 39, 46);
        visuals.widgets.noninteractive.bg_fill = PANEL;
        visuals.widgets.inactive.weak_bg_fill = SURFACE;
        visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(43, 48, 56);
        visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(81, 54, 36);
        visuals.widgets.noninteractive.fg_stroke.color = TEXT;
        visuals.widgets.inactive.fg_stroke.color = TEXT;
    }
    visuals.selection.bg_fill = if dark {
        egui::Color32::from_rgb(92, 60, 39)
    } else {
        egui::Color32::from_rgb(244, 213, 181)
    };
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = WARNING;
    visuals.window_stroke = egui::Stroke::new(
        1.0,
        if dark {
            BORDER
        } else {
            egui::Color32::from_gray(205)
        },
    );
    visuals.window_corner_radius = egui::CornerRadius::same(8);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(6);
    }
    style.visuals = visuals;
    context.set_style_of(theme, style);
}

pub fn primary_button(text: impl Into<egui::WidgetText>) -> egui::Button<'static> {
    egui::Button::new(text)
        .fill(ACCENT)
        .stroke(egui::Stroke::NONE)
}
