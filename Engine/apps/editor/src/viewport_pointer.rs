use eframe::egui;

/// Remember navigation presses inside the canvas independently of the gizmo's
/// cursor-sized interaction widget. Call only when no transform handle owns input.
pub fn dragged(response: &egui::Response, button: egui::PointerButton) -> bool {
    let id = response.id.with(("camera-drag", button as u8));
    let (pressed, down, dragging, focused) = response.ctx.input(|input| {
        (
            input.pointer.button_pressed(button),
            input.pointer.button_down(button),
            input.pointer.is_decidedly_dragging(),
            input.focused,
        )
    });
    let contains_pointer = response.contains_pointer();
    let captured = response.ctx.data_mut(|data| {
        let captured = focused
            && down
            && if pressed {
                contains_pointer
            } else {
                data.get_temp::<bool>(id).unwrap_or(false)
            };
        data.insert_temp(id, captured);
        captured
    });
    captured && dragging
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_drag_stays_captured_until_release_and_must_start_inside() {
        let context = egui::Context::default();
        let frame = |events, focused| {
            let mut dragging = false;
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 400.0),
                    )),
                    focused,
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = ui.allocate_rect(
                        egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(100.0, 100.0)),
                        egui::Sense::click_and_drag(),
                    );
                    dragging = dragged(&response, egui::PointerButton::Secondary);
                },
            );
            output.textures_delta.clear();
            dragging
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let inside = egui::pos2(30.0, 30.0);
        let outside = egui::pos2(200.0, 200.0);
        frame(vec![egui::Event::PointerMoved(inside)], true);
        frame(vec![], true);
        assert!(!frame(vec![button(inside, true)], true));
        assert!(frame(vec![egui::Event::PointerMoved(outside)], true));
        assert!(!frame(vec![button(outside, false)], true));
        assert!(!frame(vec![button(outside, true)], true));
        assert!(!frame(vec![egui::Event::PointerMoved(inside)], true));
        frame(vec![button(inside, false)], true);
        frame(vec![button(inside, true)], true);
        assert!(frame(vec![egui::Event::PointerMoved(outside)], true));
        assert!(!frame(vec![], false));
    }
}
