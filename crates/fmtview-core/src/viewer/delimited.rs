mod render;

use std::time::Duration;

use anyhow::Result;
use ratatui::layout::Size;

use crate::{
    load::DelimitedDataset,
    tui::screen::{RenderFrame, ScrollPosition},
    viewer::{InputEvent, KeyCode, KeyModifiers, MouseEventKind, ViewerAction},
};

use render::{CellDisplay, CellMatch};

const PRELOAD_BUDGET: Duration = Duration::from_millis(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Fields,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptKind {
    Field,
    Value,
}

struct Prompt {
    kind: PromptKind,
    buffer: String,
    original_field: usize,
}

pub struct DelimitedViewer {
    dataset: DelimitedDataset,
    field: usize,
    focus: Focus,
    prompt: Option<Prompt>,
    value_query: String,
    value_search_match: Option<CellMatch>,
    value_scroll: usize,
    last_total_rows: usize,
    last_value_height: usize,
    last_value_width: usize,
    cell: CellDisplay,
    notice: Option<String>,
}

impl DelimitedViewer {
    pub fn new(dataset: DelimitedDataset) -> Self {
        let cell = CellDisplay::new(dataset.current_value(0));
        Self {
            dataset,
            field: 0,
            focus: Focus::Fields,
            prompt: None,
            value_query: String::new(),
            value_search_match: None,
            value_scroll: 0,
            last_total_rows: 1,
            last_value_height: 1,
            last_value_width: 1,
            cell,
            notice: None,
        }
    }

    pub fn handle_event(&mut self, event: InputEvent, page: usize) -> ViewerAction {
        let result = match event {
            InputEvent::Key { code, modifiers } => self.handle_key(code, modifiers, page),
            InputEvent::Mouse { kind, .. } => self.handle_mouse(kind),
            InputEvent::Resize => KeyResult::Dirty,
            InputEvent::Command(_) | InputEvent::Ignore => KeyResult::Clean,
        };
        match result {
            KeyResult::Dirty => ViewerAction {
                dirty: true,
                ..ViewerAction::default()
            },
            KeyResult::Quit => ViewerAction {
                quit: true,
                ..ViewerAction::default()
            },
            KeyResult::Clean => ViewerAction::default(),
        }
    }

    pub fn render(&mut self, size: Size) -> RenderFrame {
        let rendered = render::render(self, size);
        self.last_total_rows = rendered.total_value_rows;
        self.last_value_height = rendered.value_height;
        self.last_value_width = rendered.value_width;
        self.value_scroll = self
            .value_scroll
            .min(self.last_total_rows.saturating_sub(self.last_value_height));
        RenderFrame {
            area: ratatui::layout::Rect::new(0, 0, size.width, size.height),
            styled: rendered.lines,
            sticky: Vec::new(),
            selection_mode: false,
            title: rendered.title,
            footer_text: rendered.footer,
            footer_style: crate::tui::palette::gutter_style(),
            position: ScrollPosition {
                top: self.dataset.current_index().unwrap_or(0),
                row_offset: self.value_scroll,
            },
            scroll_hint: None,
        }
    }

    pub fn preload(&mut self) -> Result<bool> {
        self.dataset.preload(PRELOAD_BUDGET)
    }

    pub const fn page_for_size(size: Size) -> usize {
        size.height.saturating_sub(4) as usize
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers, page: usize) -> KeyResult {
        if matches!(code, KeyCode::Char('c')) && modifiers.contains(KeyModifiers::CONTROL) {
            return KeyResult::Quit;
        }
        if self.prompt.is_some() {
            return self.handle_prompt_key(code, modifiers);
        }
        match code {
            KeyCode::Char('q') => KeyResult::Quit,
            KeyCode::Esc if self.focus == Focus::Value => {
                self.focus = Focus::Fields;
                KeyResult::Dirty
            }
            KeyCode::Esc => KeyResult::Quit,
            KeyCode::Enter if self.focus == Focus::Fields => {
                self.focus = Focus::Value;
                KeyResult::Dirty
            }
            KeyCode::Enter => KeyResult::Clean,
            KeyCode::Char('/') => {
                self.prompt = Some(Prompt {
                    kind: if self.focus == Focus::Fields {
                        PromptKind::Field
                    } else {
                        PromptKind::Value
                    },
                    buffer: String::new(),
                    original_field: self.field,
                });
                KeyResult::Dirty
            }
            KeyCode::Char('n') if self.focus == Focus::Value => self.repeat_value_search(true),
            KeyCode::Char('N') if self.focus == Focus::Value => self.repeat_value_search(false),
            KeyCode::Up | KeyCode::Char('k') if self.focus == Focus::Fields => self.move_field(-1),
            KeyCode::Down | KeyCode::Char('j') if self.focus == Focus::Fields => self.move_field(1),
            KeyCode::Left | KeyCode::Char('h') if self.focus == Focus::Fields => {
                self.move_record(-1)
            }
            KeyCode::Right | KeyCode::Char('l') if self.focus == Focus::Fields => {
                self.move_record(1)
            }
            KeyCode::PageUp if self.focus == Focus::Fields => self.move_record(-(page as isize)),
            KeyCode::PageDown if self.focus == Focus::Fields => self.move_record(page as isize),
            KeyCode::Home if self.focus == Focus::Fields => self.set_field(0),
            KeyCode::End if self.focus == Focus::Fields => {
                self.set_field(self.dataset.headers().len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') if self.focus == Focus::Value => self.scroll_value(-1),
            KeyCode::Down | KeyCode::Char('j') if self.focus == Focus::Value => {
                self.scroll_value(1)
            }
            KeyCode::PageUp if self.focus == Focus::Value => self.scroll_value(-(page as isize)),
            KeyCode::PageDown if self.focus == Focus::Value => self.scroll_value(page as isize),
            KeyCode::Home if self.focus == Focus::Value => {
                self.value_scroll = 0;
                KeyResult::Dirty
            }
            KeyCode::End if self.focus == Focus::Value => {
                self.value_scroll = self.last_total_rows.saturating_sub(self.last_value_height);
                KeyResult::Dirty
            }
            _ => KeyResult::Clean,
        }
    }

    fn handle_prompt_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> KeyResult {
        let Some(prompt) = self.prompt.as_mut() else {
            return KeyResult::Clean;
        };
        match code {
            KeyCode::Esc => {
                let original = prompt.original_field;
                let kind = prompt.kind;
                self.prompt = None;
                if kind == PromptKind::Field {
                    let _ = self.set_field(original);
                    KeyResult::Dirty
                } else {
                    KeyResult::Dirty
                }
            }
            KeyCode::Enter => {
                let prompt = self.prompt.take().expect("prompt was present");
                if prompt.kind == PromptKind::Value {
                    self.value_query = prompt.buffer;
                    self.value_search_match = None;
                    self.repeat_value_search(true)
                } else {
                    KeyResult::Dirty
                }
            }
            KeyCode::Backspace => {
                prompt.buffer.pop();
                if prompt.kind == PromptKind::Field {
                    self.select_first_field_match();
                }
                KeyResult::Dirty
            }
            KeyCode::Char(ch)
                if !modifiers.contains(KeyModifiers::CONTROL)
                    && !modifiers.contains(KeyModifiers::ALT) =>
            {
                prompt.buffer.push(ch);
                if prompt.kind == PromptKind::Field {
                    self.select_first_field_match();
                }
                KeyResult::Dirty
            }
            _ => KeyResult::Clean,
        }
    }

    fn handle_mouse(&mut self, kind: MouseEventKind) -> KeyResult {
        match kind {
            MouseEventKind::ScrollUp if self.focus == Focus::Value => self.scroll_value(-3),
            MouseEventKind::ScrollDown if self.focus == Focus::Value => self.scroll_value(3),
            MouseEventKind::ScrollUp => self.move_field(-1),
            MouseEventKind::ScrollDown => self.move_field(1),
            _ => KeyResult::Clean,
        }
    }

    fn move_field(&mut self, delta: isize) -> KeyResult {
        if self.dataset.headers().is_empty() {
            return KeyResult::Clean;
        }
        let target = if delta >= 0 {
            self.field.saturating_add(delta as usize)
        } else {
            self.field.saturating_sub(delta.unsigned_abs())
        }
        .min(self.dataset.headers().len() - 1);
        self.set_field(target)
    }

    fn set_field(&mut self, field: usize) -> KeyResult {
        if self.dataset.headers().is_empty() || self.field == field {
            return KeyResult::Clean;
        }
        self.field = field.min(self.dataset.headers().len() - 1);
        self.refresh_cell();
        KeyResult::Dirty
    }

    fn move_record(&mut self, delta: isize) -> KeyResult {
        match self.dataset.move_record(delta) {
            Ok(true) => {
                self.notice = None;
                self.field = self
                    .field
                    .min(self.dataset.headers().len().saturating_sub(1));
                self.refresh_cell();
                KeyResult::Dirty
            }
            Ok(false) => KeyResult::Clean,
            Err(error) => {
                self.notice = Some(format!("record navigation failed: {error:#}"));
                KeyResult::Dirty
            }
        }
    }

    fn scroll_value(&mut self, delta: isize) -> KeyResult {
        let max = self.last_total_rows.saturating_sub(self.last_value_height);
        let next = if delta >= 0 {
            self.value_scroll.saturating_add(delta as usize).min(max)
        } else {
            self.value_scroll.saturating_sub(delta.unsigned_abs())
        };
        if next == self.value_scroll {
            return KeyResult::Clean;
        }
        self.value_scroll = next;
        KeyResult::Dirty
    }

    fn select_first_field_match(&mut self) {
        let Some(prompt) = self.prompt.as_ref() else {
            return;
        };
        let query = prompt.buffer.to_lowercase();
        if query.is_empty() {
            return;
        }
        if let Some(index) = self
            .dataset
            .headers()
            .iter()
            .position(|header| header.to_lowercase().contains(&query))
            && index != self.field
        {
            self.field = index;
            self.refresh_cell();
        }
    }

    fn repeat_value_search(&mut self, forward: bool) -> KeyResult {
        if self.value_query.is_empty() {
            return KeyResult::Clean;
        }
        let found = self
            .cell
            .find_match(&self.value_query, self.value_search_match, forward);
        let Some(found) = found else {
            self.notice = Some(format!("not found: {}", self.value_query));
            return KeyResult::Dirty;
        };
        let scroll = self.cell.visual_offset_for_match(found);
        if self.value_search_match == Some(found) && self.value_scroll == scroll {
            return KeyResult::Clean;
        }
        self.notice = None;
        self.value_search_match = Some(found);
        self.value_scroll = scroll;
        KeyResult::Dirty
    }

    fn refresh_cell(&mut self) {
        self.cell = CellDisplay::new(self.dataset.current_value(self.field));
        self.cell.ensure_layout(self.last_value_width);
        self.value_scroll = 0;
        self.last_total_rows = 1;
        self.value_query.clear();
        self.value_search_match = None;
        self.notice = None;
    }

    fn visible_fields(&self, height: usize) -> Vec<usize> {
        if height == 0 || self.dataset.headers().is_empty() {
            return Vec::new();
        }
        let query = self
            .prompt
            .as_ref()
            .filter(|prompt| prompt.kind == PromptKind::Field)
            .map(|prompt| prompt.buffer.to_lowercase())
            .unwrap_or_default();
        if query.is_empty() {
            let start = self
                .field
                .saturating_sub(height / 2)
                .min(self.dataset.headers().len().saturating_sub(height));
            return (start..self.dataset.headers().len()).take(height).collect();
        }
        let mut match_count = 0_usize;
        let mut selected = None;
        for (index, header) in self.dataset.headers().iter().enumerate() {
            if header.to_lowercase().contains(&query) {
                if index == self.field {
                    selected = Some(match_count);
                }
                match_count += 1;
            }
        }
        let start = selected
            .unwrap_or(0)
            .saturating_sub(height / 2)
            .min(match_count.saturating_sub(height));
        self.dataset
            .headers()
            .iter()
            .enumerate()
            .filter(|(_, header)| header.to_lowercase().contains(&query))
            .map(|(index, _)| index)
            .skip(start)
            .take(height)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyResult {
    Clean,
    Dirty,
    Quit,
}
