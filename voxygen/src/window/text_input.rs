use crate::ui;
use winit::{
    dpi::{LogicalPosition, LogicalSize},
    event::{Ime, KeyEvent},
    keyboard::ModifiersState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextInputSource {
    Conrod,
    Iced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextInputPolicy {
    OpenText,
    StructuredAscii,
    NumericOnly,
    SecureText,
}

impl TextInputPolicy {
    pub fn ime_allowed(self) -> bool { matches!(self, Self::OpenText) }

    pub fn filter_text(self, text: &str) -> String {
        match self {
            Self::OpenText => text.chars().filter(|c| !c.is_control()).collect(),
            Self::StructuredAscii | Self::SecureText => text
                .chars()
                .filter(|c| c.is_ascii() && !c.is_control())
                .collect(),
            Self::NumericOnly => text.chars().filter(|c| c.is_ascii_digit()).collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextCursorRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl TextCursorRect {
    pub fn position(self) -> LogicalPosition<f64> { LogicalPosition::new(self.x, self.y) }

    pub fn size(self) -> LogicalSize<f64> { LogicalSize::new(self.width, self.height) }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextInputTarget {
    pub source: TextInputSource,
    pub policy: TextInputPolicy,
    pub cursor_rect: Option<TextCursorRect>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputEvent {
    InsertText(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreeditState {
    pub text: String,
    pub cursor: Option<(usize, usize)>,
}

#[derive(Debug, Default)]
pub struct TextInputManager {
    active_target: Option<TextInputTarget>,
    ime_allowed: bool,
    ime_composing: bool,
    preedit: Option<PreeditState>,
}

impl TextInputManager {
    pub fn active_target(&self) -> Option<TextInputTarget> { self.active_target }

    fn clear_composition_state(&mut self) {
        self.ime_composing = false;
        self.preedit = None;
    }

    pub fn update_target(
        &mut self,
        window: &winit::window::Window,
        target: Option<TextInputTarget>,
    ) {
        let target_changed = self.active_target != target;

        if target_changed && self.ime_allowed {
            // Reset the OS IME session when focus moves between fields so an old
            // composition cannot leak into the new target.
            window.set_ime_allowed(false);
            self.ime_allowed = false;
            self.clear_composition_state();
        }

        self.active_target = target;

        let ime_allowed = self
            .active_target
            .is_some_and(|target| target.policy.ime_allowed());
        if self.ime_allowed != ime_allowed {
            self.ime_allowed = ime_allowed;
            window.set_ime_allowed(ime_allowed);
            if !ime_allowed {
                self.clear_composition_state();
            }
        }

        if let Some(rect) = self.active_target.and_then(|target| target.cursor_rect) {
            window.set_ime_cursor_area(rect.position(), rect.size());
        }
    }

    pub fn handle_keyboard_input(
        &mut self,
        event: &KeyEvent,
        modifiers: ModifiersState,
    ) -> Vec<TextInputEvent> {
        if !event.state.is_pressed() || self.ime_composing {
            return Vec::new();
        }

        if modifiers.alt_key() || modifiers.control_key() || modifiers.super_key() {
            return Vec::new();
        }

        let Some(target) = self.active_target else {
            return Vec::new();
        };

        let Some(text) = &event.text else {
            return Vec::new();
        };

        let filtered = target.policy.filter_text(text.as_ref());
        if filtered.is_empty() {
            Vec::new()
        } else {
            vec![TextInputEvent::InsertText(filtered)]
        }
    }

    pub fn handle_ime(&mut self, ime: Ime) -> Vec<TextInputEvent> {
        let Some(target) = self
            .active_target
            .filter(|target| target.policy.ime_allowed())
        else {
            self.clear_composition_state();
            return Vec::new();
        };

        match ime {
            Ime::Enabled => Vec::new(),
            Ime::Preedit(text, cursor) => {
                if text.is_empty() {
                    self.clear_composition_state();
                } else {
                    self.ime_composing = true;
                    self.preedit = Some(PreeditState { text, cursor });
                }
                Vec::new()
            },
            Ime::Commit(text) => {
                let filtered = target.policy.filter_text(&text);
                let events = (!filtered.is_empty())
                    .then_some(vec![TextInputEvent::InsertText(filtered)])
                    .unwrap_or_default();
                self.clear_composition_state();
                events
            },
            Ime::Disabled => {
                self.clear_composition_state();
                Vec::new()
            },
        }
    }

    pub fn dispatch_conrod(event: TextInputEvent) -> Vec<ui::Event> {
        match event {
            TextInputEvent::InsertText(text) => vec![ui::Event::new_text(text)],
        }
    }

    pub fn dispatch_iced(event: TextInputEvent) -> Vec<iced::Event> {
        match event {
            TextInputEvent::InsertText(text) => text
                .chars()
                .map(iced::keyboard::Event::CharacterReceived)
                .map(iced::Event::Keyboard)
                .collect(),
        }
    }
}
