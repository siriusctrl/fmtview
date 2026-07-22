use std::collections::VecDeque;

use crate::viewer::{KeyCode, KeyModifiers};
use anyhow::{Result, bail};

use crate::load::ViewFile;

use super::{
    keys::accepts_search_char,
    state::{FooterMessageKind, ViewState},
};
use crate::viewer::file::SEARCH_CHUNK_LINES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct SearchTarget {
    pub(in crate::viewer) line: usize,
    pub(in crate::viewer) byte_index: usize,
}

#[derive(Debug, Clone)]
pub(in crate::viewer) struct SearchTask {
    pub(in crate::viewer) query: String,
    pub(in crate::viewer) direction: SearchDirection,
    // Disjoint half-open ranges of currently loaded lines not yet visited by
    // this search. The front span is consumed in `direction`; appended and
    // inserted ranges are queued without restarting the completed prefix.
    spans: VecDeque<SearchSpan>,
    known_line_count: usize,
    // A forward wrap freezes non-append spans until the true older boundary is
    // known. Live append spans remain eligible while that prefix is loading.
    pub(in crate::viewer) awaiting_older: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SearchSpan {
    start: usize,
    end: usize,
    kind: SearchSpanKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchSpanKind {
    Initial,
    Wrapped,
    Appended,
    Inserted,
}

impl SearchSpan {
    fn new(start: usize, end: usize, kind: SearchSpanKind) -> Option<Self> {
        (start < end).then_some(Self { start, end, kind })
    }

    fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

impl SearchTask {
    fn new(
        query: String,
        direction: SearchDirection,
        start_line: usize,
        line_count: usize,
    ) -> Self {
        let mut spans = VecDeque::with_capacity(3);
        let split = start_line.min(line_count.saturating_sub(1));
        match direction {
            SearchDirection::Forward => {
                push_span(
                    &mut spans,
                    SearchSpan::new(split, line_count, SearchSpanKind::Initial),
                );
                push_span(
                    &mut spans,
                    SearchSpan::new(0, split, SearchSpanKind::Wrapped),
                );
            }
            SearchDirection::Backward => {
                let split = split.saturating_add(1).min(line_count);
                push_span(
                    &mut spans,
                    SearchSpan::new(0, split, SearchSpanKind::Initial),
                );
                push_span(
                    &mut spans,
                    SearchSpan::new(split, line_count, SearchSpanKind::Wrapped),
                );
            }
        }
        Self {
            query,
            direction,
            spans,
            known_line_count: line_count,
            awaiting_older: false,
        }
    }

    pub(in crate::viewer) fn extend_for_append(&mut self, start: usize, end: usize) {
        let start = start.max(self.known_line_count);
        self.known_line_count = self.known_line_count.max(end);
        if start >= end {
            return;
        }
        if self.direction == SearchDirection::Backward {
            let boundary = self
                .spans
                .iter()
                .position(|span| span.kind != SearchSpanKind::Initial)
                .unwrap_or(self.spans.len());
            if let Some(span) = self.spans.get_mut(boundary)
                && span.kind == SearchSpanKind::Appended
                && span.end == start
            {
                span.end = end;
                return;
            }
            self.spans.insert(
                boundary,
                SearchSpan {
                    start,
                    end,
                    kind: SearchSpanKind::Appended,
                },
            );
            return;
        }
        if self.awaiting_older {
            let priority_end = self
                .spans
                .iter()
                .position(|span| span.kind != SearchSpanKind::Appended)
                .unwrap_or(self.spans.len());
            if priority_end > 0 && self.spans[priority_end - 1].end == start {
                self.spans[priority_end - 1].end = end;
                return;
            }
            self.spans.insert(
                priority_end,
                SearchSpan {
                    start,
                    end,
                    kind: SearchSpanKind::Appended,
                },
            );
            return;
        }
        let deferred_boundary = self.spans.iter().position(|span| match self.direction {
            SearchDirection::Forward => matches!(
                span.kind,
                SearchSpanKind::Inserted | SearchSpanKind::Wrapped
            ),
            SearchDirection::Backward => span.kind == SearchSpanKind::Wrapped,
        });
        if let Some(boundary) = deferred_boundary {
            if boundary > 0
                && self.spans[boundary - 1].kind == SearchSpanKind::Appended
                && self.spans[boundary - 1].end == start
            {
                self.spans[boundary - 1].end = end;
                return;
            }
            self.spans.insert(
                boundary,
                SearchSpan {
                    start,
                    end,
                    kind: SearchSpanKind::Appended,
                },
            );
            return;
        }
        if let Some(last) = self.spans.back_mut()
            && last.kind == SearchSpanKind::Appended
            && last.end == start
        {
            last.end = end;
            return;
        }
        self.spans.push_back(SearchSpan {
            start,
            end,
            kind: SearchSpanKind::Appended,
        });
    }

    pub(in crate::viewer) fn shift_for_insert(&mut self, at: usize, lines: usize) {
        if lines == 0 {
            return;
        }
        let mut absorbed = false;
        for span in &mut self.spans {
            if span.start < at && at < span.end {
                span.end = span.end.saturating_add(lines);
                absorbed = true;
            } else if span.start >= at {
                span.start = span.start.saturating_add(lines);
                span.end = span.end.saturating_add(lines);
            }
        }
        self.known_line_count = self.known_line_count.saturating_add(lines);
        if absorbed {
            return;
        }

        let shifted_start = at.saturating_add(lines);
        if let Some(existing) = self
            .spans
            .iter_mut()
            .find(|span| span.kind != SearchSpanKind::Appended && span.start == shifted_start)
        {
            existing.start = at;
            return;
        }
        let insert_at = self
            .spans
            .iter()
            .position(|span| span.kind == SearchSpanKind::Wrapped)
            .unwrap_or(self.spans.len());
        self.spans.insert(
            insert_at,
            SearchSpan {
                start: at,
                end: shifted_start,
                kind: SearchSpanKind::Inserted,
            },
        );
    }

    fn observe_line_count(&mut self, line_count: usize) {
        if line_count > self.known_line_count {
            self.extend_for_append(self.known_line_count, line_count);
        }
    }

    fn current_span(&self) -> Option<SearchSpan> {
        self.spans.front().copied()
    }

    fn consume(&mut self, lines: usize) {
        let Some(span) = self.spans.front_mut() else {
            return;
        };
        let lines = lines.min(span.len());
        match self.direction {
            SearchDirection::Forward => span.start = span.start.saturating_add(lines),
            SearchDirection::Backward => span.end = span.end.saturating_sub(lines),
        }
        if span.start == span.end {
            self.spans.pop_front();
        }
    }

    fn has_work(&self) -> bool {
        !self.spans.is_empty()
    }

    #[cfg(test)]
    pub(in crate::viewer) fn span_count(&self) -> usize {
        self.spans.len()
    }
}

fn push_span(spans: &mut VecDeque<SearchSpan>, span: Option<SearchSpan>) {
    if let Some(span) = span {
        spans.push_back(span);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::viewer) struct SearchMatchIndex {
    pub(in crate::viewer) query: String,
    pub(in crate::viewer) counted_lines: usize,
    pub(in crate::viewer) matches: usize,
    pub(in crate::viewer) line_match_totals: Vec<usize>,
    pub(in crate::viewer) exact: bool,
}

impl SearchMatchIndex {
    fn new(query: String) -> Self {
        Self {
            query,
            counted_lines: 0,
            matches: 0,
            line_match_totals: Vec::new(),
            exact: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) enum SearchDirection {
    Forward,
    Backward,
}

pub(in crate::viewer) fn handle_search_input_key(
    code: KeyCode,
    modifiers: KeyModifiers,
    state: &mut ViewState,
    line_count: usize,
) -> bool {
    match code {
        KeyCode::Char(ch) if accepts_search_char(modifiers) => {
            state.search_buffer.push(ch);
            true
        }
        KeyCode::Enter => submit_search_buffer(state, line_count),
        KeyCode::Backspace => pop_search_char(state),
        KeyCode::Esc => cancel_search_prompt(state),
        _ => false,
    }
}

pub(in crate::viewer) fn start_search_prompt(state: &mut ViewState) -> bool {
    state.clear_tool_navigation();
    state.search_active = true;
    state.search_buffer.clear();
    state.search_task = None;
    state.search_target = None;
    state.search_cursor = None;
    state.search_match_ordinal = None;
    state.search_match_target = None;
    state.structure_cursor = None;
    true
}

pub(in crate::viewer) fn pop_search_char(state: &mut ViewState) -> bool {
    let old_len = state.search_buffer.len();
    state.search_buffer.pop();
    state.search_buffer.len() != old_len
}

pub(in crate::viewer) fn cancel_search_prompt(state: &mut ViewState) -> bool {
    state.search_active = false;
    state.search_buffer.clear();
    true
}

pub(in crate::viewer) fn clear_search_session(state: &mut ViewState) -> bool {
    let was_active = state.has_search_session() || state.footer_message.is_some();
    state.search_active = false;
    state.search_buffer.clear();
    state.search_query.clear();
    state.search_task = None;
    state.search_index = None;
    state.search_target = None;
    state.search_cursor = None;
    state.search_match_ordinal = None;
    state.search_match_target = None;
    state.clear_footer_message();
    was_active
}

pub(in crate::viewer) fn submit_search_buffer(state: &mut ViewState, line_count: usize) -> bool {
    if state.search_buffer.is_empty() {
        return false;
    }

    let query = state.search_buffer.clone();
    state.search_active = false;
    state.search_buffer.clear();
    start_search(
        state,
        query,
        SearchDirection::Forward,
        state.top,
        line_count,
    )
}

pub(in crate::viewer) fn start_repeat_search(
    state: &mut ViewState,
    line_count: usize,
    direction: SearchDirection,
) -> bool {
    if state.search_query.is_empty() {
        state.set_footer_message("no search query", FooterMessageKind::Warning);
        return true;
    }

    let start = repeat_search_start(
        state.search_cursor.unwrap_or(state.top),
        line_count,
        direction,
    );
    start_search(
        state,
        state.search_query.clone(),
        direction,
        start,
        line_count,
    )
}

pub(in crate::viewer) fn repeat_search_start(
    top: usize,
    line_count: usize,
    direction: SearchDirection,
) -> usize {
    if line_count == 0 {
        return 0;
    }

    match direction {
        SearchDirection::Forward => top.saturating_add(1) % line_count,
        SearchDirection::Backward => top.checked_sub(1).unwrap_or(line_count - 1),
    }
}

pub(in crate::viewer) fn start_search(
    state: &mut ViewState,
    query: String,
    direction: SearchDirection,
    start_line: usize,
    line_count: usize,
) -> bool {
    if query.is_empty() {
        return false;
    }

    state.clear_tool_navigation();
    state.search_query = query.clone();
    state.set_footer_message(format!("searching: {query}"), FooterMessageKind::Info);
    state.search_target = None;
    state.search_cursor = None;
    state.search_match_ordinal = None;
    state.search_match_target = None;
    state.structure_cursor = None;
    ensure_search_match_index(state, &query);
    if line_count == 0 {
        state.search_task = None;
        state.set_footer_message(format!("not found: {query}"), FooterMessageKind::Warning);
        return true;
    }

    state.search_task = Some(SearchTask::new(query, direction, start_line, line_count));
    true
}

fn ensure_search_match_index(state: &mut ViewState, query: &str) {
    if state
        .search_index
        .as_ref()
        .is_some_and(|index| index.query == query)
    {
        return;
    }

    state.search_index = Some(SearchMatchIndex::new(query.to_owned()));
}

pub(in crate::viewer) fn clear_footer_message(state: &mut ViewState) -> bool {
    state.clear_footer_message()
}

pub(in crate::viewer) fn process_search_index_step(
    file: &dyn ViewFile,
    state: &mut ViewState,
) -> Result<bool> {
    if state.search_query.is_empty() {
        state.search_index = None;
        return Ok(false);
    }

    let Some(mut index) = state.search_index.take() else {
        return Ok(false);
    };
    if index.query != state.search_query {
        state.search_index = Some(SearchMatchIndex::new(state.search_query.clone()));
        return Ok(true);
    }
    if index.exact {
        state.search_index = Some(index);
        return Ok(false);
    }

    let line_count = file.line_count();
    let old_counted_lines = index.counted_lines;
    let old_matches = index.matches;
    let old_exact = index.exact;

    if index.counted_lines < line_count {
        let count = (line_count - index.counted_lines).min(SEARCH_CHUNK_LINES);
        let lines = file.read_window(index.counted_lines, count)?;
        for line in &lines {
            index.matches = index
                .matches
                .saturating_add(line.matches(&index.query).count());
            index.line_match_totals.push(index.matches);
        }
        index.counted_lines = index.counted_lines.saturating_add(lines.len());
    }

    if file.line_count_exact() && index.counted_lines >= file.line_count() {
        index.exact = true;
    }

    let dirty = index.counted_lines != old_counted_lines
        || index.matches != old_matches
        || index.exact != old_exact;
    state.search_index = Some(index);
    let ordinal_dirty = update_search_match_ordinal(file, state)?;
    Ok(dirty || ordinal_dirty)
}

pub(in crate::viewer) fn process_search_step(
    file: &dyn ViewFile,
    state: &mut ViewState,
) -> Result<bool> {
    let Some(mut task) = state.search_task.take() else {
        return Ok(false);
    };

    task.observe_line_count(file.line_count());
    if task.direction == SearchDirection::Forward
        && file.has_older_records()
        && (task.awaiting_older
            || !task.has_work()
            || task.current_span().is_some_and(|span| {
                matches!(
                    span.kind,
                    SearchSpanKind::Inserted | SearchSpanKind::Wrapped
                )
            }))
    {
        task.awaiting_older = true;
    } else if !file.has_older_records() {
        task.awaiting_older = false;
    }

    if task.awaiting_older
        && file.has_older_records()
        && !task
            .current_span()
            .is_some_and(|span| span.kind == SearchSpanKind::Appended)
    {
        state.search_task = Some(task);
        return Ok(false);
    }

    if !task.has_work() {
        if file.has_older_records() {
            state.search_task = Some(task);
            return Ok(false);
        }
        state.search_target = None;
        state.search_match_ordinal = None;
        state.search_match_target = None;
        state.set_footer_message(
            format!("not found: {}", task.query),
            FooterMessageKind::Warning,
        );
        return Ok(true);
    }

    let step = scan_search_chunk(file, &task)?;
    if let Some(target) = step.found {
        state.search_cursor = Some(target.line);
        state.search_target = Some(target);
        state.search_match_target = Some(target);
        state.search_match_ordinal = search_match_ordinal_from_index(file, state, target)?;
        state.set_footer_message(format!("match: {}", task.query), FooterMessageKind::Info);
        return Ok(true);
    }

    task.consume(step.scanned);
    if !task.has_work() && file.has_older_records() {
        task.awaiting_older = true;
        state.search_task = Some(task);
        return Ok(false);
    }
    if !task.has_work() || step.scanned == 0 {
        state.search_target = None;
        state.search_match_ordinal = None;
        state.search_match_target = None;
        state.set_footer_message(
            format!("not found: {}", task.query),
            FooterMessageKind::Warning,
        );
        return Ok(true);
    }

    state.search_task = Some(task);
    Ok(false)
}

fn update_search_match_ordinal(file: &dyn ViewFile, state: &mut ViewState) -> Result<bool> {
    let previous = state.search_match_ordinal;
    state.search_match_ordinal = state
        .search_match_target
        .map(|target| search_match_ordinal_from_index(file, state, target))
        .transpose()?
        .flatten();
    Ok(state.search_match_ordinal != previous)
}

pub(in crate::viewer) fn search_match_ordinal_from_index(
    file: &dyn ViewFile,
    state: &ViewState,
    target: SearchTarget,
) -> Result<Option<usize>> {
    let Some(index) = state.search_index.as_ref() else {
        return Ok(None);
    };
    if index.query != state.search_query
        || index.query.is_empty()
        || target.line >= index.counted_lines
    {
        return Ok(None);
    };

    let lines = file.read_window(target.line, 1)?;
    let Some(target_line) = lines.first() else {
        return Ok(None);
    };
    let byte_index = floor_char_boundary(target_line, target.byte_index.min(target_line.len()));
    let previous_matches = if target.line == 0 {
        0
    } else {
        let Some(matches) = index.line_match_totals.get(target.line - 1) else {
            return Ok(None);
        };
        *matches
    };
    let matches_before_target = target_line[..byte_index].matches(&index.query).count();
    Ok(Some(
        previous_matches
            .saturating_add(matches_before_target)
            .saturating_add(1),
    ))
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct SearchStep {
    pub(in crate::viewer) found: Option<SearchTarget>,
    pub(in crate::viewer) scanned: usize,
}

pub(in crate::viewer) fn scan_search_chunk(
    file: &dyn ViewFile,
    task: &SearchTask,
) -> Result<SearchStep> {
    match task.direction {
        SearchDirection::Forward => scan_search_forward(file, task),
        SearchDirection::Backward => scan_search_backward(file, task),
    }
}

pub(in crate::viewer) fn scan_search_forward(
    file: &dyn ViewFile,
    task: &SearchTask,
) -> Result<SearchStep> {
    let Some(span) = task.current_span() else {
        return Ok(SearchStep {
            found: None,
            scanned: 0,
        });
    };
    let count = span.len().min(SEARCH_CHUNK_LINES);
    let lines = read_search_window(file, span.start, count)?;
    for (offset, line) in lines.iter().enumerate() {
        if let Some(byte_index) = line.find(&task.query) {
            return Ok(SearchStep {
                found: Some(SearchTarget {
                    line: span.start + offset,
                    byte_index,
                }),
                scanned: offset + 1,
            });
        }
    }

    Ok(SearchStep {
        found: None,
        scanned: lines.len(),
    })
}

pub(in crate::viewer) fn scan_search_backward(
    file: &dyn ViewFile,
    task: &SearchTask,
) -> Result<SearchStep> {
    let Some(span) = task.current_span() else {
        return Ok(SearchStep {
            found: None,
            scanned: 0,
        });
    };
    let count = span.len().min(SEARCH_CHUNK_LINES);
    let start = span.end.saturating_sub(count);
    let lines = read_search_window(file, start, count)?;
    for (offset, line) in lines.iter().enumerate().rev() {
        if let Some(byte_index) = line.rfind(&task.query) {
            return Ok(SearchStep {
                found: Some(SearchTarget {
                    line: start + offset,
                    byte_index,
                }),
                scanned: lines.len().saturating_sub(offset),
            });
        }
    }

    Ok(SearchStep {
        found: None,
        scanned: lines.len(),
    })
}

fn read_search_window(file: &dyn ViewFile, start: usize, count: usize) -> Result<Vec<String>> {
    let mut lines = Vec::with_capacity(count);
    while lines.len() < count {
        let remaining = count - lines.len();
        let Some(next_start) = start.checked_add(lines.len()) else {
            bail!("search window start overflow");
        };
        let chunk = file.read_window(next_start, remaining)?;
        if chunk.is_empty() {
            bail!(
                "view file returned an empty search window at line {next_start} with {remaining} lines remaining"
            );
        }
        if chunk.len() > remaining {
            bail!(
                "view file returned {} search lines for a {remaining}-line request",
                chunk.len()
            );
        }
        lines.extend(chunk);
    }
    Ok(lines)
}
