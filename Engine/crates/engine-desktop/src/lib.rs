//! Shared chrome and update notifications for the native desktop applications.

pub mod icons;

use eframe::egui::{self, Align, Layout, Sense, ViewportCommand};
use engine_platform::{DesktopUpdate, DesktopUpdateClient};
use std::time::{Duration, Instant};

/// Keeps an update available after its single notification has been dismissed.
pub struct DesktopUpdates {
    client: Option<DesktopUpdateClient>,
    notification: UpdateNotification,
    last_poll: Option<Instant>,
    error: Option<String>,
    install_requested: bool,
    install_cancelled: bool,
}

impl Default for DesktopUpdates {
    fn default() -> Self {
        let (client, error) = match DesktopUpdateClient::discover() {
            Ok(client) => (client, None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            client,
            notification: UpdateNotification::default(),
            last_poll: None,
            error,
            install_requested: false,
            install_cancelled: false,
        }
    }
}

/// Background windows must not consume a release's one-time notification.
#[derive(Default)]
struct UpdateNotification {
    pending: Option<DesktopUpdate>,
    prompt_open: bool,
    prompt_checked: bool,
}

impl UpdateNotification {
    fn refresh(
        &mut self,
        pending: Option<DesktopUpdate>,
        focused: bool,
        claim: impl FnOnce(&DesktopUpdate) -> std::io::Result<bool>,
    ) -> std::io::Result<()> {
        if self.pending != pending {
            self.pending = pending;
            self.prompt_open = false;
            self.prompt_checked = false;
        }
        if focused
            && !self.prompt_checked
            && let Some(update) = &self.pending
        {
            self.prompt_open = claim(update)?;
            self.prompt_checked = true;
        }
        Ok(())
    }
}

impl DesktopUpdates {
    /// Draws the title bar and one-time prompt; returns a graceful-update request.
    pub fn show(&mut self, ui: &mut egui::Ui, title: &str) -> bool {
        self.poll(ui.ctx());
        let mut install = title_bar(ui, title, self.notification.pending.is_some());
        if self.notification.prompt_open {
            let mut open = true;
            egui::Window::new("Update available")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui, |ui| {
                    if let Some(update) = &self.notification.pending {
                        ui.label(format!("{} is ready to install.", update.tag));
                    }
                    ui.label("Install the latest update now?");
                    ui.label(
                        "Rustic will close and reopen after updating, restoring your projects. Unsaved editors will ask to save first.",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Install update").clicked() {
                            install = true;
                            self.notification.prompt_open = false;
                        }
                        if ui.button("Not now").clicked() {
                            self.notification.prompt_open = false;
                        }
                    });
                });
            self.notification.prompt_open &= open;
        }
        if install {
            self.notification.prompt_open = false;
        }
        install || self.install_requested
    }

    fn poll(&mut self, context: &egui::Context) {
        context.request_repaint_after(Duration::from_secs(1));
        if self
            .last_poll
            .is_some_and(|last| last.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        self.last_poll = Some(Instant::now());
        let Some(client) = &self.client else { return };
        match client.pending() {
            Ok(pending) => {
                let focused = context.input(|input| input.viewport().focused.unwrap_or(false));
                if let Err(error) = self
                    .notification
                    .refresh(pending, focused, |update| client.claim_prompt(update))
                {
                    self.error = Some(error.to_string());
                }
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        let requested = self
            .notification
            .pending
            .as_ref()
            .is_some_and(|update| client.install_requested(update));
        self.install_cancelled |= self.install_requested && !requested;
        self.install_requested = requested;
    }

    /// Coordinates graceful closure in other windows after the current editor is safe.
    pub fn request_install(&mut self, project: Option<&std::path::Path>) -> bool {
        let Some((client, update)) = self.client.as_ref().zip(self.notification.pending.as_ref())
        else {
            return false;
        };
        match client
            .prepare_restart(update, project)
            .and_then(|()| client.request_install(update))
        {
            Ok(()) => {
                self.install_requested = true;
                true
            }
            Err(error) => {
                self.error = Some(error.to_string());
                false
            }
        }
    }

    /// Leaves the update button available if an editor cancels closing.
    pub fn cancel_install(&mut self) {
        self.install_requested = false;
        if let Some(client) = &self.client
            && let Err(error) = client.cancel_install()
        {
            self.error = Some(error.to_string());
        }
    }

    /// Reports a cancellation from another editor once, so deferred closes stop too.
    pub fn take_cancellation(&mut self) -> bool {
        std::mem::take(&mut self.install_cancelled)
    }

    /// Last coordination error, suitable for the application's diagnostics.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

fn title_bar(ui: &mut egui::Ui, title: &str, update_available: bool) -> bool {
    draw_title_bar(ui, title, update_available, cfg!(target_os = "windows")).0
}

fn draw_title_bar(
    ui: &mut egui::Ui,
    title: &str,
    update_available: bool,
    custom: bool,
) -> (bool, Option<egui::Rect>) {
    if !custom && !update_available {
        return (false, None);
    }
    icons::install_loaders(ui.ctx());
    let context = ui.ctx().clone();
    let mut update = false;
    let mut update_rect = None;
    egui::Panel::top("rustic-title-bar")
        .exact_size(24.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::ZERO),
        )
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let controls_width = if custom { 159.0 } else { 0.0 };
            let update_width = if update_available { 56.0 } else { 0.0 };
            let drag_rect = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(
                    (rect.right() - controls_width - update_width).max(rect.left()),
                    rect.bottom(),
                ),
            );
            if custom {
                let drag = ui.interact(
                    drag_rect,
                    ui.id().with("window-drag"),
                    Sense::click_and_drag(),
                );
                if drag.double_clicked() {
                    toggle_maximized(&context);
                } else if drag.drag_started() {
                    context.send_viewport_cmd(ViewportCommand::StartDrag);
                }
                ui.painter().with_clip_rect(drag_rect).text(
                    drag_rect.left_center() + egui::vec2(8.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    title,
                    egui::FontId::proportional(12.0),
                    ui.visuals().text_color(),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.spacing_mut().button_padding = egui::vec2(5.0, 0.0);
                if custom {
                    if caption_button(ui, icons::Icon::X, "Close").clicked() {
                        context.send_viewport_cmd(ViewportCommand::Close);
                    }
                    if caption_button(ui, icons::Icon::Maximize, "Maximize or restore").clicked() {
                        toggle_maximized(&context);
                    }
                    if caption_button(ui, icons::Icon::Minus, "Minimize").clicked() {
                        context.send_viewport_cmd(ViewportCommand::Minimized(true));
                    }
                }
                if update_available {
                    let response = ui
                        .add_sized(
                            [56.0, 16.0],
                            egui::Button::new(egui::RichText::new("Update").size(11.0)).small(),
                        )
                        .on_hover_text("Close Rustic and install the latest update");
                    update_rect = Some(response.rect);
                    update = response.clicked();
                }
            });
        });
    if custom {
        window_resize_handles(&context);
    }
    (update, update_rect)
}

