//! Collect real touch events and primary mouse input only from the game image.
use eframe::egui;
use engine_play::pointer_input::{PointerDevice, PointerEvent, PointerPhase};
use std::collections::{BTreeMap, HashSet};

#[derive(Default)]
pub struct PointerCapture {
    active: BTreeMap<(PointerDevice, String), (PointerEvent, egui::Id)>,
    frame: Option<u64>,
    seen_views: HashSet<egui::Id>,
}

pub fn game_image_rect(rect: egui::Rect) -> egui::Rect {
    let aspect = 16.0 / 9.0;
    let size = if rect.width() / rect.height() > aspect {
        egui::vec2(rect.height() * aspect, rect.height())
    } else {
        egui::vec2(rect.width(), rect.width() / aspect)
    };
    egui::Rect::from_center_size(rect.center(), size)
}

impl PointerCapture {
    pub fn clear(&mut self) {
        self.active.clear();
    }

    pub fn collect(&mut self, ui: &egui::Ui, rect: egui::Rect) -> Vec<PointerEvent> {
        let frame = ui.ctx().cumulative_frame_nr();
        if self.frame != Some(frame) {
            self.frame = Some(frame);
            self.seen_views.clear();
        }
        if !self.seen_views.insert(ui.id()) {
            return Vec::new();
        }
        let focused = ui.input(|i| i.focused);
        if !focused || rect.width() <= 0.0 || rect.height() <= 0.0 {
            return self.cancel();
        }
        let raw = ui.input(|i| i.events.clone());
        // egui emits a synthetic pointer alongside touch; never count that as a second click.
        let touch = self.active.keys().any(|(d, _)| *d == PointerDevice::Touch)
            || raw.iter().any(|e| matches!(e, egui::Event::Touch { .. }));
        let mut output = Vec::new();
        if touch {
            self.active.retain(|(device, _), (event, _)| {
                if *device == PointerDevice::Mouse {
                    let mut event = event.clone();
                    event.phase = PointerPhase::Cancel;
                    output.push(event);
                    false
                } else {
                    true
                }
            });
        }
        for event in raw {
            if matches!(event, egui::Event::PointerGone) && !touch {
                output.extend(self.cancel());
                continue;
            }
            let Some((event, pos)) = pointer_event(&event, touch, rect) else {
                continue;
            };
            let phase = event.phase;
            let key = (event.device, event.id.clone());
            if phase == PointerPhase::Start {
                if self.active.contains_key(&key)
                    || !rect.contains(pos)
                    || ui.ctx().layer_id_at(pos) != Some(ui.layer_id())
                {
                    continue;
                }
            } else if !self
                .active
                .get(&key)
                .is_some_and(|(_, view)| *view == ui.id())
            {
                continue;
            }
            if matches!(phase, PointerPhase::End | PointerPhase::Cancel) {
                self.active.remove(&key);
            } else {
                self.active.insert(key, (event.clone(), ui.id()));
            }
            output.push(event);
        }
        output
    }

    fn cancel(&mut self) -> Vec<PointerEvent> {
        std::mem::take(&mut self.active)
            .into_values()
            .map(|(mut e, _)| {
                e.phase = PointerPhase::Cancel;
                e
            })
            .collect()
    }
}

