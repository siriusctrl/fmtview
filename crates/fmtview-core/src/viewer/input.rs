/// Backend-neutral key codes understood by the viewer engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
}

/// Modifier flags carried by a backend-neutral input event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1 << 0);
    pub const CONTROL: Self = Self(1 << 1);
    pub const ALT: Self = Self(1 << 2);
    pub const SUPER: Self = Self(1 << 3);
    pub const HYPER: Self = Self(1 << 4);
    pub const META: Self = Self(1 << 5);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Pointer actions understood by the viewer engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    ScrollDown,
    ScrollUp,
    ScrollLeft,
    ScrollRight,
    Other,
}

/// Semantic actions that an embedding can send without synthesizing a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerCommand {
    FollowTail,
    ToggleFollowTail,
}

/// Input after an application backend has translated its native event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Key {
        code: KeyCode,
        modifiers: KeyModifiers,
    },
    Mouse {
        kind: MouseEventKind,
        modifiers: KeyModifiers,
    },
    Resize,
    Command(ViewerCommand),
    Ignore,
}

/// State transition requested by one or more viewer input events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewerAction {
    pub dirty: bool,
    pub quit: bool,
    pub mouse_capture: Option<bool>,
}

impl ViewerAction {
    pub fn merge(&mut self, next: Self) {
        self.dirty |= next.dirty;
        self.quit |= next.quit;
        if next.mouse_capture.is_some() {
            self.mouse_capture = next.mouse_capture;
        }
    }
}
