//! Embedded Lucide interface icons shared by the editor and project manager.

use eframe::egui;

/// Registers image loaders for the embedded SVG interface assets.
pub fn install_loaders(context: &egui::Context) {
    egui_extras::install_image_loaders(context);
}

/// A bundled interface icon; script-language branding uses its existing assets.
#[derive(Clone, Copy)]
pub enum Icon {
    Move3d,
    Rotate3d,
    Scaling,
    Folder,
    Map,
    Image,
    Box,
    FileMusic,
    FileCog,
    FileText,
    Play,
    Square,
    Pause,
    StepForward,
    Circle,
    ChevronDown,
    Plus,
    X,
    Maximize,
    Minus,
}

impl Icon {
    /// Returns the embedded SVG, without accessing the network.
    pub fn source(self) -> egui::ImageSource<'static> {
        match self {
            Self::Move3d => egui::include_image!("../../../assets/icons/lucide/move-3d.svg"),
            Self::Rotate3d => egui::include_image!("../../../assets/icons/lucide/rotate-3d.svg"),
            Self::Scaling => egui::include_image!("../../../assets/icons/lucide/scaling.svg"),
            Self::Folder => egui::include_image!("../../../assets/icons/lucide/folder.svg"),
            Self::Map => egui::include_image!("../../../assets/icons/lucide/map.svg"),
            Self::Image => egui::include_image!("../../../assets/icons/lucide/image.svg"),
            Self::Box => egui::include_image!("../../../assets/icons/lucide/box.svg"),
            Self::FileMusic => egui::include_image!("../../../assets/icons/lucide/file-music.svg"),
            Self::FileCog => egui::include_image!("../../../assets/icons/lucide/file-cog.svg"),
            Self::FileText => egui::include_image!("../../../assets/icons/lucide/file-text.svg"),
            Self::Play => egui::include_image!("../../../assets/icons/lucide/play.svg"),
            Self::Square => egui::include_image!("../../../assets/icons/lucide/square.svg"),
            Self::Pause => egui::include_image!("../../../assets/icons/lucide/pause.svg"),
            Self::StepForward => {
                egui::include_image!("../../../assets/icons/lucide/step-forward.svg")
            }
            Self::Circle => egui::include_image!("../../../assets/icons/lucide/circle.svg"),
            Self::ChevronDown => {
                egui::include_image!("../../../assets/icons/lucide/chevron-down.svg")
            }
            Self::Plus => egui::include_image!("../../../assets/icons/lucide/plus.svg"),
            Self::X => egui::include_image!("../../../assets/icons/lucide/x.svg"),
            Self::Maximize => egui::include_image!("../../../assets/icons/lucide/maximize.svg"),
            Self::Minus => egui::include_image!("../../../assets/icons/lucide/minus.svg"),
        }
    }

    /// Uses a white SVG stroke so image tint matches the UI foreground.
    pub fn image(self, color: egui::Color32, size: f32) -> egui::Image<'static> {
        egui::Image::new(self.source())
            .fit_to_exact_size(egui::vec2(size, size))
            .tint(color)
    }

    /// Keeps the text label available to accessibility tools.
    pub fn button(self, label: &str) -> egui::Button<'static> {
        egui::Button::image_and_text(self.image(egui::Color32::WHITE, 16.0), label.to_owned())
            .image_tint_follows_text_color(true)
    }
}
