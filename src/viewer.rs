mod breadcrumb;
mod diff;
mod file;
mod input;
mod markdown_modes;
mod position;
mod render;
mod structure;

use std::io::{self, Write};

use anyhow::{Context, Result};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::backend::CrosstermBackend;

use crate::{load::ViewFile, transform::FormatKind, tui::screen::ViewerTerminal};

const EVENT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);
const EVENT_DRAIN_BUDGET: std::time::Duration = std::time::Duration::from_millis(8);
const EVENT_DRAIN_LIMIT: usize = 512;
const MOUSE_SCROLL_LINES: usize = 1;
const MOUSE_HORIZONTAL_COLUMNS: usize = 4;
const RENDER_CACHE_MAX_LINES: usize = 512;
const RENDER_CACHE_MAX_ROWS_PER_LINE: usize = 256;
const WRAP_RENDER_CHUNK_ROWS: usize = 64;
const WRAP_RENDER_CHUNKS_PER_LINE: usize = 64;
const TERMINAL_SCROLL_HINT_MAX_ROWS: usize = 12;
const WRAP_PREWARM_LOGICAL_LINES: usize = 4;
const PREWARM_PAGES: usize = 2;
const PREWARM_MAX_LINES: usize = 192;
const PREWARM_MAX_LINE_BYTES: usize = 16 * 1024;
const PREWARM_BUDGET: std::time::Duration = std::time::Duration::from_millis(4);
const LAZY_PRELOAD_LINES: usize = 4096;
const LAZY_PRELOAD_RECORDS: usize = 64;
const LAZY_PRELOAD_BUDGET: std::time::Duration = std::time::Duration::from_millis(6);
const JUMP_BUFFER_MAX_DIGITS: usize = 20;
const SEARCH_CHUNK_LINES: usize = 4096;
const TAIL_ROW_OFFSET: usize = usize::MAX;

pub fn run(file: Box<dyn ViewFile>, mode: FormatKind) -> Result<()> {
    run_terminal(|terminal| file::run_loop(terminal, file.as_ref(), mode))
}

pub(crate) fn run_diff(view: crate::diff::DiffView) -> Result<()> {
    run_terminal(|terminal| diff::run_loop(terminal, view))
}

fn run_terminal<F>(run_loop: F) -> Result<()>
where
    F: FnOnce(&mut ViewerTerminal<CrosstermBackend<io::Stdout>>) -> Result<()>,
{
    let mut stdout = io::stdout();
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut cleanup = TerminalCleanup::active();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ViewerTerminal::new(backend);
    let result = run_loop(&mut terminal);

    disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )
    .ok();
    cleanup.disarm();
    terminal.show_cursor().ok();

    result
}

struct TerminalCleanup {
    active: bool,
}

impl TerminalCleanup {
    fn active() -> Self {
        Self { active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        if !self.active {
            return;
        }

        disable_raw_mode().ok();
        let mut stdout = io::stdout();
        execute!(stdout, DisableMouseCapture, LeaveAlternateScreen).ok();
        stdout.flush().ok();
    }
}

#[cfg(test)]
use crate::load::IndexedTempFile;
#[cfg(test)]
use crate::tui::{
    palette::*,
    screen::{TerminalFrame, draw_cells},
};
#[cfg(test)]
use breadcrumb::JsonBreadcrumbCache;
#[cfg(test)]
use file::TestViewerCaches as ViewerCaches;
#[cfg(test)]
use markdown_modes::MarkdownModeCache;
#[cfg(test)]
use position::{adjust_state_for_visible_height, resolve_targets_from_view};
#[cfg(test)]
use position::{
    resolve_search_target_position, resolve_structure_target_position, search_context_rows,
    visual_row_for_byte,
};
#[cfg(test)]
use structure::*;

#[cfg(test)]
mod tests;
