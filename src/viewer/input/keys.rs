use crossterm::event::KeyModifiers;

pub(in crate::viewer) fn accepts_jump_digit(ch: char, modifiers: KeyModifiers) -> bool {
    ch.is_ascii_digit()
        && !modifiers.contains(KeyModifiers::CONTROL)
        && !modifiers.contains(KeyModifiers::ALT)
}

pub(in crate::viewer) fn accepts_search_char(modifiers: KeyModifiers) -> bool {
    !modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::ALT)
}

pub(in crate::viewer) fn plain_key(modifiers: KeyModifiers) -> bool {
    !modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::ALT)
}
