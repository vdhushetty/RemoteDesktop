pub mod platform;

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("failed to initialize input: {0}")]
    Init(String),
    #[error("failed to inject input: {0}")]
    Inject(String),
    #[error("platform not supported")]
    Unsupported,
}

/// Mouse button types
#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Button press/release action
#[derive(Debug, Clone, Copy)]
pub enum ButtonAction {
    Press,
    Release,
}

/// Trait for injecting input events on the remote machine
pub trait InputInjector {
    /// Move mouse to absolute position (in screen pixels)
    fn mouse_move(&mut self, x: i32, y: i32) -> Result<(), InputError>;

    /// Press or release a mouse button
    fn mouse_button(
        &mut self,
        button: MouseButton,
        action: ButtonAction,
    ) -> Result<(), InputError>;

    /// Scroll the mouse wheel
    fn mouse_scroll(&mut self, delta_x: f64, delta_y: f64) -> Result<(), InputError>;

    /// Press or release a key
    fn key_event(
        &mut self,
        keycode: u32,
        action: ButtonAction,
    ) -> Result<(), InputError>;
}

/// Create an input injector for the current platform
pub fn create_injector() -> Result<Box<dyn InputInjector>, InputError> {
    platform::create_platform_injector()
}
