use std::{io, time::Duration};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::line_index::IndexedTempFile;

#[derive(Debug, Clone, Copy)]
pub enum ViewMode {
    Plain,
    Diff,
}

pub fn run(file: IndexedTempFile, mode: ViewMode) -> Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode().context("failed to enable raw mode")?;
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;
    let result = run_loop(&mut terminal, &file, mode);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    file: &IndexedTempFile,
    mode: ViewMode,
) -> Result<()> {
    let mut top = 0_usize;
    let mut x = 0_usize;
    let mut wrap = true;

    loop {
        terminal
            .draw(|frame| {
                let area = frame.area();
                let [body, footer] =
                    Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
                let visible_height = usize::from(body.height.saturating_sub(2));
                let visible_width = usize::from(body.width.saturating_sub(2));
                let gutter_digits = line_number_digits(file.line_count());
                let gutter_width = gutter_digits + 3;
                let content_width = visible_width.saturating_sub(gutter_width);
                let max_top = file.line_count().saturating_sub(visible_height.max(1));
                top = top.min(max_top);

                let lines = file.read_window(top, visible_height).unwrap_or_else(|error| {
                    vec![format!("failed to read window: {error:#}")]
                });
                let render_context = RenderContext {
                    gutter_digits,
                    x,
                    width: content_width,
                    wrap,
                    mode,
                };
                let styled = render_visible_lines(&lines, top + 1, visible_height, render_context);

                let current = if file.line_count() == 0 { 0 } else { top + 1 };
                let bottom = top.saturating_add(visible_height).min(file.line_count());
                let display_mode = if wrap {
                    "wrap".to_owned()
                } else {
                    format!("nowrap x:{x}")
                };
                let title = format!(
                    " {} | {} lines | {}-{} | {:>3}% | {} ",
                    file.label(),
                    file.line_count(),
                    current,
                    bottom,
                    progress_percent(bottom, file.line_count()),
                    display_mode
                );
                let paragraph = Paragraph::new(styled).block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
                frame.render_widget(paragraph, body);

                let footer_text =
                    " q/Esc quit | j/k/↑/↓ line | Space/f page down | b page up | Ctrl-d/u half | g/G top/end | w wrap ";
                frame.render_widget(
                    Paragraph::new(footer_text).style(Style::default().fg(Color::DarkGray)),
                    footer,
                );
            })
            .context("failed to draw terminal frame")?;

        if !event::poll(Duration::from_millis(250)).context("failed to poll terminal event")? {
            continue;
        }

        let Event::Key(key) = event::read().context("failed to read terminal event")? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        let page = terminal
            .size()
            .map(|size| usize::from(size.height.saturating_sub(4)).max(1))
            .unwrap_or(20);

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Char('w') => {
                wrap = !wrap;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                top = top
                    .saturating_add(1)
                    .min(file.line_count().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                top = top.saturating_sub(1);
            }
            KeyCode::PageDown | KeyCode::Char(' ') | KeyCode::Char('f') => {
                top = top
                    .saturating_add(page)
                    .min(file.line_count().saturating_sub(1));
            }
            KeyCode::PageUp | KeyCode::Char('b') => {
                top = top.saturating_sub(page);
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                top = top
                    .saturating_add((page / 2).max(1))
                    .min(file.line_count().saturating_sub(1));
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                top = top.saturating_sub((page / 2).max(1));
            }
            KeyCode::Home | KeyCode::Char('g') => {
                top = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                top = file.line_count().saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') if !wrap => {
                x = x.saturating_add(4);
            }
            KeyCode::Left | KeyCode::Char('h') if !wrap => {
                x = x.saturating_sub(4);
            }
            _ => {}
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct RenderContext {
    gutter_digits: usize,
    x: usize,
    width: usize,
    wrap: bool,
    mode: ViewMode,
}

fn render_visible_lines(
    lines: &[String],
    first_line_number: usize,
    height: usize,
    context: RenderContext,
) -> Vec<Line<'static>> {
    let mut rendered = Vec::with_capacity(height);

    for (index, line) in lines.iter().enumerate() {
        if rendered.len() >= height {
            break;
        }

        let remaining = height - rendered.len();
        rendered.extend(render_logical_line(
            line,
            first_line_number + index,
            remaining,
            context,
        ));
    }

    rendered
}

fn render_logical_line(
    line: &str,
    line_number: usize,
    max_rows: usize,
    context: RenderContext,
) -> Vec<Line<'static>> {
    if max_rows == 0 {
        return Vec::new();
    }

    if !context.wrap {
        return vec![styled_segment(
            line_number_gutter(line_number, context.gutter_digits),
            line,
            context.x,
            context.x.saturating_add(context.width),
            context.mode,
        )];
    }

    let spans = highlight_content(line, context.mode);
    let ranges = wrap_ranges(
        line,
        context.width,
        continuation_indent(line, context.width),
        max_rows,
    );
    ranges
        .iter()
        .enumerate()
        .map(|(index, range)| {
            let gutter = if index == 0 {
                line_number_gutter(line_number, context.gutter_digits)
            } else {
                continuation_gutter(context.gutter_digits)
            };
            let mut line_spans = vec![gutter];
            if range.continuation_indent > 0 {
                line_spans.push(Span::styled(
                    " ".repeat(range.continuation_indent),
                    Style::default(),
                ));
            }
            line_spans.extend(slice_spans(&spans, range.start, range.end));
            Line::from(line_spans)
        })
        .collect()
}

fn styled_segment(
    gutter: Span<'static>,
    line: &str,
    start: usize,
    end: usize,
    mode: ViewMode,
) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(gutter);
    spans.extend(slice_spans(&highlight_content(line, mode), start, end));
    Line::from(spans)
}