fn pointer_event(
    event: &egui::Event,
    touch: bool,
    rect: egui::Rect,
) -> Option<(PointerEvent, egui::Pos2)> {
    let (id, device, phase, pos, pressure) = match event {
        egui::Event::Touch {
            device_id,
            id,
            phase,
            pos,
            force,
        } => (
            format!("{}:{}", device_id.0, id.0),
            PointerDevice::Touch,
            match phase {
                egui::TouchPhase::Start => PointerPhase::Start,
                egui::TouchPhase::Move => PointerPhase::Move,
                egui::TouchPhase::End => PointerPhase::End,
                egui::TouchPhase::Cancel => PointerPhase::Cancel,
            },
            pos,
            force.unwrap_or(0.0),
        ),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            ..
        } if !touch => (
            "mouse".into(),
            PointerDevice::Mouse,
            if *pressed {
                PointerPhase::Start
            } else {
                PointerPhase::End
            },
            pos,
            if *pressed { 1.0 } else { 0.0 },
        ),
        egui::Event::PointerMoved(pos) if !touch => (
            "mouse".into(),
            PointerDevice::Mouse,
            PointerPhase::Move,
            pos,
            1.0,
        ),
        _ => return None,
    };
    let event = PointerEvent {
        id,
        device,
        phase,
        position: [
            (pos.x - rect.left()) / rect.width(),
            (pos.y - rect.top()) / rect.height(),
        ],
        pressure: pressure.clamp(0.0, 1.0),
        aspect: rect.width() / rect.height(),
    };
    Some((event, *pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        context: &egui::Context,
        capture: &mut PointerCapture,
        events: Vec<egui::Event>,
        focused: bool,
    ) -> Vec<PointerEvent> {
        let mut output = Vec::new();
        let mut frame_output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                focused,
                events,
                ..Default::default()
            },
            |ui| {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(400.0, 225.0));
                ui.allocate_rect(rect, egui::Sense::click_and_drag());
                output.extend(capture.collect(ui, rect));
            },
        );
        frame_output.textures_delta.clear();
        output
    }
    fn touch(id: u64, phase: egui::TouchPhase, pos: egui::Pos2) -> egui::Event {
        egui::Event::Touch {
            device_id: egui::TouchDeviceId(7),
            id: egui::TouchId(id),
            phase,
            pos,
            force: Some(0.6),
        }
    }
    fn mouse(pressed: bool, pos: egui::Pos2) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    #[test]
    fn touchscreen_does_not_duplicate_mouse_and_cancels_on_focus_loss() {
        let context = egui::Context::default();
        let mut capture = PointerCapture::default();
        frame(&context, &mut capture, vec![], true);
        let pos = egui::pos2(300.0, 212.5);
        let output = frame(
            &context,
            &mut capture,
            vec![
                touch(1, egui::TouchPhase::Start, pos),
                mouse(true, pos),
                touch(2, egui::TouchPhase::Start, pos),
            ],
            true,
        );
        assert_eq!(output.len(), 2);
        assert!(output.iter().all(|e| e.device == PointerDevice::Touch));
        assert!(output[0].position.iter().all(|v| (*v - 0.5).abs() < 1e-6));
        assert_ne!(output[0].id, output[1].id);
        let output = frame(&context, &mut capture, vec![], false);
        assert_eq!(output.len(), 2);
        assert!(output.iter().all(|e| e.phase == PointerPhase::Cancel));
        assert!(
            frame(
                &context,
                &mut capture,
                vec![touch(1, egui::TouchPhase::End, pos)],
                true
            )
            .is_empty()
        );
    }
    #[test]
    fn bars_cannot_start_clicks_and_outside_releases_stay_outside() {
        let context = egui::Context::default();
        let mut capture = PointerCapture::default();
        frame(&context, &mut capture, vec![], true);
        assert!(
            frame(
                &context,
                &mut capture,
                vec![
                    mouse(true, egui::pos2(20.0, 20.0)),
                    mouse(false, egui::pos2(300.0, 200.0))
                ],
                true
            )
            .is_empty()
        );
        let output = frame(
            &context,
            &mut capture,
            vec![
                mouse(true, egui::pos2(300.0, 212.5)),
                mouse(false, egui::pos2(600.0, 212.5)),
            ],
            true,
        );
        assert_eq!(output.len(), 2);
        assert!(output[1].position[0] > 1.0);
        let rect = game_image_rect(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        ));
        assert!((rect.height() - 450.0).abs() < 1e-6);
        assert!((rect.top() - 75.0).abs() < 1e-6);
    }
}
