use crate::load::ViewFile;

use super::types::{RenderContext, ViewportBottom};

pub(in crate::viewer) fn line_number_digits(line_count: usize) -> usize {
    line_count.max(1).to_string().len()
}

pub(in crate::viewer) fn viewer_progress_percent(
    file: &dyn ViewFile,
    context: RenderContext,
    logical_bottom: usize,
    viewport_bottom: Option<ViewportBottom>,
) -> usize {
    if !context.wrap {
        return progress_percent(logical_bottom, file.line_count());
    }

    let bottom = viewport_bottom
        .map(|bottom| viewport_bottom_byte_offset(file, bottom))
        .unwrap_or(0);
    byte_progress_percent(bottom, file.byte_len())
}

pub(in crate::viewer) fn viewport_bottom_byte_offset(
    file: &dyn ViewFile,
    bottom: ViewportBottom,
) -> u64 {
    if bottom.line_end {
        if bottom.line_index + 1 >= file.line_count() {
            return file.byte_len();
        }
        return file.byte_offset_for_line(bottom.line_index + 1);
    }

    file.byte_offset_for_line(bottom.line_index)
        .saturating_add(bottom.byte_end as u64)
}

pub(in crate::viewer) fn progress_percent(bottom: usize, line_count: usize) -> usize {
    bottom
        .saturating_mul(100)
        .checked_div(line_count)
        .unwrap_or(100)
}

pub(in crate::viewer) fn byte_progress_percent(position: u64, total: u64) -> usize {
    if total == 0 {
        return 100;
    }

    position
        .min(total)
        .saturating_mul(100)
        .checked_div(total)
        .unwrap_or(100) as usize
}
