use std::{
    io,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::diff::{
    DiffChange, DiffLayout, DiffModel, DiffRange, NumberedDiffLine, SideDiffRow, UnifiedDiffRow,
};

use super::{
    EVENT_DRAIN_BUDGET, EVENT_DRAIN_LIMIT, EVENT_POLL_INTERVAL, MOUSE_HORIZONTAL_COLUMNS,
    MOUSE_SCROLL_LINES, ViewMode,
    highlight::highlight_content_window,
    palette::{
        diff_added_inline_bg, diff_added_line_bg, diff_added_style, diff_removed_inline_bg,
        diff_removed_line_bg, diff_removed_style, gutter_style, plain_style,
    },
    render::{ViewPosition, byte_index_for_char, char_count, format_count},
    terminal::{ScrollHint, TerminalFrame, ViewerTerminal},
};

const SIDE_BY_SIDE_MIN_WIDTH: usize = 110;
const DIFF_SCROLL_HINT_MAX_ROWS: usize = 12;

#[derive(Debug)]
struct DiffViewState {
    top: usize,
    x: usize,
    layout: DiffLayout,
    message: Option<String>,
    change_cursor: Option<usize>,
}

impl DiffViewState {
    fn new(layout: DiffLayout) -> Self {
        Self {
            top: 0,
            x: 0,
            layout,
            message: None,
            change_cursor: None,
        }
    }
}

pub(super) fn run_loop(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    model: &DiffModel,
) -> Result<()> {
    let initial_layout = terminal
        .size()
        .map(|size| initial_layout(size.width))
        .unwrap_or(DiffLayout::Unified);
    let mut state = DiffViewState::new(initial_layout);
    let mut dirty = true;

    loop {
        if dirty {
            draw_view(terminal, model, &mut state)?;
            dirty = false;
        }

        if !event::poll(EVENT_POLL_INTERVAL).context("failed to poll terminal event")? {
            continue;
        }

        let (page, visible_height) = terminal
            .size()
            .map(|size| {
                (
                    usize::from(size.height.saturating_sub(4)).max(1),
                    diff_visible_height(size.height),
                )
            })
            .unwrap_or((20, 20));
        let action = drain_events(model, &mut state, page, visible_height)?;
        if action.quit {
            break;
        }
        dirty |= action.dirty;
    }

    Ok(())
}

fn initial_layout(width: u16) -> DiffLayout {
    if usize::from(width) >= SIDE_BY_SIDE_MIN_WIDTH {
        DiffLayout::SideBySide
    } else {
        DiffLayout::Unified
    }
}

fn diff_visible_height(terminal_height: u16) -> usize {
    usize::from(terminal_height.saturating_sub(3)).max(1)
}

fn draw_view(
    terminal: &mut ViewerTerminal<CrosstermBackend<io::Stdout>>,
    model: &DiffModel,
    state: &mut DiffViewState,
) -> Result<()> {
    let size = terminal.size().context("failed to read terminal size")?;
    let area = Rect::new(0, 0, size.width, size.height);
    let visible_height = diff_visible_height(size.height);
    let content_width = usize::from(size.width.saturating_sub(2));
    clamp_top(state, model, visible_height);

    let styled = render_rows(
        model,
        state.layout,
        state.top,
        visible_height,
        content_width,
        state.x,
    );
    let row_count = model.row_count(state.layout);
    let current = if row_count == 0 { 0 } else { state.top + 1 };
    let bottom = state
        .top
        .saturating_add(styled.len())
        .min(row_count)
        .max(current);
    let progress = progress_percent(bottom, row_count);
    let change_text = if model.has_changes() {
        format!(
            "{} changes",
            format_count(model.changed_rows(state.layout).len())
        )
    } else {
        "no changes".to_owned()
    };
    let title = format!(
        " {} <-> {} | {} rows | {} | {}-{} | {:>3}% | diff {} ",
        model.left_label(),
        model.right_label(),
        format_count(row_count),
        change_text,
        current,
        bottom,
        progress,
        state.layout.label()
    );
    let footer_text = state.message.take().unwrap_or_else(|| {
        " q/Esc quit | s single/split | ]/[ next/prev block | j/k wheel | Space/b | h/l ".to_owned()
    });
    let position = ViewPosition {
        top: state.top,
        row_offset: 0,
    };
    let scroll_hint = diff_scroll_hint(terminal, position);
    terminal
        .draw(TerminalFrame {
            area,
            styled,
            sticky: Vec::new(),
            title,
            footer_text,
            position,
            scroll_hint,
        })
        .context("failed to draw terminal frame")?;
    Ok(())
}

pub(super) fn render_rows(
    model: &DiffModel,
    layout: DiffLayout,
    top: usize,
    height: usize,
    width: usize,
    x: usize,
) -> Vec<Line<'static>> {
    if height == 0 {
        return Vec::new();
    }

    match layout {
        DiffLayout::Unified => model
            .unified_rows()
            .iter()
            .skip(top)
            .take(height)
            .map(|row| render_unified_row(row, model, width, x))
            .collect(),
        DiffLayout::SideBySide => model
            .side_rows()
            .iter()
            .skip(top)
            .take(height)
            .map(|row| render_side_row(row, model, width, x))
            .collect(),
    }
}

