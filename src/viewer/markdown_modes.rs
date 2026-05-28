use anyhow::Result;

use crate::{
    formats::markdown::highlight::MarkdownFenceState, load::ViewFile, transform::FormatKind,
};

const MARKDOWN_MODE_CHECKPOINT_INTERVAL_LINES: usize = 512;
const MARKDOWN_MODE_SCAN_CHUNK_LINES: usize = 512;

#[derive(Debug, Default)]
pub(in crate::viewer) struct MarkdownModeCache {
    checkpoints: Vec<MarkdownModeCheckpoint>,
}

#[derive(Debug, Clone, Copy)]
struct MarkdownModeCheckpoint {
    line: usize,
    state: MarkdownFenceState,
}

impl MarkdownModeCache {
    pub(in crate::viewer) fn line_modes(
        &mut self,
        file: &dyn ViewFile,
        start: usize,
        lines: &[String],
        mode: FormatKind,
    ) -> Result<Option<Vec<FormatKind>>> {
        if mode != FormatKind::Markdown || lines.is_empty() {
            return Ok(None);
        }

        let mut state = self.state_before(file, start)?;
        let modes = lines
            .iter()
            .map(|line| {
                let line_mode = state.line_format(line);
                state.advance(line);
                line_mode
            })
            .collect();

        self.remember_interval_checkpoint(start.saturating_add(lines.len()), state);
        Ok(Some(modes))
    }

    fn state_before(&mut self, file: &dyn ViewFile, target: usize) -> Result<MarkdownFenceState> {
        let (mut line, mut state) = self
            .checkpoint_before(target)
            .map(|checkpoint| (checkpoint.line, checkpoint.state))
            .unwrap_or((0, MarkdownFenceState::default()));

        while line < target {
            let count = target
                .saturating_sub(line)
                .min(MARKDOWN_MODE_SCAN_CHUNK_LINES);
            let lines = file.read_window(line, count)?;
            if lines.is_empty() {
                break;
            }

            for source_line in &lines {
                self.remember_interval_checkpoint(line, state);
                state.advance(source_line);
                line += 1;
                if line >= target {
                    break;
                }
            }

            if lines.len() < count {
                break;
            }
        }

        self.remember_interval_checkpoint(line, state);
        Ok(state)
    }

    fn checkpoint_before(&self, target: usize) -> Option<MarkdownModeCheckpoint> {
        self.checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.line <= target)
            .copied()
    }

    fn remember_interval_checkpoint(&mut self, line: usize, state: MarkdownFenceState) {
        if line % MARKDOWN_MODE_CHECKPOINT_INTERVAL_LINES != 0 {
            return;
        }

        match self
            .checkpoints
            .binary_search_by_key(&line, |checkpoint| checkpoint.line)
        {
            Ok(position) => self.checkpoints[position].state = state,
            Err(position) => self
                .checkpoints
                .insert(position, MarkdownModeCheckpoint { line, state }),
        }
    }

    #[cfg(test)]
    pub(in crate::viewer) fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }
}