fn line_number_gutter(line_number: usize, gutter_digits: usize) -> Span<'static> {
    Span::styled(format!("{line_number:>gutter_digits$} │ "), gutter_style())
}

fn continuation_gutter(gutter_digits: usize) -> Span<'static> {
    Span::styled(format!("{:>gutter_digits$} ┆ ", ""), gutter_style())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WrapRange {
    start: usize,
    end: usize,
    continuation_indent: usize,
}

fn wrap_ranges(
    line: &str,
    width: usize,
    continuation_indent: usize,
    max_rows: usize,
) -> Vec<WrapRange> {
    if max_rows == 0 {
        return Vec::new();
    }

    let char_count = line.chars().count();
    if char_count == 0 || width == 0 {
        return vec![WrapRange {
            start: 0,
            end: 0,
            continuation_indent: 0,
        }];
    }

    let mut ranges = Vec::new();
    let mut start = 0;
    while start < char_count && ranges.len() < max_rows {
        let continuation = !ranges.is_empty();
        let indent = if continuation {
            continuation_indent.min(width.saturating_sub(1))
        } else {
            0
        };
        let row_width = width.saturating_sub(indent).max(1);
        let hard_end = start.saturating_add(row_width).min(char_count);
        let end = if hard_end < char_count {
            best_wrap_end(line, start, hard_end).unwrap_or(hard_end)
        } else {
            hard_end
        };
        let end = end.max(start + 1);
        ranges.push(WrapRange {
            start,
            end,
            continuation_indent: indent,
        });
        start = end;
    }

    ranges
}

fn best_wrap_end(line: &str, start: usize, hard_end: usize) -> Option<usize> {
    let chars = line.chars().collect::<Vec<_>>();
    let min_end = start + ((hard_end - start) / 2).max(1);

    for end in (min_end..=hard_end).rev() {
        let ch = chars[end - 1];
        if ch.is_whitespace() || matches!(ch, ',' | '>' | '}' | ']' | ';') {
            return Some(end);
        }
    }

    None
}

fn continuation_indent(line: &str, width: usize) -> usize {
    if width < 8 {
        return 0;
    }

    let indent = line
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| if ch == '\t' { 2 } else { 1 })
        .sum::<usize>()
        + 2;
    indent.min(24).min(width / 2)
}

