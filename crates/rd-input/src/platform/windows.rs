use crate::{ButtonAction, InputError, InputInjector, MouseButton};
use enigo::{Enigo, Keyboard, Mouse, Settings, Axis, Button, Direction, Coordinate};

pub struct WindowsInjector {
    enigo: Enigo,
}

impl WindowsInjector {
    pub fn new() -> Result<Self, InputError> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| InputError::Init(format!("enigo init: {e}")))?;
        Ok(Self { enigo })
    }
}

impl InputInjector for WindowsInjector {
    fn mouse_move(&mut self, x: i32, y: i32) -> Result<(), InputError> {
        self.enigo
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(|e| InputError::Inject(format!("mouse_move: {e}")))
    }

    fn mouse_button(
        &mut self,
        button: MouseButton,
        action: ButtonAction,
    ) -> Result<(), InputError> {
        let btn = match button {
            MouseButton::Left => Button::Left,
            MouseButton::Right => Button::Right,
            MouseButton::Middle => Button::Middle,
        };
        let dir = match action {
            ButtonAction::Press => Direction::Press,
            ButtonAction::Release => Direction::Release,
        };
        self.enigo
            .button(btn, dir)
            .map_err(|e| InputError::Inject(format!("mouse_button: {e}")))
    }

    fn mouse_scroll(&mut self, _delta_x: f64, delta_y: f64) -> Result<(), InputError> {
        self.enigo
            .scroll(delta_y as i32, Axis::Vertical)
            .map_err(|e| InputError::Inject(format!("scroll: {e}")))
    }

    fn key_event(
        &mut self,
        keycode: u32,
        action: ButtonAction,
    ) -> Result<(), InputError> {
        let key = enigo::Key::Other(keycode);
        let dir = match action {
            ButtonAction::Press => Direction::Press,
            ButtonAction::Release => Direction::Release,
        };
        self.enigo
            .key(key, dir)
            .map_err(|e| InputError::Inject(format!("key_event: {e}")))
    }
}