fn render_unified_row(
    row: &UnifiedDiffRow,
    model: &DiffModel,
    width: usize,
    x: usize,
) -> Line<'static> {
    match row {
        UnifiedDiffRow::Message { text } => styled_text_line(text, width, plain_style()),
        UnifiedDiffRow::Context {
            left,
            right,
            content,
        } => render_unified_content(
            Some(*left),
            Some(*right),
            ' ',
            gutter_style(),
            content,
            model,
            width,
            x,
            None,
        ),
        UnifiedDiffRow::Delete {
            left,
            content,
            change,
        } => render_unified_content(
            Some(*left),
            None,
            '-',
            diff_removed_style(),
            content,
            model,
            width,
            x,
            Some(DiffCellStyle {
                side: DiffSide::Removed,
                change: *change,
            }),
        ),
        UnifiedDiffRow::Insert {
            right,
            content,
            change,
        } => render_unified_content(
            None,
            Some(*right),
            '+',
            diff_added_style(),
            content,
            model,
            width,
            x,
            Some(DiffCellStyle {
                side: DiffSide::Added,
                change: *change,
            }),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_unified_content(
    left: Option<usize>,
    right: Option<usize>,
    marker: char,
    marker_style: Style,
    content: &str,
    model: &DiffModel,
    width: usize,
    x: usize,
    diff_style: Option<DiffCellStyle>,
) -> Line<'static> {
    let mut spans = Vec::new();
    let bg_style = diff_style.map(|style| style.line_style());
    push_number(&mut spans, left, model.left_digits(), bg_style);
    push_styled_text(&mut spans, " ", gutter_style(), bg_style);
    push_number(&mut spans, right, model.right_digits(), bg_style);
    push_styled_text(&mut spans, " ", gutter_style(), bg_style);
    push_styled_text(&mut spans, &marker.to_string(), marker_style, bg_style);
    push_styled_text(&mut spans, " ", gutter_style(), bg_style);

    let prefix_width = model.left_digits() + model.right_digits() + 4;
    let content_width = width.saturating_sub(prefix_width);
    let used = prefix_width.saturating_add(push_structured_content(
        &mut spans,
        content,
        x,
        content_width,
        diff_style,
    ));
    if bg_style.is_some() {
        fill_row(&mut spans, width.saturating_sub(used), bg_style);
    }
    Line::from(spans)
}

fn render_side_row(row: &SideDiffRow, model: &DiffModel, width: usize, x: usize) -> Line<'static> {
    match row {
        SideDiffRow::Message { text } => styled_text_line(text, width, plain_style()),
        SideDiffRow::Context {
            left,
            right,
            content,
        } => render_side_content(
            Some(NumberedDiffLine {
                number: *left,
                content: content.clone(),
            }),
            Some(NumberedDiffLine {
                number: *right,
                content: content.clone(),
            }),
            model,
            width,
            x,
            None,
            None,
        ),
        SideDiffRow::Change { left, right } => render_side_content(
            left.as_ref().map(|line| line.line.clone()),
            right.as_ref().map(|line| line.line.clone()),
            model,
            width,
            x,
            left.as_ref().map(|line| DiffCellStyle {
                side: DiffSide::Removed,
                change: line.change,
            }),
            right.as_ref().map(|line| DiffCellStyle {
                side: DiffSide::Added,
                change: line.change,
            }),
        ),
    }
}