fn slice_spans(spans: &[Span<'static>], start: usize, end: usize) -> Vec<Span<'static>> {
    if end <= start {
        return Vec::new();
    }

    let mut sliced = Vec::new();
    let mut cursor = 0;

    for span in spans {
        let text = span.content.as_ref();
        let len = text.chars().count();
        let span_start = cursor;
        let span_end = cursor + len;
        cursor = span_end;

        let overlap_start = start.max(span_start);
        let overlap_end = end.min(span_end);
        if overlap_start >= overlap_end {
            continue;
        }

        let text = slice_chars(text, overlap_start - span_start, overlap_end - span_start);
        sliced.push(Span::styled(text, span.style));
    }

    sliced
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end - start).collect()
}

fn line_number_digits(line_count: usize) -> usize {
    line_count.max(1).to_string().len()
}

fn progress_percent(bottom: usize, line_count: usize) -> usize {
    bottom
        .saturating_mul(100)
        .checked_div(line_count)
        .unwrap_or(100)
}

fn highlight_content(line: &str, mode: ViewMode) -> Vec<Span<'static>> {
    match mode {
        ViewMode::Plain => highlight_structured(line),
        ViewMode::Diff if line.starts_with("@@") => vec![Span::styled(
            line.to_owned(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )],
        ViewMode::Diff if line.starts_with("+++") || line.starts_with("---") => {
            vec![Span::styled(
                line.to_owned(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]
        }
        ViewMode::Diff if line.starts_with('+') => highlight_diff_payload(line, Color::Green),
        ViewMode::Diff if line.starts_with('-') => highlight_diff_payload(line, Color::Red),
        ViewMode::Diff => highlight_structured(line),
    }
}

fn highlight_diff_payload(line: &str, color: Color) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        line[..1].to_owned(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )];
    spans.extend(highlight_structured(&line[1..]));
    spans
}

fn highlight_structured(line: &str) -> Vec<Span<'static>> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('<') {
        highlight_xml_line(line)
    } else {
        highlight_json_like(line)
    }
}

fn highlight_json_like(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;

    while index < line.len() {
        let rest = &line[index..];
        let ch = rest.chars().next().expect("index should point to a char");

        if ch.is_whitespace() {
            let end = take_while(line, index, char::is_whitespace);
            push_span(&mut spans, &line[index..end], Style::default());
            index = end;
            continue;
        }

        if ch == '"' {
            let end = json_string_end(line, index);
            if json_string_is_key(line, end) {
                push_span(&mut spans, &line[index..end], key_style());
            } else {
                spans.extend(highlight_json_string_value(&line[index..end]));
            }
            index = end;
            continue;
        }

        if ch == '-' || ch.is_ascii_digit() {
            let end = take_while(line, index, is_json_number_char);
            push_span(&mut spans, &line[index..end], number_style());
            index = end;
            continue;
        }

        if let Some((word, style)) = json_keyword(rest) {
            push_span(&mut spans, word, style);
            index += word.len();
            continue;
        }

        if "{}[]:,".contains(ch) {
            push_span(
                &mut spans,
                &line[index..index + ch.len_utf8()],
                punctuation_style(),
            );
            index += ch.len_utf8();
            continue;
        }

        push_span(
            &mut spans,
            &line[index..index + ch.len_utf8()],
            Style::default(),
        );
        index += ch.len_utf8();
    }

    spans
}

fn highlight_json_string_value(text: &str) -> Vec<Span<'static>> {
    if !text.contains('<') {
        return highlight_string_segment(text);
    }

    let mut spans = Vec::new();
    let inner_start = if text.starts_with('"') { 1 } else { 0 };
    let inner_end = if text.len() > inner_start && text.ends_with('"') {
        text.len() - 1
    } else {
        text.len()
    };

    spans.extend(highlight_string_segment(&text[..inner_start]));
    spans.extend(highlight_inline_xml(&text[inner_start..inner_end], 0));
    spans.extend(highlight_string_segment(&text[inner_end..]));
    spans
}

fn highlight_string_segment(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;
    let mut plain_start = 0;

    while index < text.len() {
        if let Some(end) = escape_token_end(text, index) {
            push_span(&mut spans, &text[plain_start..index], string_style());
            push_span(&mut spans, &text[index..end], escape_style());
            index = end;
            plain_start = index;
            continue;
        }

        let ch = text[index..]
            .chars()
            .next()
            .expect("index should point to a char");
        index += ch.len_utf8();
    }

    push_span(&mut spans, &text[plain_start..], string_style());
    spans
}

fn highlight_xml_line(line: &str) -> Vec<Span<'static>> {
    let base_depth = xml_depth_from_indent(line);
    highlight_inline_xml(line, base_depth)
}

