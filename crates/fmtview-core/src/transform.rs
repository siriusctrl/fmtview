mod detect;
mod engine;
mod types;

#[cfg(test)]
mod tests;

pub(crate) const IO_BUFFER_BYTES: usize = 256 * 1024;

#[cfg(test)]
pub(crate) use engine::format_record_bytes;
pub(crate) use engine::format_record_display_bytes;
#[cfg(test)]
pub(crate) use engine::format_record_to_string;
#[cfg(test)]
pub(crate) use engine::format_source_to_temp;
pub(crate) use engine::transform_source_to_temp;
pub(crate) use engine::trim_record_line_end;
pub(crate) use engine::{format_record_lines, parseable_record_line};
pub(crate) use types::TransformStrategy;
pub use types::{FormatKind, FormatOptions};