fn render_side_content(
    left: Option<NumberedDiffLine>,
    right: Option<NumberedDiffLine>,
    model: &DiffModel,
    width: usize,
    x: usize,
    left_diff: Option<DiffCellStyle>,
    right_diff: Option<DiffCellStyle>,
) -> Line<'static> {
    let fixed_width = model.left_digits() + model.right_digits() + 7;
    let content_width = width.saturating_sub(fixed_width);
    let left_width = content_width / 2;
    let right_width = content_width.saturating_sub(left_width);
    let mut spans = Vec::new();
    push_side_cell(
        &mut spans,
        left.as_ref(),
        model.left_digits(),
        left_width,
        x,
        left_diff,
    );
    spans.push(Span::styled(" │ ", gutter_style()));
    push_side_cell(
        &mut spans,
        right.as_ref(),
        model.right_digits(),
        right_width,
        x,
        right_diff,
    );
    Line::from(spans)
}

fn push_side_cell(
    spans: &mut Vec<Span<'static>>,
    line: Option<&NumberedDiffLine>,
    digits: usize,
    width: usize,
    x: usize,
    diff_style: Option<DiffCellStyle>,
) {
    let bg_style = diff_style.map(|style| style.line_style());
    push_number(spans, line.map(|line| line.number), digits, bg_style);
    push_styled_text(spans, " ", gutter_style(), bg_style);
    let marker = diff_style
        .filter(|_| line.is_some())
        .map(|style| style.marker())
        .unwrap_or(' ');
    let marker_style = diff_style
        .filter(|_| line.is_some())
        .map(|style| style.marker_style())
        .unwrap_or_else(gutter_style);
    push_styled_text(spans, &marker.to_string(), marker_style, bg_style);
    push_styled_text(spans, " ", gutter_style(), bg_style);

    let before = spans.len();
    if let Some(line) = line {
        push_structured_content(spans, &line.content, x, width, diff_style);
    }
    let used = spans[before..]
        .iter()
        .map(|span| char_count(span.content.as_ref()))
        .sum::<usize>();
    fill_row(spans, width.saturating_sub(used), bg_style);
}

fn push_number(
    spans: &mut Vec<Span<'static>>,
    number: Option<usize>,
    digits: usize,
    bg_style: Option<Style>,
) {
    let text = number
        .map(|number| format!("{number:>digits$}"))
        .unwrap_or_else(|| " ".repeat(digits));
    push_styled_text(spans, &text, gutter_style(), bg_style);
}

fn push_structured_content(
    spans: &mut Vec<Span<'static>>,
    content: &str,
    x: usize,
    width: usize,
    diff_style: Option<DiffCellStyle>,
) -> usize {
    if width == 0 {
        return 0;
    }

    let start = byte_index_for_char(content, x);
    let end = byte_index_for_char(content, x.saturating_add(width));
    let highlighted = highlight_content_window(content, ViewMode::Plain, start, end);
    let mut cursor = x;
    let mut written = 0_usize;
    for span in highlighted {
        let text = span.content.as_ref();
        let count = char_count(text);
        push_diff_span_segments(spans, text, cursor, span.style, diff_style);
        cursor = cursor.saturating_add(count);
        written = written.saturating_add(count);
    }
    written
}