fn highlight_inline_xml(line: &str, base_depth: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;
    let mut state = XmlPairState::default();

    while index < line.len() {
        let rest = &line[index..];
        if rest.starts_with('<') {
            let end = rest
                .find('>')
                .map(|position| index + position + 1)
                .unwrap_or(line.len());
            let tag = &line[index..end];
            if looks_like_xml_tag(tag) {
                spans.extend(highlight_xml_tag(tag, &mut state, base_depth));
            } else {
                spans.extend(highlight_string_segment(tag));
            }
            index = end;
        } else {
            let end = rest
                .find('<')
                .map(|position| index + position)
                .unwrap_or(line.len());
            spans.extend(highlight_string_segment(&line[index..end]));
            index = end;
        }
    }

    spans
}

fn highlight_xml_tag(tag: &str, state: &mut XmlPairState, base_depth: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut index = 0;
    let kind = xml_tag_kind(tag);
    let name_range = xml_tag_name_range(tag);
    let name = name_range.map(|(start, end)| &tag[start..end]);
    let tag_state = state.apply(kind, name, base_depth);

    while index < tag.len() {
        let rest = &tag[index..];
        let ch = rest.chars().next().expect("index should point to a char");

        if let Some((start, end)) = name_range
            && index == start
        {
            let style = if tag_state.matched {
                xml_depth_style(tag_state.depth)
            } else {
                error_style()
            };
            push_span(&mut spans, &tag[start..end], style);
            index = end;
            continue;
        }

        if ch.is_whitespace() {
            let end = take_while(tag, index, char::is_whitespace);
            push_span(&mut spans, &tag[index..end], Style::default());
            index = end;
            continue;
        }

        if rest.starts_with("\\\"") || rest.starts_with("\\'") {
            let quote = rest.chars().nth(1).expect("escaped quote should exist");
            let end = escaped_quoted_end(tag, index, quote);
            spans.extend(highlight_string_segment(&tag[index..end]));
            index = end;
            continue;
        }

        if ch == '"' || ch == '\'' {
            let end = quoted_end(tag, index, ch);
            spans.extend(highlight_string_segment(&tag[index..end]));
            index = end;
            continue;
        }

        if "<>/=?!".contains(ch) {
            push_span(
                &mut spans,
                &tag[index..index + ch.len_utf8()],
                punctuation_style(),
            );
            index += ch.len_utf8();
            continue;
        }

        if is_xml_name_char(ch) {
            let end = take_while(tag, index, is_xml_name_char);
            push_span(&mut spans, &tag[index..end], attr_style());
            index = end;
            continue;
        }

        push_span(
            &mut spans,
            &tag[index..index + ch.len_utf8()],
            Style::default(),
        );
        index += ch.len_utf8();
    }

    spans
}

#[derive(Debug, Default)]
struct XmlPairState {
    stack: Vec<XmlOpenTag>,
}

