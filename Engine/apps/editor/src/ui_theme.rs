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