#[derive(Debug, Clone, Copy)]
struct DiffCellStyle {
    side: DiffSide,
    change: DiffChange,
}

impl DiffCellStyle {
    fn line_style(self) -> Style {
        Style::default().bg(self.line_bg())
    }

    fn inline_style(self) -> Style {
        Style::default()
            .bg(self.inline_bg())
            .add_modifier(Modifier::BOLD)
    }

    fn marker(self) -> char {
        match self.side {
            DiffSide::Removed => '-',
            DiffSide::Added => '+',
        }
    }

    fn marker_style(self) -> Style {
        match self.side {
            DiffSide::Removed => diff_removed_style(),
            DiffSide::Added => diff_added_style(),
        }
    }

    fn range(self) -> Option<DiffRange> {
        match self.side {
            DiffSide::Removed => self.change.left_range,
            DiffSide::Added => self.change.right_range,
        }
    }

    fn line_bg(self) -> Color {
        match self.side {
            DiffSide::Removed => diff_removed_line_bg(self.change.intensity),
            DiffSide::Added => diff_added_line_bg(self.change.intensity),
        }
    }

    fn inline_bg(self) -> Color {
        match self.side {
            DiffSide::Removed => diff_removed_inline_bg(self.change.intensity),
            DiffSide::Added => diff_added_inline_bg(self.change.intensity),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum DiffSide {
    Removed,
    Added,
}

fn push_diff_span_segments(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    start_char: usize,
    base_style: Style,
    diff_style: Option<DiffCellStyle>,
) {
    let Some(diff_style) = diff_style else {
        spans.push(Span::styled(text.to_owned(), base_style));
        return;
    };

    let line_style = diff_style.line_style();
    let Some(range) = diff_style.range() else {
        spans.push(Span::styled(text.to_owned(), base_style.patch(line_style)));
        return;
    };
    let text_len = char_count(text);
    let end_char = start_char.saturating_add(text_len);
    if range.end <= start_char || range.start >= end_char {
        spans.push(Span::styled(text.to_owned(), base_style.patch(line_style)));
        return;
    }
    if range.start <= start_char && range.end >= end_char {
        spans.push(Span::styled(
            text.to_owned(),
            base_style
                .patch(line_style)
                .patch(diff_style.inline_style()),
        ));
        return;
    }

    let before_end = range.start.saturating_sub(start_char).min(text_len);
    let inline_start = before_end;
    let inline_end = range.end.saturating_sub(start_char).min(text_len);
    push_optional_segment(spans, text, 0, before_end, base_style.patch(line_style));
    push_optional_segment(
        spans,
        text,
        inline_start,
        inline_end,
        base_style
            .patch(line_style)
            .patch(diff_style.inline_style()),
    );
    push_optional_segment(
        spans,
        text,
        inline_end,
        text_len,
        base_style.patch(line_style),
    );
}

fn push_optional_segment(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    start: usize,
    end: usize,
    style: Style,
) {
    if start >= end {
        return;
    }

    spans.push(Span::styled(slice_char_range(text, start, end), style));
}

fn slice_char_range(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

fn push_styled_text(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    style: Style,
    bg_style: Option<Style>,
) {
    let style = bg_style
        .map(|bg_style| style.patch(bg_style))
        .unwrap_or(style);
    spans.push(Span::styled(text.to_owned(), style));
}

fn fill_row(spans: &mut Vec<Span<'static>>, count: usize, bg_style: Option<Style>) {
    if count == 0 {
        return;
    }

    spans.push(Span::styled(
        " ".repeat(count),
        bg_style.unwrap_or_default(),
    ));
}

fn styled_text_line(text: &str, width: usize, style: Style) -> Line<'static> {
    let end = byte_index_for_char(text, width);
    Line::from(vec![Span::styled(text[..end].to_owned(), style)])
}

fn drain_events(
    model: &DiffModel,
    state: &mut DiffViewState,
    page: usize,
    visible_height: usize,
) -> Result<DiffEventAction> {
    let started = Instant::now();
    let mut action = DiffEventAction::default();
    let mut processed = 0;

    loop {
        let event = event::read().context("failed to read terminal event")?;
        let next = handle_event(event, model, state, page, visible_height);
        action.merge(next);
        processed += 1;

        if action.quit
            || processed >= EVENT_DRAIN_LIMIT
            || started.elapsed() >= EVENT_DRAIN_BUDGET
            || !event::poll(Duration::ZERO).context("failed to poll terminal event")?
        {
            break;
        }
    }

    Ok(action)
}

fn handle_event(
    event: Event,
    model: &DiffModel,
    state: &mut DiffViewState,
    page: usize,
    visible_height: usize,
) -> DiffEventAction {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Release => DiffEventAction::default(),
        Event::Key(key) => {
            handle_key_event(key.code, key.modifiers, model, state, page, visible_height)
        }
        Event::Mouse(mouse) => {
            handle_mouse_event(mouse.kind, mouse.modifiers, model, state, visible_height)
        }
        Event::Resize(_, _) => DiffEventAction {
            dirty: true,
            quit: false,
        },
        _ => DiffEventAction::default(),
    }
}

fn handle_key_event(
    code: KeyCode,
    modifiers: KeyModifiers,
    model: &DiffModel,
    state: &mut DiffViewState,
    page: usize,
    visible_height: usize,
) -> DiffEventAction {
    if matches!(code, KeyCode::Char('c')) && modifiers.contains(KeyModifiers::CONTROL) {
        return DiffEventAction {
            dirty: false,
            quit: true,
        };
    }

    let line_count = model.row_count(state.layout);
    let dirty = match code {
        KeyCode::Char('q') | KeyCode::Esc => {
            return DiffEventAction {
                dirty: false,
                quit: true,
            };
        }
        KeyCode::Char('s') if plain_key(modifiers) => {
            state.layout = state.layout.toggle();
            state.change_cursor = None;
            clamp_top(state, model, visible_height);
            true
        }
        KeyCode::Char(']') if plain_key(modifiers) => {
            jump_change(model, state, DiffJump::Next, page, visible_height)
        }
        KeyCode::Char('[') if plain_key(modifiers) => {
            jump_change(model, state, DiffJump::Previous, page, visible_height)
        }
        KeyCode::Down | KeyCode::Char('j') if plain_key(modifiers) => {
            state.change_cursor = None;
            scroll_by(state, line_count, visible_height, 1)
        }
        KeyCode::Up | KeyCode::Char('k') if plain_key(modifiers) => {
            state.change_cursor = None;
            scroll_by(state, line_count, visible_height, -1)
        }
        KeyCode::PageDown | KeyCode::Char(' ') | KeyCode::Char('f') if plain_key(modifiers) => {
            state.change_cursor = None;
            scroll_by(state, line_count, visible_height, page as isize)
        }
        KeyCode::PageUp | KeyCode::Char('b') if plain_key(modifiers) => {
            state.change_cursor = None;
            scroll_by(state, line_count, visible_height, -(page as isize))
        }
        KeyCode::Home | KeyCode::Char('g') if plain_key(modifiers) => {
            state.change_cursor = None;
            set_top(state, 0, line_count, visible_height)
        }
        KeyCode::End | KeyCode::Char('G') if plain_key(modifiers) => {
            state.change_cursor = None;
            set_top(
                state,
                max_top_for_view(line_count, visible_height),
                line_count,
                visible_height,
            )
        }
        KeyCode::Right | KeyCode::Char('l') if plain_key(modifiers) => {
            scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        KeyCode::Left | KeyCode::Char('h') if plain_key(modifiers) => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        _ => false,
    };

    DiffEventAction { dirty, quit: false }
}

fn handle_mouse_event(
    kind: MouseEventKind,
    modifiers: KeyModifiers,
    model: &DiffModel,
    state: &mut DiffViewState,
    visible_height: usize,
) -> DiffEventAction {
    let line_count = model.row_count(state.layout);
    let dirty = match kind {
        MouseEventKind::ScrollDown if modifiers.contains(KeyModifiers::SHIFT) => {
            scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize)
        }
        MouseEventKind::ScrollUp if modifiers.contains(KeyModifiers::SHIFT) => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        MouseEventKind::ScrollDown => {
            state.change_cursor = None;
            scroll_by(
                state,
                line_count,
                visible_height,
                MOUSE_SCROLL_LINES as isize,
            )
        }
        MouseEventKind::ScrollUp => {
            state.change_cursor = None;
            scroll_by(
                state,
                line_count,
                visible_height,
                -(MOUSE_SCROLL_LINES as isize),
            )
        }
        MouseEventKind::ScrollRight => scroll_x_by(&mut state.x, MOUSE_HORIZONTAL_COLUMNS as isize),
        MouseEventKind::ScrollLeft => {
            scroll_x_by(&mut state.x, -(MOUSE_HORIZONTAL_COLUMNS as isize))
        }
        _ => false,
    };

    DiffEventAction { dirty, quit: false }
}

#[derive(Debug, Default)]
struct DiffEventAction {
    dirty: bool,
    quit: bool,
}

impl DiffEventAction {
    fn merge(&mut self, next: Self) {
        self.dirty |= next.dirty;
        self.quit |= next.quit;
    }
}

#[derive(Debug, Clone, Copy)]
enum DiffJump {
    Next,
    Previous,
}

fn jump_change(
    model: &DiffModel,
    state: &mut DiffViewState,
    direction: DiffJump,
    page: usize,
    visible_height: usize,
) -> bool {
    let changes = model.changed_rows(state.layout);
    if changes.is_empty() {
        state.message = Some("no differences".to_owned());
        return true;
    }
    let targets = change_block_starts(changes);

    let anchor = state.change_cursor.unwrap_or(state.top);
    let target = match direction {
        DiffJump::Next => targets
            .iter()
            .copied()
            .find(|row| *row > anchor)
            .unwrap_or(targets[0]),
        DiffJump::Previous => targets
            .iter()
            .rev()
            .copied()
            .find(|row| *row < anchor)
            .unwrap_or(*targets.last().unwrap_or(&0)),
    };
    state.change_cursor = Some(target);
    set_top(
        state,
        target.saturating_sub(diff_context_rows(page)),
        model.row_count(state.layout),
        visible_height,
    )
}

fn change_block_starts(changes: &[usize]) -> Vec<usize> {
    changes
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, row)| {
            (index == 0 || row > changes[index - 1].saturating_add(1)).then_some(row)
        })
        .collect()
}