fn caption_button(ui: &mut egui::Ui, icon: icons::Icon, tooltip: &str) -> egui::Response {
    let response = ui.add_sized(
        [53.0, 24.0],
        egui::Button::image(icon.image(egui::Color32::WHITE, 14.0))
            .small()
            .image_tint_follows_text_color(true)
            .frame(false),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    response.on_hover_text(tooltip)
}

fn toggle_maximized(context: &egui::Context) {
    let maximized = context.input(|input| input.viewport().maximized.unwrap_or(false));
    context.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
}

fn window_resize_handles(context: &egui::Context) {
    use egui::{CursorIcon as C, ResizeDirection as R};
    if context.input(|input| input.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let rect = context.viewport_rect();
    let Some(position) = context.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    let left = position.x < rect.left() + 5.0;
    let right = position.x > rect.right() - 5.0;
    let top = position.y < rect.top() + 5.0;
    let bottom = position.y > rect.bottom() - 5.0;
    let direction = match (left, right, top, bottom) {
        (true, _, true, _) => Some((R::NorthWest, C::ResizeNwSe)),
        (_, true, true, _) => Some((R::NorthEast, C::ResizeNeSw)),
        (true, _, _, true) => Some((R::SouthWest, C::ResizeNeSw)),
        (_, true, _, true) => Some((R::SouthEast, C::ResizeNwSe)),
        (true, _, _, _) => Some((R::West, C::ResizeHorizontal)),
        (_, true, _, _) => Some((R::East, C::ResizeHorizontal)),
        (_, _, true, _) => Some((R::North, C::ResizeVertical)),
        (_, _, _, true) => Some((R::South, C::ResizeVertical)),
        _ => None,
    };
    if let Some((direction, cursor)) = direction {
        context.set_cursor_icon(cursor);
        if context.input(|input| input.pointer.primary_pressed()) {
            context.send_viewport_cmd(ViewportCommand::BeginResize(direction));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_window_leaves_prompt_for_the_focused_editor() {
        let update = Some(DesktopUpdate {
            tag: "v2.1-Pre-Alpha".into(),
        });
        let mut launcher = UpdateNotification::default();
        let mut editor = UpdateNotification::default();
        launcher
            .refresh(update.clone(), false, |_| panic!("background claim"))
            .unwrap();
        assert_eq!(launcher.pending, update);
        assert!(!launcher.prompt_open);
        editor.refresh(update.clone(), true, |_| Ok(true)).unwrap();
        assert!(editor.prompt_open);
        // Repeated polling and focus changes preserve the current prompt.
        editor
            .refresh(update.clone(), false, |_| panic!("duplicate claim"))
            .unwrap();
        assert!(editor.prompt_open);
        editor.prompt_open = false;
        editor
            .refresh(update.clone(), true, |_| panic!("dismissed prompt"))
            .unwrap();
        assert!(!editor.prompt_open);
        // The shared client's claim reports that the editor already notified.
        launcher
            .refresh(update.clone(), true, |_| Ok(false))
            .unwrap();
        assert!(!launcher.prompt_open);
        assert_eq!(launcher.pending, update);
        let newer = Some(DesktopUpdate {
            tag: "v2.2-Pre-Alpha".into(),
        });
        launcher
            .refresh(newer.clone(), false, |_| panic!("background claim"))
            .unwrap();
        launcher.refresh(newer, true, |_| Ok(true)).unwrap();
        assert!(launcher.prompt_open);
    }

    #[test]
    fn failed_prompt_claim_can_retry_and_removed_updates_close_the_prompt() {
        let update = Some(DesktopUpdate {
            tag: "v2.1-Pre-Alpha".into(),
        });
        let mut notification = UpdateNotification::default();
        assert!(
            notification
                .refresh(update.clone(), true, |_| Err(std::io::Error::other("busy")))
                .is_err()
        );
        notification.refresh(update, true, |_| Ok(true)).unwrap();
        assert!(notification.prompt_open);
        notification
            .refresh(None, true, |_| panic!("no pending update"))
            .unwrap();
        assert!(!notification.prompt_open);
        assert!(notification.pending.is_none());
    }

    fn frame(context: &egui::Context, input: egui::RawInput, ui: impl FnMut(&mut egui::Ui)) {
        let mut output = context.run_ui(input, ui);
        output.textures_delta.clear();
    }

    #[test]
    fn update_button_stays_immediately_left_of_caption_controls_when_resized() {
        let context = egui::Context::default();
        context.set_theme(egui::Theme::Dark);
        context.style_mut_of(egui::Theme::Dark, |style| {
            style.spacing.interact_size.y = 30.0;
            style.spacing.button_padding = egui::vec2(12.0, 6.0);
        });
        for width in [1280.0, 1080.0, 760.0] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 700.0),
                )),
                ..Default::default()
            };
            let mut button = None;
            frame(&context, input, |ui| {
                button = draw_title_bar(ui, "Rustic Editor", true, true).1;
            });
            let button = button.expect("ready updates must have a title-bar action");
            assert!((button.right() - (width - 159.0)).abs() < 1.0);
            assert!((button.width() - 56.0).abs() < 1.0);
            assert!((button.height() - 16.0).abs() < 1.0);
            assert!((button.center().y - 12.0).abs() < 1.0);
        }
    }

    #[test]
    fn update_action_is_hidden_until_ready_and_clickable_after_prompt_dismissal() {
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 700.0));
        let mut button = None;
        frame(
            &context,
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                assert!(draw_title_bar(ui, "Rustic Editor", false, true).1.is_none());
            },
        );
        frame(
            &context,
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                button = draw_title_bar(ui, "Rustic Editor", true, true).1;
            },
        );
        let position = button.unwrap().center();
        let events = vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        let mut clicked = false;
        frame(
            &context,
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                clicked = draw_title_bar(ui, "Rustic Editor", true, true).0;
            },
        );
        assert!(
            clicked,
            "the persistent Update button must still start the update flow"
        );
    }
}
