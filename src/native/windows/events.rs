//! Buffer callbacks while Win32 synchronously re-enters a geometry operation.
use crate::{EventHandler, KeyCode, KeyMods, MouseButton};
use std::collections::VecDeque;

pub(super) enum Event {
    Motion(f32, f32),
    RawMotion(f32, f32),
    Wheel(f32, f32),
    ButtonDown(MouseButton, f32, f32),
    ButtonUp(MouseButton, f32, f32),
    Character(char, KeyMods, bool),
    KeyDown(KeyCode, KeyMods, bool),
    KeyUp(KeyCode, KeyMods),
    Minimized,
    Restored,
    Quit,
}

impl Event {
    pub(super) fn dispatch(self, handler: &mut dyn EventHandler) {
        match self {
            Self::Motion(x, y) => handler.mouse_motion_event(x, y),
            Self::RawMotion(x, y) => handler.raw_mouse_motion(x, y),
            Self::Wheel(x, y) => handler.mouse_wheel_event(x, y),
            Self::ButtonDown(button, x, y) => handler.mouse_button_down_event(button, x, y),
            Self::ButtonUp(button, x, y) => handler.mouse_button_up_event(button, x, y),
            Self::Character(c, mods, repeat) => handler.char_event(c, mods, repeat),
            Self::KeyDown(key, mods, repeat) => handler.key_down_event(key, mods, repeat),
            Self::KeyUp(key, mods) => handler.key_up_event(key, mods),
            Self::Minimized => handler.window_minimized_event(),
            Self::Restored => handler.window_restored_event(),
            Self::Quit => handler.quit_requested_event(),
        }
    }
}

#[derive(Default)]
pub(super) struct PendingEvents(pub(super) VecDeque<Event>);

impl EventHandler for PendingEvents {
    fn update(&mut self) {
        unreachable!()
    }
    fn draw(&mut self) {
        unreachable!()
    }
    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        self.0.push_back(Event::Motion(x, y));
    }
    fn raw_mouse_motion(&mut self, x: f32, y: f32) {
        self.0.push_back(Event::RawMotion(x, y));
    }
    fn mouse_wheel_event(&mut self, x: f32, y: f32) {
        self.0.push_back(Event::Wheel(x, y));
    }
    fn mouse_button_down_event(&mut self, b: MouseButton, x: f32, y: f32) {
        self.0.push_back(Event::ButtonDown(b, x, y));
    }
    fn mouse_button_up_event(&mut self, b: MouseButton, x: f32, y: f32) {
        self.0.push_back(Event::ButtonUp(b, x, y));
    }
    fn char_event(&mut self, c: char, mods: KeyMods, repeat: bool) {
        self.0.push_back(Event::Character(c, mods, repeat));
    }
    fn key_down_event(&mut self, key: KeyCode, mods: KeyMods, repeat: bool) {
        self.0.push_back(Event::KeyDown(key, mods, repeat));
    }
    fn key_up_event(&mut self, key: KeyCode, mods: KeyMods) {
        self.0.push_back(Event::KeyUp(key, mods));
    }
    fn window_minimized_event(&mut self) {
        self.0.push_back(Event::Minimized);
    }
    fn window_restored_event(&mut self) {
        self.0.push_back(Event::Restored);
    }
    fn quit_requested_event(&mut self) {
        self.0.push_back(Event::Quit);
    }
}