fn diff_context_rows(page: usize) -> usize {
    if page < 4 {
        return 0;
    }

    (page / 3).clamp(2, 8).min(page.saturating_sub(1))
}

fn scroll_by(
    state: &mut DiffViewState,
    line_count: usize,
    visible_height: usize,
    delta: isize,
) -> bool {
    let max_top = max_top_for_view(line_count, visible_height);
    let target = if delta >= 0 {
        state.top.saturating_add(delta as usize).min(max_top)
    } else {
        state.top.saturating_sub(delta.unsigned_abs())
    };
    set_top(state, target, line_count, visible_height)
}

fn set_top(
    state: &mut DiffViewState,
    top: usize,
    line_count: usize,
    visible_height: usize,
) -> bool {
    let old = state.top;
    state.top = top.min(max_top_for_view(line_count, visible_height));
    state.top != old
}

fn clamp_top(state: &mut DiffViewState, model: &DiffModel, visible_height: usize) {
    state.top = state.top.min(max_top_for_view(
        model.row_count(state.layout),
        visible_height,
    ));
}

fn max_top_for_view(row_count: usize, visible_height: usize) -> usize {
    row_count.saturating_sub(visible_height.max(1))
}

fn scroll_x_by(x: &mut usize, delta: isize) -> bool {
    let old = *x;
    if delta >= 0 {
        *x = x.saturating_add(delta as usize);
    } else {
        *x = x.saturating_sub(delta.unsigned_abs());
    }
    *x != old
}

