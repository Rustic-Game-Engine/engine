use eframe::egui;

/// Use egui's built-in theme and widget defaults, as in the egui demo.
pub fn apply(context: &egui::Context) {
    context.set_style_of(egui::Theme::Dark, egui::Style::default());
    context.set_style_of(
        egui::Theme::Light,
        egui::Style {
            visuals: egui::Visuals::light(),
            ..Default::default()
        },
    );
}

pub fn workspace<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    // Dock panes are transparent. Paint the standard panel background beneath
    // them instead of exposing eframe's dark clear color in light mode.
    egui::CentralPanel::default().show(ui, add_contents)
}

pub fn section(ui: &mut egui::Ui, title: &str, detail: impl Into<egui::WidgetText>) {
    ui.horizontal(|ui| {
        ui.strong(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(detail);
        });
    });
}

pub fn property_grid<R>(
    ui: &mut egui::Ui,
    add_rows: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::group(ui.style()).show(ui, add_rows)
}

pub fn component_card<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.strong(title);
        ui.separator();
        add_contents(ui)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_background_tracks_theme_changes() {
        let context = egui::Context::default();
        apply(&context);
        for theme in [egui::Theme::Dark, egui::Theme::Light, egui::Theme::Dark] {
            context.set_theme(theme);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    egui::Panel::top("toolbar").show(ui, |ui| {
                        ui.label("View");
                    });
                    let remaining = ui.available_rect_before_wrap();
                    let response = workspace(ui, |ui| {
                        ui.label("Inspector");
                    });
                    assert_eq!(response.response.rect, remaining);
                },
            );
            output.textures_delta.clear();
            let background = theme.default_visuals().panel_fill;
            assert!(
                output.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Rect(rect)
                    if rect.fill == background && rect.rect.contains(screen.center()))
                }),
                "the dock workspace must paint the {theme:?} panel background"
            );
        }
    }
}
