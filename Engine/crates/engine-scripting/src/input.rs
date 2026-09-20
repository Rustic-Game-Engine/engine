use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "device")]
pub enum ActionBinding {
    Keyboard { key: String, scale_milli: i16 },
    MouseButton { button: u8, scale_milli: i16 },
    MouseAxisX { scale_milli: i16 },
    MouseAxisY { scale_milli: i16 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InputAction {
    pub name: String,
    pub bindings: Vec<ActionBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActionMap {
    pub format_version: u32,
    pub actions: Vec<InputAction>,
}

impl Default for ActionMap {
    fn default() -> Self {
        Self {
            format_version: 1,
            actions: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ActionState {
    pub pressed: bool,
    pub released: bool,
    pub held: bool,
    pub axis: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputFrame {
    pub keys: BTreeSet<String>,
    /// Keys pressed since the previous frame (auto-repeat is excluded).
    pub keys_pressed: BTreeSet<String>,
    /// Keys released since the previous frame.
    pub keys_released: BTreeSet<String>,
    /// Ordered keyboard events since the previous frame.
    pub key_events: Vec<KeyEvent>,
    pub mouse_buttons: BTreeSet<u8>,
    pub mouse_delta: [f32; 2],
}

/// A backend-independent keyboard event. `key` uses W3C/winit physical names such
/// as `KeyW`, `Digit1`, `ArrowLeft`, `Escape`, and `F12`; unknown platform keys are
/// preserved as strings instead of being discarded.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyEvent {
    pub key: String,
    pub state: KeyEventState,
    pub repeat: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyEventState {
    Pressed,
    Released,
}

impl InputFrame {
    pub fn key(&self, key: &str) -> ActionState {
        ActionState {
            pressed: self.keys_pressed.contains(key),
            released: self.keys_released.contains(key),
            held: self.keys.contains(key),
            axis: f32::from(self.keys.contains(key)),
        }
    }

    pub fn any_key_pressed(&self) -> bool {
        !self.keys_pressed.is_empty()
    }

    pub fn any_key_released(&self) -> bool {
        !self.keys_released.is_empty()
    }
}

/// Thread-safe-shell-friendly accumulator. Feed every OS key event into this and
/// call `take_frame` once per game frame.
#[derive(Clone, Debug, Default)]
pub struct KeyboardInput {
    held: BTreeSet<String>,
    pressed: BTreeSet<String>,
    released: BTreeSet<String>,
    events: Vec<KeyEvent>,
}

impl KeyboardInput {
    pub fn set_key(&mut self, key: impl Into<String>, down: bool, repeat: bool) {
        let key = key.into();
        if down {
            if !repeat && self.held.insert(key.clone()) {
                self.pressed.insert(key.clone());
            }
        } else {
            self.held.remove(&key);
            self.released.insert(key.clone());
        }
        self.events.push(KeyEvent {
            key,
            state: if down {
                KeyEventState::Pressed
            } else {
                KeyEventState::Released
            },
            repeat,
        });
    }

    pub fn focus_lost(&mut self) {
        for key in std::mem::take(&mut self.held) {
            self.released.insert(key.clone());
            self.events.push(KeyEvent {
                key,
                state: KeyEventState::Released,
                repeat: false,
            });
        }
    }

    pub fn take_frame(&mut self) -> InputFrame {
        InputFrame {
            keys: self.held.clone(),
            keys_pressed: std::mem::take(&mut self.pressed),
            keys_released: std::mem::take(&mut self.released),
            key_events: std::mem::take(&mut self.events),
            ..InputFrame::default()
        }
    }
}

/// Runtime-only, deterministic action sampling. Physical events are translated by the shell.
#[derive(Clone, Debug)]
pub struct InputSystem {
    map: ActionMap,
    previous: BTreeMap<String, bool>,
    fixed: BTreeMap<String, ActionState>,
}

impl InputSystem {
    /// Validates and creates an action sampler.
    ///
    /// # Errors
    /// Returns an error for unsupported versions or duplicate/empty action names.
    pub fn new(map: ActionMap) -> Result<Self, String> {
        if map.format_version != 1 {
            return Err(format!(
                "unsupported action-map version {}",
                map.format_version
            ));
        }
        let mut names = BTreeSet::new();
        for a in &map.actions {
            if a.name.trim().is_empty() || !names.insert(a.name.clone()) {
                return Err(format!("invalid or duplicate input action `{}`", a.name));
            }
        }
        Ok(Self {
            map,
            previous: BTreeMap::new(),
            fixed: BTreeMap::new(),
        })
    }
    pub fn sample_fixed(&mut self, frame: &InputFrame) {
        let mut next = BTreeMap::new();
        for action in &self.map.actions {
            let mut axis = 0.0;
            for binding in &action.bindings {
                let (active, amount) = match binding {
                    ActionBinding::Keyboard { key, scale_milli } => {
                        (frame.keys.contains(key), f32::from(*scale_milli) / 1000.0)
                    }
                    ActionBinding::MouseButton {
                        button,
                        scale_milli,
                    } => (
                        frame.mouse_buttons.contains(button),
                        f32::from(*scale_milli) / 1000.0,
                    ),
                    ActionBinding::MouseAxisX { scale_milli } => (
                        frame.mouse_delta[0] != 0.0,
                        frame.mouse_delta[0] * (f32::from(*scale_milli) / 1000.0),
                    ),
                    ActionBinding::MouseAxisY { scale_milli } => (
                        frame.mouse_delta[1] != 0.0,
                        frame.mouse_delta[1] * (f32::from(*scale_milli) / 1000.0),
                    ),
                };
                if active {
                    axis += amount;
                }
            }
            let held = axis != 0.0;
            let previous = self.previous.get(&action.name).copied().unwrap_or(false);
            next.insert(
                action.name.clone(),
                ActionState {
                    pressed: held && !previous,
                    released: !held && previous,
                    held,
                    axis,
                },
            );
            self.previous.insert(action.name.clone(), held);
        }
        self.fixed = next;
    }
    pub fn action(&self, name: &str) -> ActionState {
        self.fixed.get(name).copied().unwrap_or_default()
    }
    pub fn focus_lost(&mut self) {
        let names: Vec<_> = self.previous.keys().cloned().collect();
        self.fixed.clear();
        for name in names {
            let was = self.previous.insert(name.clone(), false).unwrap_or(false);
            self.fixed.insert(
                name,
                ActionState {
                    released: was,
                    ..ActionState::default()
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn system() -> InputSystem {
        InputSystem::new(ActionMap {
            format_version: 1,
            actions: vec![InputAction {
                name: "move".into(),
                bindings: vec![ActionBinding::Keyboard {
                    key: "KeyW".into(),
                    scale_milli: 1000,
                }],
            }],
        })
        .unwrap()
    }
    #[test]
    fn pressed_held_released_and_focus_loss() {
        let mut s = system();
        let mut f = InputFrame::default();
        f.keys.insert("KeyW".into());
        s.sample_fixed(&f);
        assert!(s.action("move").pressed);
        s.sample_fixed(&f);
        assert!(s.action("move").held && !s.action("move").pressed);
        s.focus_lost();
        assert!(s.action("move").released);
    }

    #[test]
    fn raw_keyboard_state_and_any_key_events_are_frame_scoped() {
        let mut input = KeyboardInput::default();
        input.set_key("KeyA", true, false);
        let first = input.take_frame();
        assert!(first.key("KeyA").pressed && first.key("KeyA").held);
        assert!(first.any_key_pressed());
        assert_eq!(first.key_events.len(), 1);
        let second = input.take_frame();
        assert!(!second.key("KeyA").pressed && second.key("KeyA").held);
        input.set_key("KeyA", false, false);
        assert!(input.take_frame().key("KeyA").released);
    }
}