fn diff_scroll_hint(
    terminal: &ViewerTerminal<CrosstermBackend<io::Stdout>>,
    position: ViewPosition,
) -> Option<ScrollHint> {
    let previous = terminal.previous_position()?;
    if previous.row_offset != 0 || position.row_offset != 0 {
        return None;
    }

    let delta = position.top.abs_diff(previous.top);
    if delta == 0 || delta > DIFF_SCROLL_HINT_MAX_ROWS {
        return None;
    }
    let amount = u16::try_from(delta).ok()?;
    if position.top > previous.top {
        Some(ScrollHint::up(amount))
    } else {
        Some(ScrollHint::down(amount))
    }
}

fn progress_percent(bottom: usize, row_count: usize) -> usize {
    bottom
        .saturating_mul(100)
        .checked_div(row_count)
        .unwrap_or(100)
}

fn plain_key(modifiers: KeyModifiers) -> bool {
    modifiers.is_empty() || modifiers == KeyModifiers::SHIFT
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{hint::black_box, time::Instant};

    fn sample_model() -> DiffModel {
        DiffModel::from_unified_patch(
            "left".to_owned(),
            "right".to_owned(),
            "\
--- left
+++ right
@@ -1,4 +1,4 @@
 {
-  \"a\": 1,
+  \"a\": 2,
   \"b\": true
 }
",
        )
    }

    #[test]
    fn renders_unified_diff_rows_with_line_numbers() {
        let lines = render_rows(&sample_model(), DiffLayout::Unified, 0, 3, 80, 0);
        let text = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>();

        assert_eq!(text[0], "1 1   {");
        assert_eq!(text[1], "2   -   \"a\": 1,");
        assert_eq!(text[2], "  2 +   \"a\": 2,");
    }

    #[test]
    fn renders_side_by_side_change_pairs() {
        let lines = render_rows(&sample_model(), DiffLayout::SideBySide, 1, 1, 80, 0);
        let text = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.contains("-   \"a\": 1,"));
        assert!(text.contains("+   \"a\": 2,"));
    }

    #[test]
    fn interactive_diff_hides_patch_control_rows() {
        let lines = render_rows(&sample_model(), DiffLayout::Unified, 0, 10, 80, 0);
        let text = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(!text.contains("@@"));
        assert!(!text.contains("---"));
        assert!(!text.contains("+++"));
    }

    #[test]
    fn changed_rows_use_line_and_inline_backgrounds() {
        let lines = render_rows(&sample_model(), DiffLayout::Unified, 1, 1, 80, 0);
        let removed = &lines[0];

        assert!(removed.spans.iter().any(|span| {
            span.style.bg == Some(diff_removed_line_bg(crate::diff::DiffIntensity::Low))
        }));
        assert!(removed.spans.iter().any(|span| {
            span.content.as_ref().contains('1')
                && span.style.bg == Some(diff_removed_inline_bg(crate::diff::DiffIntensity::Low))
        }));
    }

    #[test]
    fn change_jump_skips_adjacent_rows_in_same_block() {
        let model = DiffModel::from_unified_patch(
            "left".to_owned(),
            "right".to_owned(),
            "\
--- left
+++ right
@@ -1,4 +1,4 @@
 a
-old
+new
 b
 c
@@ -20,3 +20,3 @@
 x
-old2
+new2
 y
",
        );
        let targets = change_block_starts(model.changed_rows(DiffLayout::Unified));
        let mut state = DiffViewState::new(DiffLayout::Unified);

        assert_eq!(targets, vec![1, 6]);
        jump_change(&model, &mut state, DiffJump::Next, 9, 4);
        assert_eq!(state.change_cursor, Some(1));

        assert!(jump_change(&model, &mut state, DiffJump::Next, 9, 4));
        assert_eq!(state.change_cursor, Some(6));
        assert_eq!(state.top, 3);
    }

    #[test]
    fn diff_scroll_clamps_to_last_full_page() {
        let model = sample_model();
        let mut state = DiffViewState::new(DiffLayout::Unified);

        assert!(scroll_by(
            &mut state,
            model.row_count(DiffLayout::Unified),
            3,
            99,
        ));

        assert_eq!(state.top, model.row_count(DiffLayout::Unified) - 3);
    }

    #[test]
    fn side_by_side_scroll_uses_longer_display_side() {
        let model = DiffModel::from_unified_patch(
            "left".to_owned(),
            "right".to_owned(),
            "\
--- left
+++ right
@@ -1,3 +1,5 @@
 a
-old
+new1
+new2
+new3
 z
",
        );
        let mut state = DiffViewState::new(DiffLayout::SideBySide);
        let row_count = model.row_count(DiffLayout::SideBySide);

        assert!(scroll_by(&mut state, row_count, 3, 99));
        assert_eq!(state.top, 2);

        let text = render_rows(&model, DiffLayout::SideBySide, state.top, 3, 80, 0)
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.contains("new3"));
    }

    #[test]
    #[ignore = "performance smoke; run benches/diff-performance.sh"]
    fn perf_diff_view_render() {
        let patch = generated_patch(2_048, 3);
        let model = DiffModel::from_unified_patch("left".to_owned(), "right".to_owned(), &patch);
        let mut rendered_rows = 0_usize;
        let started = Instant::now();
        for index in 0..2_000 {
            let top = index
                % model
                    .row_count(DiffLayout::Unified)
                    .saturating_sub(32)
                    .max(1);
            let unified = render_rows(&model, DiffLayout::Unified, top, 28, 120, index % 8);
            rendered_rows = rendered_rows.saturating_add(unified.len());
            black_box(unified);

            let side_top = index
                % model
                    .row_count(DiffLayout::SideBySide)
                    .saturating_sub(32)
                    .max(1);
            let side = render_rows(&model, DiffLayout::SideBySide, side_top, 28, 160, index % 8);
            rendered_rows = rendered_rows.saturating_add(side.len());
            black_box(side);
        }
        let elapsed = started.elapsed();
        eprintln!(
            "diff view render: {elapsed:?}, rows={} changes={} rendered_rows={} patch_bytes={}",
            model.row_count(DiffLayout::Unified),
            model.changed_rows(DiffLayout::Unified).len(),
            rendered_rows,
            patch.len()
        );
    }

    fn generated_patch(hunks: usize, changes_per_hunk: usize) -> String {
        let mut patch = String::from("--- left\n+++ right\n");
        for hunk in 0..hunks {
            let start = hunk.saturating_mul(16).saturating_add(1);
            patch.push_str(&format!("@@ -{start},10 +{start},10 @@\n"));
            patch.push_str(" {\n");
            patch.push_str(&format!("   \"id\": {hunk},\n"));
            for change in 0..changes_per_hunk {
                patch.push_str(&format!("-  \"old_{change}\": \"{}\",\n", "x".repeat(48)));
            }
            for change in 0..changes_per_hunk {
                patch.push_str(&format!("+  \"new_{change}\": \"{}\",\n", "y".repeat(48)));
            }
            patch.push_str("   \"ok\": true\n");
            patch.push_str(" }\n");
        }
        patch
    }
}
