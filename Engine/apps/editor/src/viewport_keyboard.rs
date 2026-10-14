use eframe::egui;

pub fn active(response: &egui::Response) -> bool {
    // egui wants keyboard input for any focused widget, including this viewport.
    // Give the viewport its own keys while respecting focus in other widgets.
    response.ctx.input(|input| input.focused)
        && (response.has_focus()
            || (response.hovered() && !response.ctx.egui_wants_keyboard_input()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    enum Focus {
        None,
        Viewport,
        Text,
    }

    fn keyboard_active(focus: Focus, hovered: bool, window_focused: bool) -> bool {
        let ctx = egui::Context::default();
        let mut active = false;
        let mut text = String::new();
        // Warm up hit testing before checking the hovered response.
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    focused: window_focused,
                    events: vec![egui::Event::PointerMoved(if hovered {
                        egui::pos2(50.0, 50.0)
                    } else {
                        egui::pos2(700.0, 500.0)
                    })],
                    ..Default::default()
                },
                |ui| {
                    let (_, viewport) = ui.allocate_exact_size(
                        egui::vec2(200.0, 200.0),
                        egui::Sense::click_and_drag(),
                    );
                    let field = ui.text_edit_singleline(&mut text);
                    match focus {
                        Focus::None => {}
                        Focus::Viewport => viewport.request_focus(),
                        Focus::Text => field.request_focus(),
                    }
                    active = super::active(&viewport);
                },
            );
            output.textures_delta.clear();
        }
        active
    }

    #[test]
    fn clicking_viewport_does_not_block_its_keyboard_controls() {
        assert!(keyboard_active(Focus::Viewport, true, true));
        assert!(keyboard_active(Focus::Viewport, false, true));
    }

    #[test]
    fn hovering_allows_navigation_without_a_prior_click() {
        assert!(keyboard_active(Focus::None, true, true));
        assert!(!keyboard_active(Focus::None, false, true));
    }

    #[test]
    fn typing_and_window_blur_do_not_drive_the_viewport() {
        assert!(!keyboard_active(Focus::Text, true, true));
        assert!(!keyboard_active(Focus::Viewport, true, false));
        assert!(!keyboard_active(Focus::None, true, false));
    }
}
