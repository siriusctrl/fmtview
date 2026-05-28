mod diff;
mod file;

use std::io::{self, Write};

use anyhow::{Context, Result};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::backend::CrosstermBackend;

use crate::{load::ViewFile, transform::FormatKind, tui::screen::ViewerTerminal};

pub fn run(file: Box<dyn ViewFile>, mode: FormatKind, notice: Option<String>) -> Result<()> {
    run_terminal(|terminal| file::run_loop(terminal, file.as_ref(), mode, notice))
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
use file::TestViewerCaches as ViewerCaches;
#[cfg(test)]
use file::breadcrumb::JsonBreadcrumbCache;
#[cfg(test)]
use file::markdown_modes::MarkdownModeCache;
#[cfg(test)]
use file::position::{adjust_state_for_visible_height, resolve_targets_from_view};
#[cfg(test)]
use file::position::{
    resolve_search_target_position, resolve_structure_target_position, search_context_rows,
    visual_row_for_byte,
};
#[cfg(test)]
use file::structure::*;
#[cfg(test)]
use file::{
    MOUSE_HORIZONTAL_COLUMNS, MOUSE_SCROLL_LINES, NOTICE_DURATION, TAIL_ROW_OFFSET,
    WRAP_RENDER_CHUNK_ROWS,
};

#[cfg(test)]
mod tests;