#[derive(Debug)]
struct XmlOpenTag {
    name: String,
    depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XmlTagKind {
    Open,
    Close,
    SelfClosing,
    Other,
}

#[derive(Debug, Clone, Copy)]
struct XmlTagState {
    depth: usize,
    matched: bool,
}

impl XmlPairState {
    fn apply(&mut self, kind: XmlTagKind, name: Option<&str>, base_depth: usize) -> XmlTagState {
        match (kind, name) {
            (XmlTagKind::Open, Some(name)) => {
                let depth = base_depth + self.stack.len();
                self.stack.push(XmlOpenTag {
                    name: name.to_owned(),
                    depth,
                });
                XmlTagState {
                    depth,
                    matched: true,
                }
            }
            (XmlTagKind::SelfClosing, Some(_)) => XmlTagState {
                depth: base_depth + self.stack.len(),
                matched: true,
            },
            (XmlTagKind::Close, Some(name)) => match self.stack.pop() {
                Some(open) if open.name == name => XmlTagState {
                    depth: open.depth,
                    matched: true,
                },
                Some(open) => {
                    self.stack.push(open);
                    XmlTagState {
                        depth: base_depth + self.stack.len().saturating_sub(1),
                        matched: false,
                    }
                }
                None => XmlTagState {
                    depth: base_depth,
                    matched: true,
                },
            },
            _ => XmlTagState {
                depth: base_depth + self.stack.len(),
                matched: true,
            },
        }
    }
}

fn looks_like_xml_tag(tag: &str) -> bool {
    tag.starts_with("</")
        || tag.starts_with("<?")
        || tag.starts_with("<!")
        || xml_tag_name_range(tag).is_some()
}

fn xml_tag_kind(tag: &str) -> XmlTagKind {
    if tag.starts_with("</") {
        XmlTagKind::Close
    } else if tag.starts_with("<?") || tag.starts_with("<!") {
        XmlTagKind::Other
    } else if tag.trim_end_matches('>').trim_end().ends_with('/') {
        XmlTagKind::SelfClosing
    } else {
        XmlTagKind::Open
    }
}

fn xml_tag_name_range(tag: &str) -> Option<(usize, usize)> {
    let mut index = if tag.starts_with("</") { 2 } else { 1 };
    while index < tag.len() {
        let ch = tag[index..].chars().next()?;
        if !ch.is_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }

    let start = index;
    let end = take_while(tag, start, is_xml_name_char);
    (end > start).then_some((start, end))
}

fn xml_depth_from_indent(line: &str) -> usize {
    line.chars()
        .take_while(|ch| ch.is_whitespace())
        .map(|ch| if ch == '\t' { 2 } else { 1 })
        .sum::<usize>()
        / 2
}

fn take_while<F>(text: &str, start: usize, mut predicate: F) -> usize
where
    F: FnMut(char) -> bool,
{
    let mut end = start;
    for ch in text[start..].chars() {
        if !predicate(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    end
}

fn json_string_end(line: &str, start: usize) -> usize {
    let mut escaped = false;
    for (offset, ch) in line[start + 1..].char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return start + 1 + offset + ch.len_utf8();
        }
    }
    line.len()
}

fn json_string_is_key(line: &str, end: usize) -> bool {
    line[end..].trim_start().starts_with(':')
}

fn is_json_number_char(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.' | 'e' | 'E')
}

fn json_keyword(rest: &str) -> Option<(&str, Style)> {
    for keyword in ["true", "false"] {
        if rest.starts_with(keyword) && keyword_boundary(rest, keyword.len()) {
            return Some((keyword, bool_style()));
        }
    }

    if rest.starts_with("null") && keyword_boundary(rest, "null".len()) {
        Some(("null", null_style()))
    } else {
        None
    }
}

fn keyword_boundary(rest: &str, end: usize) -> bool {
    rest[end..]
        .chars()
        .next()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
}

fn quoted_end(text: &str, start: usize, quote: char) -> usize {
    for (offset, ch) in text[start + 1..].char_indices() {
        if ch == quote {
            return start + 1 + offset + ch.len_utf8();
        }
    }
    text.len()
}

fn escaped_quoted_end(text: &str, start: usize, quote: char) -> usize {
    let pattern = if quote == '"' { "\\\"" } else { "\\'" };
    text[start + pattern.len()..]
        .find(pattern)
        .map(|offset| start + pattern.len() + offset + pattern.len())
        .unwrap_or(text.len())
}

fn escape_token_end(text: &str, start: usize) -> Option<usize> {
    let rest = text.get(start..)?;
    if !rest.starts_with('\\') {
        return None;
    }

    let mut chars = rest.chars();
    chars.next()?;
    let escaped = chars.next()?;
    let escaped_start = start + '\\'.len_utf8();
    let escaped_end = escaped_start + escaped.len_utf8();

    if escaped == 'u' {
        let unicode_end = escaped_end + 4;
        if text
            .get(escaped_end..unicode_end)
            .is_some_and(|digits| digits.chars().all(|ch| ch.is_ascii_hexdigit()))
        {
            return Some(unicode_end);
        }
    }

    Some(escaped_end)
}

fn is_xml_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.')
}

fn push_span(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if !text.is_empty() {
        spans.push(Span::styled(text.to_owned(), style));
    }
}

fn gutter_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn punctuation_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn key_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn xml_depth_style(depth: usize) -> Style {
    const COLORS: [Color; 6] = [
        Color::Cyan,
        Color::Magenta,
        Color::Yellow,
        Color::Green,
        Color::Blue,
        Color::LightCyan,
    ];

    Style::default()
        .fg(COLORS[depth % COLORS.len()])
        .add_modifier(Modifier::BOLD)
}

fn attr_style() -> Style {
    Style::default().fg(Color::Yellow)
}

fn string_style() -> Style {
    Style::default().fg(Color::Green)
}

fn escape_style() -> Style {
    Style::default()
        .fg(Color::LightMagenta)
        .add_modifier(Modifier::BOLD)
}

