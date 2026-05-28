mod detect;
mod engine;
mod json;
mod types;
mod xml;

#[cfg(test)]
mod tests;

const IO_BUFFER_BYTES: usize = 256 * 1024;

#[cfg(test)]
pub(crate) use engine::format_record_to_string;
#[cfg(test)]
pub(crate) use engine::format_source_to_temp;
pub(crate) use engine::{format_record_bytes, format_record_lines, parseable_record_line};
pub(crate) use engine::{transform_source_to_temp, trim_record_line_end};
pub(crate) use types::{FormatKind, FormatOptions, TransformStrategy};
