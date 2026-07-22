//! Headless viewer engine for the `fmtview` command-line application.

mod diff;
mod formats;
mod input;
mod load;
#[cfg(test)]
mod perf;
mod profile;
mod timeline;
mod transform;
mod tui;
mod viewer;

pub use diff::{DiffView, diff_sources, diff_view};
pub use formats::ContentShape;
pub use input::InputSource;
pub use load::{
    LoadPlan, OpenedViewFile, RecordTimelineViewFile, ViewFile, ViewFileChange,
    open_follow_view_file, open_view_file, open_view_file_with_fallback,
};
pub use profile::TypeProfile;
pub use timeline::{
    FileRecordTimeline, FileTimelineInstrumentation, RecordId, RecordLoadLimit, RecordTimeline,
    TimelineRead, TimelineReadNext, TimelineRecord, TimelineRefresh, TimelineResetReason,
    TimelineSnapshot,
};
pub use transform::{FormatKind, FormatOptions};
pub use tui::screen::{RenderFrame, ScrollDirection, ScrollHint, ScrollPosition};
pub use viewer::{
    DiffViewer, FileViewer, InputEvent, KeyCode, KeyModifiers, MouseEventKind, ViewerAction,
    ViewerCommand,
};

/// Transform one source according to an already resolved profile.
pub fn transform_source_to_temp(
    source: &InputSource,
    options: &FormatOptions,
    profile: TypeProfile,
) -> anyhow::Result<tempfile::NamedTempFile> {
    transform::transform_source_to_temp(source, options, profile.transform)
}

/// Paint a backend-neutral frame into a ratatui buffer without a terminal.
pub fn render_frame_to_buffer(buffer: &mut ratatui::buffer::Buffer, frame: RenderFrame) {
    tui::buffer_frame::render_frame(
        buffer,
        frame.styled,
        frame.sticky,
        frame.selection_mode,
        frame.title,
        frame.footer_text,
        frame.footer_style,
    );
}