fn number_style() -> Style {
    Style::default().fg(Color::Magenta)
}

fn bool_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

fn null_style() -> Style {
    Style::default().fg(Color::Blue)
}

fn error_style() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_by_character_not_byte() {
        assert_eq!(slice_chars("a路径b", 1, 3), "路径");
    }

    #[test]
    fn styled_line_keeps_a_gutter() {
        let line = render_logical_line(
            r#"  "name": "fmtview","#,
            12,
            1,
            RenderContext {
                gutter_digits: 3,
                x: 0,
                width: 80,
                wrap: false,
                mode: ViewMode::Plain,
            },
        )
        .remove(0);
        assert_eq!(span_text(&line.spans), r#" 12 │   "name": "fmtview","#);
    }

    #[test]
    fn wrap_uses_continuation_gutter_and_indent() {
        let lines = render_logical_line(
            r#"  "payload": "abcdefghijklmnopqrstuvwxyz","#,
            7,
            3,
            RenderContext {
                gutter_digits: 2,
                x: 0,
                width: 18,
                wrap: true,
                mode: ViewMode::Plain,
            },
        );

        assert!(lines.len() > 1);
        assert!(span_text(&lines[0].spans).starts_with(" 7 │ "));
        assert!(span_text(&lines[1].spans).starts_with("   ┆     "));
    }

    #[test]
    fn nowrap_applies_horizontal_offset() {
        let lines = render_logical_line(
            "abcdef",
            1,
            1,
            RenderContext {
                gutter_digits: 1,
                x: 2,
                width: 3,
                wrap: false,
                mode: ViewMode::Plain,
            },
        );

        assert_eq!(span_text(&lines[0].spans), "1 │ cde");
    }

    #[test]
    fn json_highlight_preserves_visible_text() {
        let spans = highlight_json_like(r#"  "ok": true, "n": 42, "none": null"#);
        assert_eq!(span_text(&spans), r#"  "ok": true, "n": 42, "none": null"#);
    }

    #[test]
    fn json_string_escape_tokens_are_highlighted() {
        let spans = highlight_json_like(r#"  "text": "line\nnext\t\u263A\\done""#);
        assert_eq!(span_text(&spans), r#"  "text": "line\nnext\t\u263A\\done""#);

        assert_eq!(styles_for_text(&spans, r#"\n"#), vec![escape_style()]);
        assert_eq!(styles_for_text(&spans, r#"\t"#), vec![escape_style()]);
        assert_eq!(styles_for_text(&spans, r#"\u263A"#), vec![escape_style()]);
        assert_eq!(styles_for_text(&spans, r#"\\"#), vec![escape_style()]);
    }

    #[test]
    fn xml_highlight_preserves_visible_text() {
        let spans = highlight_xml_line(r#"<root id="1"><child>value</child></root>"#);
        assert_eq!(
            span_text(&spans),
            r#"<root id="1"><child>value</child></root>"#
        );
    }

    #[test]
    fn embedded_xml_string_uses_tag_pairing() {
        let spans = highlight_json_like(r#"  "xml": "<root><child id=\"1\">v</child></root>""#);
        assert_eq!(
            span_text(&spans),
            r#"  "xml": "<root><child id=\"1\">v</child></root>""#
        );

        let root_styles = styles_for_text(&spans, "root");
        assert_eq!(root_styles.len(), 2);
        assert_eq!(root_styles[0], root_styles[1]);

        let child_styles = styles_for_text(&spans, "child");
        assert_eq!(child_styles.len(), 2);
        assert_eq!(child_styles[0], child_styles[1]);
        assert_ne!(root_styles[0], child_styles[0]);
        assert_eq!(
            styles_for_text(&spans, r#"\""#),
            vec![escape_style(), escape_style()]
        );
    }

    #[test]
    fn mismatched_inline_xml_tag_is_marked() {
        let spans = highlight_json_like(r#"  "xml": "<root></child>""#);
        let child_styles = styles_for_text(&spans, "child");
        assert_eq!(child_styles, vec![error_style()]);
    }

    fn span_text(spans: &[Span<'static>]) -> String {
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    fn styles_for_text(spans: &[Span<'static>], text: &str) -> Vec<Style> {
        spans
            .iter()
            .filter(|span| span.content.as_ref() == text)
            .map(|span| span.style)
            .collect()
    }
}
