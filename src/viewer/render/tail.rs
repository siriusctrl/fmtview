use anyhow::Result;

use crate::load::ViewFile;

use super::super::input::ViewState;
use super::{
    line::rendered_row_count,
    types::{RenderContext, ViewPosition},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::viewer) struct TailPositionKey {
    pub(in crate::viewer) line_count: usize,
    pub(in crate::viewer) visible_height: usize,
    pub(in crate::viewer) width: usize,
}

#[derive(Debug, Default)]
pub(in crate::viewer) struct TailPositionCache {
    pub(in crate::viewer) key: Option<TailPositionKey>,
    pub(in crate::viewer) position: Option<ViewPosition>,
}

impl TailPositionCache {
    pub(in crate::viewer) fn position(
        &mut self,
        file: &dyn ViewFile,
        visible_height: usize,
        context: RenderContext,
    ) -> Result<ViewPosition> {
        if !context.wrap {
            return Ok(ViewPosition {
                top: last_full_logical_page_top(file.line_count(), visible_height),
                row_offset: 0,
            });
        }

        let key = TailPositionKey {
            line_count: file.line_count(),
            visible_height,
            width: context.width,
        };
        if self.key == Some(key) {
            if let Some(position) = self.position {
                return Ok(position);
            }
        }

        let position = compute_tail_position(file, visible_height, context)?;
        self.key = Some(key);
        self.position = Some(position);
        Ok(position)
    }
}

pub(in crate::viewer) fn compute_tail_position(
    file: &dyn ViewFile,
    visible_height: usize,
    context: RenderContext,
) -> Result<ViewPosition> {
    let line_count = file.line_count();
    if line_count == 0 || visible_height == 0 {
        return Ok(ViewPosition {
            top: 0,
            row_offset: 0,
        });
    }

    if !context.wrap {
        return Ok(ViewPosition {
            top: last_full_logical_page_top(line_count, visible_height),
            row_offset: 0,
        });
    }

    let mut needed_rows = visible_height;
    let mut end = line_count;
    while end > 0 {
        let start = end.saturating_sub(visible_height.max(32));
        let lines = file.read_window(start, end - start)?;
        for (index, line) in lines.iter().enumerate().rev() {
            let line_index = start + index;
            let rows = rendered_row_count(line, context);
            if rows >= needed_rows {
                return Ok(ViewPosition {
                    top: line_index,
                    row_offset: rows - needed_rows,
                });
            }
            needed_rows -= rows;
        }
        end = start;
    }

    Ok(ViewPosition {
        top: 0,
        row_offset: 0,
    })
}

pub(in crate::viewer) fn last_full_logical_page_top(
    line_count: usize,
    visible_height: usize,
) -> usize {
    line_count.saturating_sub(visible_height.max(1))
}

pub(in crate::viewer) fn is_after_tail(state: &ViewState, tail: ViewPosition) -> bool {
    state.top > tail.top || (state.top == tail.top && state.top_row_offset > tail.row_offset)
}
